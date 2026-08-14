//! Rendering a template tree into a Git tree.
//!
//! The output is written straight into Git objects, never to the filesystem.
//! That is what makes "`update` does not touch your working tree" structural
//! rather than a promise: there is no code path here that opens a file for
//! writing in the project.
//!
//! Rendering is deterministic — see `docs/concepts/determinism.md` for the
//! full list of hazards and how each is handled.

use miette::Diagnostic;
use thiserror::Error;

use crate::context::Context;
use crate::eval::{EvalError, render_string};
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
    template: &impl GitBackend,
    project: &impl GitBackend,
    entries: &[TreeEntry],
    context: &Context,
) -> Result<Oid, RenderError> {
    let rendered = render_entries(template, entries, context)?;

    let mut tree_entries = Vec::with_capacity(rendered.len());
    for file in rendered {
        let oid = project.write_blob(&file.content)?;
        tree_entries.push(TreeEntry {
            path: file.path,
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
    template: &impl GitBackend,
    entries: &[TreeEntry],
    context: &Context,
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

        let Some(path) = render_path(&entry.path, context)? else {
            // A segment rendered empty, so this entry — and, for a directory,
            // everything beneath it — is skipped. That is how a template makes
            // a whole subtree conditional.
            continue;
        };

        let source = template.read_blob(entry.oid)?;
        let is_template = entry.path.ends_with(TEMPLATE_SUFFIX);

        let content = if is_template && !is_binary(&source) {
            let text = String::from_utf8_lossy(&source);
            render_string(&text, context, &entry.path)
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
fn render_path(path: &str, context: &Context) -> Result<Option<String>, RenderError> {
    // Strip the suffix before rendering, so a templated directory name ending
    // in `.jinja` is not mistaken for a template file.
    let stripped = path.strip_suffix(TEMPLATE_SUFFIX).unwrap_or(path);

    let mut segments = Vec::new();
    for segment in stripped.split('/') {
        let rendered =
            render_string(segment, context, path).map_err(|source| RenderError::Path {
                path: path.to_string(),
                source,
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
            render_entries(&self.repo, entries, context).unwrap()
        }

        fn tree(&self, entries: &[TreeEntry], context: &Context) -> Oid {
            render_tree(&self.repo, &self.repo, entries, context).unwrap()
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
    fn a_path_that_would_escape_the_tree_is_rejected() {
        let f = Fixture::new();
        let mut context = Context::new();
        context.set_answer("evil", Value::String("..".into()));

        let error =
            render_entries(&f.repo, &[f.entry("{{ evil }}/x", b"x")], &context).unwrap_err();

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

        let error =
            render_entries(&f.repo, &[f.entry("{{ evil }}/x", b"x")], &context).unwrap_err();

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

        let rendered = render_entries(&f.repo, &entries[..1], &context());
        assert_eq!(rendered.unwrap()[0].content, b"a\r\nb\r\n");

        let rendered = render_entries(&f.repo, &entries[1..], &context());
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
}
