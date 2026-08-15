//! Rendering a template tree into a Git tree.
//!
//! The output is written straight into Git objects, never to the filesystem.
//! That is what makes "`update` does not touch your working tree" structural
//! rather than a promise: there is no code path here that opens a file for
//! writing in the project.
//!
//! Rendering is deterministic — see `docs/concepts/determinism.md` for the
//! full list of hazards and how each is handled.

use std::sync::Arc;

use miette::Diagnostic;
use thiserror::Error;

use crate::context::Context;
use crate::eval::{EvalError, Partials, render_string};
use crate::git::{FileMode, GitBackend, Oid, TreeEntry};

/// The suffix marking a file as a template.
pub const TEMPLATE_SUFFIX: &str = ".jinja";

/// How much of a file to inspect when deciding whether it is binary.
///
/// A NUL in the first 8 KiB is the same heuristic Git uses. Rendering a PNG
/// would corrupt it, and the corruption would be silent.
const BINARY_SNIFF_LEN: usize = 8192;

/// Errors from rendering.
#[derive(Debug, Error, Diagnostic)]
pub enum RenderError {
    /// A file's contents failed to render.
    #[error("failed to render `{path}`")]
    #[diagnostic(code(tpl::render::content))]
    Content {
        /// The template path.
        path: String,
        /// Why.
        #[source]
        #[diagnostic_source]
        source: EvalError,
    },

    /// A path segment failed to render.
    #[error("failed to render the path `{path}`")]
    #[diagnostic(code(tpl::render::path))]
    Path {
        /// The template path.
        path: String,
        /// Why.
        #[source]
        #[diagnostic_source]
        source: EvalError,
    },

    /// A rendered path would escape the tree.
    #[error("`{path}` renders to `{rendered}`, which escapes the tree")]
    #[diagnostic(
        code(tpl::render::escapes_tree),
        help(
            "a rendered path segment may not be `.`, `..`, absolute, or contain a `/`. \
             Use separate directories in the template instead."
        )
    )]
    EscapesTree {
        /// The template path.
        path: String,
        /// What it rendered to.
        rendered: String,
    },

    /// Two template paths rendered to the same output path.
    #[error("`{first}` and `{second}` both render to `{rendered}`")]
    #[diagnostic(
        code(tpl::render::collision),
        help("two template files cannot produce the same output file")
    )]
    Collision {
        /// The path that got there first.
        first: String,
        /// The path that collided with it.
        second: String,
        /// The output path they share.
        rendered: String,
    },

    /// A Git operation failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Git(#[from] crate::git::GitError),

    /// A partial's bytes are not valid UTF-8.
    #[error("the partial `{path}` is not valid UTF-8")]
    #[diagnostic(
        code(tpl::render::partial_not_utf8),
        help(
            "a `{TEMPLATE_SUFFIX}` file outside the render root is a partial, and a partial \
             must be text. Move it inside the render root if it is meant to be rendered and \
             copied as-is."
        )
    )]
    PartialNotUtf8 {
        /// The path in the template repository.
        path: String,
    },
}

/// Collect the partials a template makes importable.
///
/// A partial is any `TEMPLATE_SUFFIX` blob **outside** the render root, named
/// by its repository-root-relative path. Outside the root is what makes it a
/// partial: the tree walk only ever sees the root subtree, so a macro
/// definition cannot leak into the rendered project and no skip rule is needed
/// to keep it out. Restricting to `.jinja` bounds what is read eagerly and
/// keeps data files the business of `[data]`, which knows how to parse them.
///
/// Lossy decoding is deliberately not used. A binary `.jinja` outside the root
/// is an authoring mistake, and silently importing replacement characters would
/// surface later as an incomprehensible parse error.
pub fn collect_partials(
    template: &dyn GitBackend,
    tree: Oid,
    root: &str,
) -> Result<Partials, RenderError> {
    // `list_tree` is already in Git-canonical sorted order, and `Partials` is a
    // `BTreeMap`, so the resulting name order does not vary between runs.
    let entries = template.list_tree(tree)?;

    let prefix = format!("{}/", root.trim_end_matches('/'));
    let mut out = Vec::new();

    for entry in entries {
        if !entry.mode.is_blob() || entry.mode == FileMode::Link {
            continue;
        }
        if entry.path.starts_with(&prefix) || entry.path == root {
            continue;
        }
        if !entry.path.ends_with(TEMPLATE_SUFFIX) {
            continue;
        }

        let bytes = template.read_blob(entry.oid)?;
        let source = String::from_utf8(bytes).map_err(|_| RenderError::PartialNotUtf8 {
            path: entry.path.clone(),
        })?;
        out.push((entry.path, source));
    }

    Ok(Partials::new(out))
}

/// A rendered file, before it becomes a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The output path.
    pub path: String,
    /// The output bytes.
    pub content: Vec<u8>,
    /// Whether the executable bit is set.
    pub executable: bool,
    /// Whether the file went through MiniJinja, or was copied byte-for-byte.
    ///
    /// Reported because it is the one thing about a rendering an author cannot
    /// see in the output: a workflow full of `${{ }}` that survived intact and
    /// one that was never rendered look identical, and only the second is
    /// correct by construction.
    pub templated: bool,
}

/// Render a template tree against a context, producing a Git tree.
///
/// `entries` must be the flattened template subtree, in Git tree order. The
/// caller supplies it rather than a directory, because reading from a Git tree
/// is what pins the template revision and gives a deterministic traversal
/// order — `readdir` order varies by filesystem.
///
/// The source and destination repositories are separate parameters because they
/// genuinely are separate: the template's blobs live in the template
/// repository — often a temporary clone — while the rendered tree must be
/// written into the project, which is where the ref will point.
pub fn render_tree(
    template: &dyn GitBackend,
    project: &dyn GitBackend,
    entries: &[TreeEntry],
    context: &Context,
    partials: &Arc<Partials>,
) -> Result<Oid, RenderError> {
    let rendered = render_entries(template, entries, context, partials)?;
    write_tree(project, &rendered)
}

/// Write already-rendered files into `project` as a tree.
///
/// Split from [`render_tree`] because it is the only part of a rendering that
/// needs a repository to write into: `git tpl render --output` produces the
/// same `Rendered` values and writes them to a directory instead.
pub fn write_tree(project: &dyn GitBackend, rendered: &[Rendered]) -> Result<Oid, RenderError> {
    let mut tree_entries = Vec::with_capacity(rendered.len());
    for file in rendered {
        let oid = project.write_blob(&file.content)?;
        tree_entries.push(TreeEntry {
            path: file.path.clone(),
            oid,
            mode: if file.executable {
                FileMode::BlobExecutable
            } else {
                FileMode::Blob
            },
        });
    }

    Ok(project.build_tree(&tree_entries)?)
}

/// Render every entry, resolving paths and contents.
///
/// Reads blobs from `template`; produces bytes, not Git objects.
pub fn render_entries(
    template: &dyn GitBackend,
    entries: &[TreeEntry],
    context: &Context,
    partials: &Arc<Partials>,
) -> Result<Vec<Rendered>, RenderError> {
    // Sorted input in, sorted output out. Rendering can reorder paths — a
    // templated segment may render to anything — so the result is re-sorted
    // below rather than assumed.
    let mut sorted: Vec<&TreeEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    let mut out: Vec<Rendered> = Vec::with_capacity(sorted.len());
    let mut seen: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();

    for entry in sorted {
        // Symlinks are copied as-is: their content is the target path, and
        // rendering it would silently repoint the link.
        if !entry.mode.is_blob() && entry.mode != FileMode::Link {
            continue;
        }

        let Some(path) = render_path(&entry.path, context, partials)? else {
            // A segment rendered empty, so this entry — and, for a directory,
            // everything beneath it — is skipped. That is how a template makes
            // a whole subtree conditional.
            continue;
        };

        let source = template.read_blob(entry.oid)?;
        // A binary blob is copied even when it is named `.jinja`, so
        // "was it rendered" is not the same question as "is it named like a
        // template". This is the answer to the first.
        let templated = entry.path.ends_with(TEMPLATE_SUFFIX) && !is_binary(&source);

        let content = if templated {
            let text = String::from_utf8_lossy(&source);
            render_string(&text, context, &entry.path, partials)
                .map_err(|source| RenderError::Content {
                    path: entry.path.clone(),
                    source,
                })?
                .into_bytes()
        } else {
            // Copied byte-for-byte. No line-ending translation, ever: a `\r\n`
            // in the template stays `\r\n` on every platform, or the same
            // inputs would produce different trees on Windows and Linux.
            source
        };

        if let Some(first) = seen.get(&path) {
            return Err(RenderError::Collision {
                first: first.clone(),
                second: entry.path.clone(),
                rendered: path,
            });
        }
        seen.insert(path.clone(), entry.path.clone());

        out.push(Rendered {
            path,
            content,
            templated,
            // Git records nothing about permissions except the executable bit,
            // so it is the only one that can be carried.
            executable: entry.mode == FileMode::BlobExecutable,
        });
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Render a path, stripping `.jinja` and evaluating each segment.
///
/// Returns `None` when a segment renders empty, meaning the entry is skipped.
fn render_path(
    path: &str,
    context: &Context,
    partials: &Arc<Partials>,
) -> Result<Option<String>, RenderError> {
    // Strip the suffix before rendering, so a templated directory name ending
    // in `.jinja` is not mistaken for a template file.
    let stripped = path.strip_suffix(TEMPLATE_SUFFIX).unwrap_or(path);

    let mut segments = Vec::new();
    for segment in stripped.split('/') {
        let rendered = render_string(segment, context, path, partials).map_err(|source| {
            RenderError::Path {
                path: path.to_string(),
                source,
            }
        })?;
        let rendered = rendered.trim().to_string();

        if rendered.is_empty() {
            return Ok(None);
        }

        // Rejected rather than resolved. A template repository is untrusted
        // input, and `..` here is a request to write outside the tree. The tree
        // builder would reject it too, but the error would name a libgit2
        // internal rather than the template file that caused it.
        if rendered == "." || rendered == ".." || rendered.contains('/') || rendered.contains('\\')
        {
            return Err(RenderError::EscapesTree {
                path: path.to_string(),
                rendered,
            });
        }

        segments.push(rendered);
    }

    if segments.is_empty() {
        return Ok(None);
    }

    Ok(Some(segments.join("/")))
}

/// Whether content should be treated as binary.
fn is_binary(content: &[u8]) -> bool {
    content.iter().take(BINARY_SNIFF_LEN).any(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::no_partials;
    use crate::git::libgit2::LibGit2;
    use crate::template::Value;

    struct Fixture {
        _dir: tempfile::TempDir,
        repo: LibGit2,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = LibGit2::init(dir.path()).unwrap();
            Self { _dir: dir, repo }
        }

        fn entry(&self, path: &str, content: &[u8]) -> TreeEntry {
            TreeEntry {
                path: path.to_string(),
                oid: self.repo.write_blob(content).unwrap(),
                mode: FileMode::Blob,
            }
        }

        fn executable(&self, path: &str, content: &[u8]) -> TreeEntry {
            TreeEntry {
                mode: FileMode::BlobExecutable,
                ..self.entry(path, content)
            }
        }

        fn render(&self, entries: &[TreeEntry], context: &Context) -> Vec<Rendered> {
            render_entries(&self.repo, entries, context, no_partials()).unwrap()
        }

        fn tree(&self, entries: &[TreeEntry], context: &Context) -> Oid {
            render_tree(&self.repo, &self.repo, entries, context, no_partials()).unwrap()
        }
    }

    fn context() -> Context {
        let mut context = Context::new();
        context.set_answer("project_name", Value::String("Demo".into()));
        context.set_answer("ci", Value::Bool(true));
        context.set_computed("package_name", Value::String("demo".into()));
        context
    }

    #[test]
    fn a_jinja_file_is_rendered_and_loses_its_suffix() {
        let f = Fixture::new();
        let entries = [f.entry("README.md.jinja", b"# {{ project_name }}\n")];

        let rendered = f.render(&entries, &context());

        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].path, "README.md");
        assert_eq!(rendered[0].content, b"# Demo\n");
    }

    /// GitHub Actions files are full of `${{ }}`. A tool that rendered every
    /// file would mangle them, which is why only `.jinja` is rendered.
    #[test]
    fn a_non_jinja_file_is_copied_byte_for_byte() {
        let f = Fixture::new();
        let workflow = b"run: echo ${{ github.sha }}\n";
        let entries = [f.entry(".github/workflows/ci.yml", workflow)];

        let rendered = f.render(&entries, &context());

        assert_eq!(rendered[0].path, ".github/workflows/ci.yml");
        assert_eq!(rendered[0].content, workflow);
    }

    #[test]
    fn only_the_final_jinja_suffix_is_stripped() {
        let f = Fixture::new();
        let entries = [f.entry("a.jinja.jinja", b"x")];
        assert_eq!(f.render(&entries, &context())[0].path, "a.jinja");
    }

    #[test]
    fn a_templated_path_segment_is_rendered() {
        let f = Fixture::new();
        let entries = [f.entry(
            "src/{{ package_name }}/mod.rs.jinja",
            b"// {{ project_name }}",
        )];

        let rendered = f.render(&entries, &context());

        assert_eq!(rendered[0].path, "src/demo/mod.rs");
        assert_eq!(rendered[0].content, b"// Demo");
    }

    /// A segment rendering empty is how a template makes a whole subtree
    /// conditional.
    #[test]
    fn a_segment_rendering_empty_skips_the_entry() {
        let f = Fixture::new();
        let mut context = context();
        context.set_answer("ci", Value::Bool(false));

        let entries = [
            f.entry("{% if ci %}.github{% endif %}/workflows/ci.yml", b"jobs:"),
            f.entry("README.md", b"keep"),
        ];

        let rendered = f.render(&entries, &context);

        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].path, "README.md");
    }

    #[test]
    fn a_conditional_subtree_is_kept_when_the_condition_holds() {
        let f = Fixture::new();
        let entries = [f.entry("{% if ci %}.github{% endif %}/workflows/ci.yml", b"jobs:")];

        let rendered = f.render(&entries, &context());

        assert_eq!(rendered[0].path, ".github/workflows/ci.yml");
    }

    /// A template repository is untrusted input, and `..` in a rendered path is
    /// a request to write outside the tree.
    #[test]
    fn a_path_that_would_escape_the_tree_is_refused() {
        let f = Fixture::new();
        let mut context = Context::new();
        context.set_answer("evil", Value::String("..".into()));

        let error = render_entries(
            &f.repo,
            &[f.entry("{{ evil }}/x", b"x")],
            &context,
            no_partials(),
        )
        .unwrap_err();

        assert!(
            matches!(error, RenderError::EscapesTree { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_path_segment_rendering_a_separator_is_rejected() {
        let f = Fixture::new();
        let mut context = Context::new();
        context.set_answer("evil", Value::String("a/b".into()));

        let error = render_entries(
            &f.repo,
            &[f.entry("{{ evil }}/x", b"x")],
            &context,
            no_partials(),
        )
        .unwrap_err();

        assert!(
            matches!(error, RenderError::EscapesTree { .. }),
            "{error:?}"
        );
    }

    /// Rendering a PNG would corrupt it, and the corruption would be silent.
    #[test]
    fn a_binary_file_is_copied_even_when_named_jinja() {
        let f = Fixture::new();
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d{{ project_name }}";
        let entries = [f.entry("logo.png.jinja", png)];

        let rendered = f.render(&entries, &context());

        assert_eq!(rendered[0].path, "logo.png");
        assert_eq!(rendered[0].content, png, "binary content must be untouched");
    }

    #[test]
    fn the_executable_bit_is_preserved() {
        let f = Fixture::new();
        let entries = [
            f.executable(
                "scripts/build.sh.jinja",
                b"#!/bin/sh\necho {{ project_name }}\n",
            ),
            f.entry("README.md", b"x"),
        ];

        let rendered = f.render(&entries, &context());

        let script = rendered
            .iter()
            .find(|r| r.path == "scripts/build.sh")
            .unwrap();
        let readme = rendered.iter().find(|r| r.path == "README.md").unwrap();
        assert!(script.executable);
        assert!(!readme.executable);
    }

    /// A tool that normalised line endings would produce different trees on
    /// Windows and Linux from the same template.
    #[test]
    fn line_endings_are_never_translated() {
        let f = Fixture::new();
        let entries = [
            f.entry("crlf.txt", b"a\r\nb\r\n"),
            f.entry("crlf.txt.jinja", b"{{ project_name }}\r\nx\r\n"),
        ];

        let rendered = render_entries(&f.repo, &entries[..1], &context(), no_partials());
        assert_eq!(rendered.unwrap()[0].content, b"a\r\nb\r\n");

        let rendered = render_entries(&f.repo, &entries[1..], &context(), no_partials());
        assert_eq!(rendered.unwrap()[0].content, b"Demo\r\nx\r\n");
    }

    #[test]
    fn output_is_ordered_by_path_regardless_of_input_order() {
        let f = Fixture::new();
        let entries = [
            f.entry("z.txt", b"z"),
            f.entry("a.txt", b"a"),
            f.entry("m/b.txt", b"b"),
        ];

        let paths: Vec<_> = f
            .render(&entries, &context())
            .into_iter()
            .map(|r| r.path)
            .collect();

        assert_eq!(paths, ["a.txt", "m/b.txt", "z.txt"]);
    }

    #[test]
    fn two_templates_rendering_to_one_path_is_an_error() {
        let f = Fixture::new();
        let mut context = Context::new();
        context.set_answer("name", Value::String("same".into()));

        let error = render_entries(
            &f.repo,
            &[f.entry("{{ name }}.txt", b"a"), f.entry("same.txt", b"b")],
            &context,
            no_partials(),
        )
        .unwrap_err();

        assert!(matches!(error, RenderError::Collision { .. }), "{error:?}");
    }

    #[test]
    fn a_failing_template_names_the_file_it_came_from() {
        let f = Fixture::new();
        let error = render_entries(
            &f.repo,
            &[f.entry("bad.md.jinja", b"{{ 'x' | no_such_filter }}")],
            &context(),
            no_partials(),
        )
        .unwrap_err();

        match error {
            RenderError::Content { path, .. } => assert_eq!(path, "bad.md.jinja"),
            other => panic!("expected a content error, got {other:?}"),
        }
    }

    /// The guarantee the whole ref model rests on: identical inputs, identical
    /// tree. A rendering that varied would commit on every run.
    #[test]
    fn rendering_the_same_inputs_twice_produces_the_same_tree() {
        let f = Fixture::new();
        let entries = [
            f.entry("README.md.jinja", b"# {{ project_name }}\n"),
            f.entry(
                "src/{{ package_name }}/mod.rs.jinja",
                b"// {{ package_name }}",
            ),
            f.executable("run.sh", b"#!/bin/sh\n"),
            f.entry("logo.png", b"\x89PNG\x00\x01\x02"),
        ];

        let first = f.tree(&entries, &context());
        let second = f.tree(&entries, &context());

        assert_eq!(first, second);
    }

    #[test]
    fn a_different_answer_produces_a_different_tree() {
        let f = Fixture::new();
        let entries = [f.entry("README.md.jinja", b"# {{ project_name }}\n")];

        let first = f.tree(&entries, &context());

        let mut other = context();
        other.set_answer("project_name", Value::String("Other".into()));
        let second = f.tree(&entries, &other);

        assert_ne!(first, second);
    }

    #[test]
    fn an_empty_template_renders_to_an_empty_tree() {
        let f = Fixture::new();
        let tree = f.tree(&[], &context());
        assert!(f.repo.list_tree(tree).unwrap().is_empty());
    }

    #[test]
    fn binary_detection_only_looks_at_the_start_of_a_file() {
        assert!(!is_binary(b"plain text"));
        assert!(is_binary(b"text\x00more"));
        // A NUL beyond the sniff window is not detected, which matches Git and
        // keeps the check bounded on large files.
        let mut late = vec![b'a'; BINARY_SNIFF_LEN + 10];
        late.push(0);
        assert!(!is_binary(&late));
    }

    /// Fixture helpers for the loader: a template tree with files both inside
    /// and outside the render root.
    impl Fixture {
        /// Build a tree from `path -> content` pairs and collect its partials.
        fn partials(&self, root: &str, files: &[(&str, &[u8])]) -> Partials {
            let entries: Vec<TreeEntry> = files
                .iter()
                .map(|(path, content)| self.entry(path, content))
                .collect();
            let tree = self.repo.build_tree(&entries).unwrap();
            collect_partials(&self.repo, tree, root).unwrap()
        }

        fn render_with(
            &self,
            entries: &[TreeEntry],
            context: &Context,
            partials: &Arc<Partials>,
        ) -> Result<Vec<Rendered>, RenderError> {
            render_entries(&self.repo, entries, context, partials)
        }
    }

    #[test]
    fn a_macro_imported_from_outside_the_root_is_expanded() {
        let f = Fixture::new();
        let partials = Arc::new(f.partials(
            "template",
            &[
                (
                    "macros.jinja",
                    b"{% macro title(name) %}# {{ name }}{% endmacro %}",
                ),
                ("template/README.md.jinja", b"unused"),
            ],
        ));

        let rendered = f
            .render_with(
                &[f.entry(
                    "README.md.jinja",
                    b"{% import 'macros.jinja' as m %}{{ m.title(project_name) }}\n",
                )],
                &context(),
                &partials,
            )
            .unwrap();

        assert_eq!(rendered[0].path, "README.md");
        assert_eq!(rendered[0].content, b"# Demo\n");
    }

    #[test]
    fn a_partial_in_a_subdirectory_is_named_by_its_full_path() {
        let f = Fixture::new();
        let partials = Arc::new(f.partials(
            "template",
            &[(
                "macros/rust.jinja",
                b"{% macro crate_name(n) %}{{ n }}-rs{% endmacro %}",
            )],
        ));

        let rendered = f
            .render_with(
                &[f.entry(
                    "Cargo.toml.jinja",
                    b"{% import 'macros/rust.jinja' as m %}{{ m.crate_name(package_name) }}",
                )],
                &context(),
                &partials,
            )
            .unwrap();

        assert_eq!(rendered[0].content, b"demo-rs");
    }

    /// Partials live outside the render root, so the tree walk — which only
    /// ever sees the root subtree — cannot emit one. This pins the property
    /// that makes the whole design need no skip rule.
    #[test]
    fn a_partial_is_not_written_into_the_rendered_tree() {
        let f = Fixture::new();
        let partials = Arc::new(f.partials(
            "template",
            &[
                ("macros.jinja", b"{% macro x() %}x{% endmacro %}"),
                ("template/README.md.jinja", b"hello"),
            ],
        ));
        assert_eq!(partials.names().collect::<Vec<_>>(), ["macros.jinja"]);

        // The entries the walk is given are the *root subtree*, already
        // relative to the root — `macros.jinja` is simply not among them.
        let rendered = f
            .render_with(
                &[f.entry("README.md.jinja", b"hello")],
                &context(),
                &partials,
            )
            .unwrap();

        assert_eq!(
            rendered.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            ["README.md"]
        );
    }

    #[test]
    fn a_jinja_file_inside_the_root_is_not_a_partial() {
        let f = Fixture::new();
        let partials = f.partials(
            "template",
            &[
                ("macros.jinja", b"outside"),
                ("template/nested.jinja", b"inside"),
                ("template/deep/nested.jinja", b"inside"),
            ],
        );

        assert_eq!(partials.names().collect::<Vec<_>>(), ["macros.jinja"]);
    }

    /// A non-`.jinja` file outside the root is not loadable. Reading data files
    /// is what `[data]` is for, and it knows how to parse them.
    #[test]
    fn a_non_jinja_file_outside_the_root_is_not_a_partial() {
        let f = Fixture::new();
        let partials = f.partials(
            "template",
            &[
                ("template.toml", b"name = 'demo'"),
                ("data/licenses.toml", b"ids = []"),
                ("README.md", b"the template's own readme"),
            ],
        );

        assert!(partials.is_empty());
    }

    /// What the author does not already know is which names *do* exist —
    /// nearly always a typo, or a path written relative to the render root.
    #[test]
    fn an_unknown_import_names_the_partials_that_do_exist() {
        let f = Fixture::new();
        let partials = Arc::new(f.partials(
            "template",
            &[("macros.jinja", b"x"), ("macros/rust.jinja", b"y")],
        ));

        let error = f
            .render_with(
                &[f.entry("README.md.jinja", b"{% import 'marcos.jinja' as m %}")],
                &context(),
                &partials,
            )
            .unwrap_err();

        let message = format!("{:?}", miette::Report::new(error));
        assert!(message.contains("marcos.jinja"), "{message}");
        assert!(
            message.contains("macros.jinja, macros/rust.jinja"),
            "{message}"
        );
    }

    #[test]
    fn importing_from_a_template_with_no_partials_says_so() {
        let f = Fixture::new();
        let error = f
            .render_with(
                &[f.entry("README.md.jinja", b"{% import 'macros.jinja' as m %}")],
                &context(),
                no_partials(),
            )
            .unwrap_err();

        let message = format!("{:?}", miette::Report::new(error));
        assert!(message.contains("defines no partials"), "{message}");
    }

    #[test]
    fn a_partial_that_is_not_utf8_is_rejected_by_name() {
        let f = Fixture::new();
        let entries = [f.entry("macros.jinja", b"\xff\xfe not text")];
        let tree = f.repo.build_tree(&entries).unwrap();

        let error = collect_partials(&f.repo, tree, "template").unwrap_err();

        assert!(matches!(
            error,
            RenderError::PartialNotUtf8 { ref path } if path == "macros.jinja"
        ));
    }

    /// Invariant 2. A loader is a new source of inputs, so it gets its own
    /// determinism test rather than relying on the one above.
    #[test]
    fn rendering_with_the_same_partials_twice_produces_the_same_tree() {
        let f = Fixture::new();
        let partials = Arc::new(f.partials(
            "template",
            &[(
                "macros.jinja",
                b"{% macro title(name) %}# {{ name }}{% endmacro %}",
            )],
        ));
        let entries = [f.entry(
            "README.md.jinja",
            b"{% import 'macros.jinja' as m %}{{ m.title(project_name) }}\n",
        )];

        let first = render_tree(&f.repo, &f.repo, &entries, &context(), &partials).unwrap();
        let second = render_tree(&f.repo, &f.repo, &entries, &context(), &partials).unwrap();

        assert_eq!(first, second);
    }

    /// Changing a partial must change the tree, or `update` would produce no
    /// commit for a real change to the template.
    #[test]
    fn changing_a_partial_changes_the_rendered_tree() {
        let f = Fixture::new();
        let entries = [f.entry(
            "README.md.jinja",
            b"{% import 'macros.jinja' as m %}{{ m.title(project_name) }}\n",
        )];

        let before = Arc::new(f.partials(
            "template",
            &[(
                "macros.jinja",
                b"{% macro title(name) %}# {{ name }}{% endmacro %}",
            )],
        ));
        let after = Arc::new(f.partials(
            "template",
            &[(
                "macros.jinja",
                b"{% macro title(name) %}## {{ name }}{% endmacro %}",
            )],
        ));

        assert_ne!(
            render_tree(&f.repo, &f.repo, &entries, &context(), &before).unwrap(),
            render_tree(&f.repo, &f.repo, &entries, &context(), &after).unwrap(),
        );
    }

    /// `{% include %}` shares the loader with `{% import %}`, so it resolves
    /// against the same set and needs no separate machinery.
    #[test]
    fn include_resolves_against_the_same_partials_as_import() {
        let f = Fixture::new();
        let partials =
            Arc::new(f.partials("template", &[("header.jinja", b"# {{ project_name }}")]));

        let rendered = f
            .render_with(
                &[f.entry("README.md.jinja", b"{% include 'header.jinja' %}\n")],
                &context(),
                &partials,
            )
            .unwrap();

        assert_eq!(rendered[0].content, b"# Demo\n");
    }
}
