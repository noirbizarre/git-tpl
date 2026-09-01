//! Template resolution: fetching a template repository and reading its
//! manifest at a chosen revision.
//!
//! A template with `[extends]` resolves as a *chain*: this module fetches
//! each ancestor in turn, checks it is pinned and not a repeat of one already
//! seen, and folds the chain's manifests into one effective [`Manifest`]
//! (`ops::extends::merge_chain`). See `docs/adr/034-template-inheritance.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use miette::Diagnostic;
use thiserror::Error;

use super::Trust;
use super::extends::{self, ExtendsError};
use crate::data::{Decision, REMOTE_LIMIT_BYTES, RemoteRequest, SourceKind, TemplateTree};
use crate::eval::Partials;
use crate::git::libgit2::LibGit2;
use crate::git::{GitBackend, GitError, Oid, TreeEntry};
use crate::provenance::{ExtendsProvenance, WORKTREE_REF};
use crate::render::{LayeredEntry, RenderError, TEMPLATE_SUFFIX, collect_partials};
use crate::template::{DataSourceDecl, MANIFEST_NAME, Manifest, ManifestError};
use crate::userconfig::UserConfig;

/// Errors from resolving a template.
#[derive(Debug, Error, Diagnostic)]
pub enum ResolveError {
    /// The template could not be fetched or opened.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Git(#[from] GitError),

    /// The manifest is missing or invalid.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Manifest(#[from] ManifestError),

    /// An `[extends]` chain could not be resolved or merged.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Extends(#[from] ExtendsError),

    /// The manifest's `root` directory does not exist in the template.
    #[error("`{root}` does not exist in the template")]
    #[diagnostic(
        code(tpl::resolve::missing_root),
        help(
            "`root` in {MANIFEST_NAME} names the subdirectory that gets rendered. \
             It defaults to `template`."
        )
    )]
    MissingRoot {
        /// The configured root.
        root: String,
    },

    /// `--dirty` was asked for on a template that is not a local path.
    #[error("`--dirty` needs a local template")]
    #[diagnostic(
        code(tpl::resolve::dirty_needs_local),
        help("`{origin}` is remote, and there is no working tree to read")
    )]
    DirtyNeedsLocal {
        /// The configured source.
        // Not named `source`: thiserror reserves that name for `#[source]`.
        origin: String,
    },

    /// A cache directory could not be created.
    #[error("could not prepare the template cache at `{}`", path.display())]
    #[diagnostic(code(tpl::resolve::cache))]
    Cache {
        /// The path involved.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
}

/// One resolved template repository, at a pinned revision, before it is known
/// whether it is the leaf or an ancestor.
struct Layer {
    repo: Box<dyn GitBackend>,
    manifest: Manifest,
    tree: Oid,
    root_tree: Oid,
    root: String,
    revision: Oid,
    reference: String,
    dirty: bool,
    ignored: Vec<String>,
    _cache: Option<tempfile::TempDir>,
}

/// An ancestor above the leaf in an `[extends]` chain.
struct Ancestor {
    repo: Box<dyn GitBackend>,
    /// This ancestor's own `[extends].source`, as its child declared it.
    source: String,
    /// This ancestor's own, unmerged manifest.
    manifest: Manifest,
    tree: Oid,
    root_tree: Oid,
    root: String,
    revision: Oid,
    /// This ancestor's own `[extends].rev`, as its child declared it.
    reference: String,
    /// Repository-root-relative paths this ancestor's own child asked to
    /// remove from the merge (`[extends].remove`).
    removed: Vec<String>,
    _cache: Option<tempfile::TempDir>,
}

/// A template, resolved to a revision and ready to render.
pub struct Resolved {
    /// The repository holding it. Kept alive because trees are read from it.
    ///
    /// Boxed as a trait object: `resolve` is the one place that has to choose
    /// between opening and cloning, and naming the concrete backend beyond
    /// that choice is what let `LibGit2` spread through `ops`.
    pub repo: Box<dyn GitBackend>,
    /// The effective manifest: this template's own for a template with no
    /// `[extends]`; otherwise the whole chain folded into one, by
    /// `ops::extends::merge_chain`.
    pub manifest: Manifest,
    /// The whole template tree, for reading data files.
    pub tree: Oid,
    /// The subtree that gets rendered.
    pub root_tree: Oid,
    /// The render root, as resolved. Kept because it is what separates an
    /// output file from a partial, and the manifest's value may be overridden.
    pub root: String,
    /// The commit the revision resolved to.
    pub revision: Oid,
    /// The reference as configured — a branch, tag, SHA, or `<worktree>`.
    pub reference: String,
    /// Whether an uncommitted working tree was read.
    pub dirty: bool,
    /// Paths a `.gitignore` kept out of a `--dirty` render.
    ///
    /// Empty for a committed revision. Surfaced because the ignore stack
    /// includes `core.excludesFile`: a global rule set years ago on an
    /// unrelated project can remove a file the author can see on disk, and an
    /// unexplained absence in a rendering is the hardest kind of bug to find.
    ///
    /// Only paths a render actually reads — under `root`, a partial, or a
    /// declared data file. See `affects_render`.
    pub ignored: Vec<String>,
    /// Ancestors above this template, nearest parent first, root ancestor
    /// last. Empty for a template with no `[extends]`.
    ancestors: Vec<Ancestor>,
    /// `[data]` entry name -> index into `ancestors` that declared the entry
    /// currently in effect. Absent for an entry this template's own manifest
    /// declares or overrides.
    data_origin: BTreeMap<String, usize>,
    /// `[questions.<name>]` -> index into `ancestors` that declared the
    /// question currently in effect. Absent for one this template's own
    /// manifest declares or overrides. See [`Resolved::question_origin`].
    question_origin: BTreeMap<String, usize>,
    /// Kept so the temporary clone outlives the resolution.
    _cache: Option<tempfile::TempDir>,
}

impl Resolved {
    /// Whether this template extends anything.
    pub fn has_ancestors(&self) -> bool {
        !self.ancestors.is_empty()
    }

    /// This template's own rendered-subtree entries, ignoring any `[extends]`
    /// chain entirely.
    ///
    /// `git tpl lint` uses this rather than [`Resolved::entries`]: static
    /// analysis of an inherited file, in a repository lint has no reason to
    /// clone a second time just to check syntax it cannot act on, is left for
    /// a future ADR. Every other consumer of a template's files —
    /// rendering, provenance — needs the full merge and uses `entries`
    /// instead.
    pub fn own_entries(&self) -> Result<Vec<TreeEntry>, GitError> {
        self.repo.list_tree(self.root_tree)
    }

    /// The flattened, merged entries of the rendered subtree, in path order.
    ///
    /// For a template with no `[extends]`, exactly the entries of this
    /// template's own root subtree, each tagged origin `0` (this repository).
    /// Otherwise, each layer's own entries are collected root ancestor first,
    /// nearer layers overwriting a further layer's entry at the same
    /// *pre-render* path — the override ADR-034 describes. An ancestor's own
    /// entry removed by its child's `[extends].remove` never enters the merge
    /// at all.
    pub fn entries(&self) -> Result<Vec<LayeredEntry>, GitError> {
        let mut merged: BTreeMap<String, LayeredEntry> = BTreeMap::new();

        // Root ancestor first, so a nearer layer's `insert` below overwrites
        // it — `ancestors` is stored nearest-first, so this is `.rev()`.
        for (index, ancestor) in self.ancestors.iter().enumerate().rev() {
            let removed = removed_root_relative(&ancestor.root, &ancestor.removed);
            for entry in ancestor.repo.list_tree(ancestor.root_tree)? {
                if removed.contains(entry.path.as_str()) {
                    continue;
                }
                merged.insert(
                    entry.path.clone(),
                    LayeredEntry {
                        entry,
                        // `0` is this template's own repo; ancestor `i` is
                        // origin `i + 1` (`Resolved::read_blob`).
                        origin: index + 1,
                    },
                );
            }
        }
        for entry in self.repo.list_tree(self.root_tree)? {
            merged.insert(entry.path.clone(), LayeredEntry { entry, origin: 0 });
        }

        Ok(merged.into_values().collect())
    }

    /// Read a blob produced by [`Resolved::entries`], from whichever
    /// repository actually holds it.
    pub fn read_blob(&self, origin: usize, oid: Oid) -> Result<Vec<u8>, GitError> {
        match origin {
            0 => self.repo.read_blob(oid),
            n => self.ancestors[n - 1].repo.read_blob(oid),
        }
    }

    /// Every repository in the chain, this template's own first.
    pub fn repos(&self) -> Vec<&dyn GitBackend> {
        let mut repos: Vec<&dyn GitBackend> = vec![self.repo.as_ref()];
        repos.extend(self.ancestors.iter().map(|a| a.repo.as_ref()));
        repos
    }

    /// The templates an `{% import %}` or `{% include %}` may resolve to.
    ///
    /// Read from the whole template tree rather than the rendered subtree, so
    /// the set is exactly the `.jinja` files that are *not* output. Read from
    /// the tree — including the synthetic `--dirty` one — so a partial is
    /// pinned to the same revision as everything else it renders with.
    ///
    /// For a template with no `[extends]`, exactly this template's own
    /// partials. Otherwise, every layer's own partials, merged by name —
    /// nearest layer wins for a bare reference, `parent:name` reaches the
    /// next declaration out (ADR-034).
    pub fn partials(&self) -> Result<Arc<Partials>, RenderError> {
        let own = collect_partials(self.repo.as_ref(), self.tree, &self.root)?;
        if self.ancestors.is_empty() {
            return Ok(Arc::new(own));
        }

        let mut layers = vec![own];
        for ancestor in &self.ancestors {
            let collected =
                collect_partials(ancestor.repo.as_ref(), ancestor.tree, &ancestor.root)?
                    .without(&ancestor.removed);
            layers.push(collected);
        }
        Ok(Arc::new(Partials::merge_chain(layers)))
    }

    /// The template trees a `[data]` loader may read a `kind = "template"`
    /// source from, this template's own first, then each ancestor in order.
    /// Pairs with [`Resolved::data_origin`].
    pub fn template_trees(&self) -> Vec<TemplateTree<'_>> {
        let mut trees = vec![TemplateTree {
            repo: self.repo.as_ref(),
            tree: self.tree,
            revision: self.revision,
            reference: self.reference.clone(),
        }];
        trees.extend(self.ancestors.iter().map(|ancestor| TemplateTree {
            repo: ancestor.repo.as_ref(),
            tree: ancestor.tree,
            revision: ancestor.revision,
            reference: ancestor.reference.clone(),
        }));
        trees
    }

    /// `[data]` entry name -> index into [`Resolved::template_trees`]'s
    /// ancestors (`0` is the nearest parent), for every entry an ancestor
    /// currently contributes.
    pub fn data_origin(&self) -> &BTreeMap<String, usize> {
        &self.data_origin
    }

    /// `[questions.<name>]` -> index into [`Resolved::extends_provenance`]'s
    /// chain (`0` is the nearest parent), for every question an ancestor
    /// currently contributes. Absent for one this template's own manifest
    /// declares or overrides.
    ///
    /// For `git tpl context --json`, so a chain several layers deep can be
    /// debugged without cloning every ancestor by hand to find which one
    /// wrote a given question.
    pub fn question_origin(&self) -> &BTreeMap<String, usize> {
        &self.question_origin
    }

    /// The ancestor chain, for the `Template-Extends` provenance trailer —
    /// nearest parent first, root ancestor last. Empty for a template with no
    /// `[extends]`.
    pub fn extends_provenance(&self) -> Vec<ExtendsProvenance> {
        self.ancestors
            .iter()
            .map(|ancestor| ExtendsProvenance {
                source: ancestor.source.clone(),
                revision: ancestor.revision,
            })
            .collect()
    }
}

/// How to resolve a template.
pub struct Request<'a> {
    /// Any Git URL, or a path on this machine.
    pub source: &'a str,
    /// Branch, tag or commit. `None` means the remote's default branch.
    pub reference: Option<&'a str>,
    /// Override the manifest's rendered subdirectory.
    pub root: Option<&'a str>,
    /// Read the template's working tree rather than its `HEAD`.
    pub dirty: bool,
}

/// Fetch and resolve a template, and its `[extends]` chain if it has one.
///
/// `user` and `trust` exist for the `[extends]` chain alone: an ancestor's
/// source is chosen by the *template author*, exactly like a `[data]`
/// `kind = "git"` source, and is cloned exactly like one — so it is confirmed
/// exactly like one, before that clone happens. This is unrelated to, and
/// does not affect, a *separate* leaf-wide `[trust]` shortcut for `[data]`
/// sources applied later, in `render_resolved`: trusting the leaf template
/// does not imply trusting what it extends (ADR-034).
pub fn resolve(
    request: Request<'_>,
    user: &UserConfig,
    trust: &mut Trust<'_>,
) -> Result<Resolved, ResolveError> {
    let leaf = resolve_layer(
        request.source,
        request.reference,
        request.root,
        request.dirty,
    )?;

    // A template with no `[extends]` is a chain of one: its own manifest is
    // already the effective one, and there is nothing to fold.
    let (ancestors, manifest, data_origin, question_origin) = if leaf.manifest.extends.is_some() {
        resolve_ancestors(&leaf, request.source, user, trust)?
    } else {
        (
            Vec::new(),
            leaf.manifest.clone(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
    };

    Ok(Resolved {
        repo: leaf.repo,
        manifest,
        tree: leaf.tree,
        root_tree: leaf.root_tree,
        root: leaf.root,
        revision: leaf.revision,
        reference: leaf.reference,
        dirty: leaf.dirty,
        ignored: leaf.ignored,
        ancestors,
        question_origin,
        data_origin,
        _cache: leaf._cache,
    })
}

/// Resolve, open or clone one template repository to a pinned revision.
///
/// The shared body behind resolving the leaf (the template a caller actually
/// asked for) and each ancestor in an `[extends]` chain: only the source,
/// reference and root differ between the two, and `dirty` is never true for
/// an ancestor (only a local, directly-requested template can be `--dirty`).
fn resolve_layer(
    source: &str,
    reference: Option<&str>,
    root_override: Option<&str>,
    dirty: bool,
) -> Result<Layer, ResolveError> {
    let local = local_path(source);

    if dirty && local.is_none() {
        return Err(ResolveError::DirtyNeedsLocal {
            origin: source.to_string(),
        });
    }

    let (repo, cache) = match &local {
        // A local template is opened in place. Cloning it would cost a copy
        // and, worse, would hide uncommitted changes that `--dirty` needs.
        Some(path) => (Box::new(LibGit2::open(path)?) as Box<dyn GitBackend>, None),
        None => {
            // A fresh temporary clone per run. Caching between runs would be
            // faster, but a stale cache silently rendering an old template is a
            // far worse failure than a slow fetch — and the alternative is a
            // cache-invalidation problem nobody wants in a tool whose entire
            // premise is reproducibility. An `[extends]` chain multiplies this
            // cost by its depth; see docs/adr/034-template-inheritance.md.
            let dir = tempfile::tempdir().map_err(|source| ResolveError::Cache {
                path: std::env::temp_dir(),
                source,
            })?;
            let repo = LibGit2::clone_bare(source, dir.path())?;
            (Box::new(repo) as Box<dyn GitBackend>, Some(dir))
        }
    };

    let (revision, tree, reference, dirty, ignored) = if dirty {
        let path = local.as_ref().expect("checked above");
        // The HEAD it is based on, recorded so the rendering is at least
        // attributable even though it is not reproducible.
        let base = repo.head_commit()?.unwrap_or_else(|| {
            // A template repository with no commits at all. Rendering its
            // working tree is still meaningful; there is simply no base.
            Oid::from_bytes([0; 20])
        });
        let (tree, ignored) = repo.tree_from_workdir(path)?;
        (base, tree, WORKTREE_REF.to_string(), true, ignored)
    } else {
        let reference = match reference {
            Some(reference) => reference.to_string(),
            None => repo.default_branch()?,
        };
        let revision = repo.resolve_revision(&reference, source)?;
        let tree = repo.commit_tree(revision)?;
        (revision, tree, reference, false, Vec::new())
    };

    let manifest_bytes =
        repo.read_path(tree, MANIFEST_NAME)?
            .ok_or_else(|| ManifestError::Missing {
                origin: source.to_string(),
            })?;
    let manifest_text = String::from_utf8_lossy(&manifest_bytes);
    let manifest = Manifest::parse(&manifest_text, MANIFEST_NAME)?;

    let root = root_override.unwrap_or(&manifest.root).to_string();
    let root_tree = match repo.subtree(tree, &root)? {
        Some(tree) => tree,
        // A layer that declares `[extends]` is not required to be
        // self-sufficient: a child that only overrides a question or a data
        // source, contributing no files of its own, has nothing under `root`
        // at all -- Git does not track an empty directory, so there is no
        // tree entry to find. Read as the empty tree rather than an error,
        // since the merge (`Resolved::entries`) may still produce real output
        // from an ancestor. A template with no `[extends]` keeps the strict
        // reading: an empty root there is almost always a typo in `root`.
        None if manifest.extends.is_some() => repo.build_tree(&[])?,
        None => return Err(ResolveError::MissingRoot { root: root.clone() }),
    };

    // The walk that produced `ignored` covers the whole repository, and that is
    // deliberate: the *tree* is needed whole, for partials and for `lint`. Only
    // the report is narrowed. Warning about a path that could never have been
    // rendered — `.opencode/` beside `template.toml`, say, ignored by a rule in
    // the user's global `core.excludesFile` — is noise nothing in the template
    // can silence, printed above every rendering (#83).
    let mut ignored = ignored;
    ignored.retain(|path| affects_render(path, &root, &manifest.data));

    Ok(Layer {
        repo,
        manifest,
        tree,
        root_tree,
        root,
        revision,
        reference,
        dirty,
        ignored,
        _cache: cache,
    })
}

/// An ancestor chain, folded into one effective manifest — what
/// [`resolve_ancestors`] produces: the ancestors, the merged manifest, its
/// `[data]` origins, and its `[questions]` origins, in that order.
type AncestorChain = (
    Vec<Ancestor>,
    Manifest,
    BTreeMap<String, usize>,
    BTreeMap<String, usize>,
);

/// Walk an `[extends]` chain above `leaf`, and fold it into one manifest.
///
/// Depth and cycles are checked as each ancestor is fetched, by name, before
/// anything about it is trusted further — a chain that is too deep or that
/// revisits a template it has already resolved is rejected up front, exactly
/// as the issue that motivated ADR-034 asked for. A remote ancestor's source
/// is confirmed the same way, before it is ever cloned — see
/// [`confirm_ancestor`].
fn resolve_ancestors(
    leaf: &Layer,
    leaf_source: &str,
    user: &UserConfig,
    trust: &mut Trust<'_>,
) -> Result<AncestorChain, ResolveError> {
    let mut ancestors: Vec<Ancestor> = Vec::new();
    // `(source, revision)` rather than `source` alone: the same source at two
    // different revisions is not a cycle, only reading the exact same
    // revision twice is.
    let mut visited: Vec<(String, Oid)> = vec![(leaf_source.to_string(), leaf.revision)];
    let mut path: Vec<String> = vec![leaf_source.to_string()];

    let mut pending = leaf.manifest.extends.clone();

    while let Some(declared) = pending {
        if ancestors.len() >= extends::MAX_DEPTH {
            return Err(ExtendsError::Depth {
                limit: extends::MAX_DEPTH,
            }
            .into());
        }

        // Confirmed *before* the clone, one ancestor at a time: the chain is
        // only discoverable incrementally (this ancestor's own `[extends]`,
        // naming the next one, is not readable until it is cloned), so —
        // unlike `[data]`, fully known from the leaf's manifest alone — there
        // is no way to list the whole chain before any of it is reached.
        confirm_ancestor(&declared.source, ancestors.len(), user, trust)?;

        let layer = resolve_layer(&declared.source, Some(&declared.rev), None, false)?;

        if !extends::is_pinned(layer.repo.as_ref(), &declared.rev)? {
            return Err(ExtendsError::Unpinned {
                origin: declared.source.clone(),
                rev: declared.rev.clone(),
            }
            .into());
        }

        let key = (declared.source.clone(), layer.revision);
        path.push(declared.source.clone());
        if visited.contains(&key) {
            return Err(ExtendsError::Cycle { path }.into());
        }
        visited.push(key);

        for remove in &declared.remove {
            if layer.repo.read_path(layer.tree, remove)?.is_none() {
                return Err(ExtendsError::RemoveMissing {
                    origin: declared.source.clone(),
                    path: remove.clone(),
                }
                .into());
            }
        }

        pending = layer.manifest.extends.clone();
        ancestors.push(Ancestor {
            repo: layer.repo,
            source: declared.source,
            manifest: layer.manifest,
            tree: layer.tree,
            root_tree: layer.root_tree,
            root: layer.root,
            revision: layer.revision,
            reference: layer.reference,
            removed: declared.remove,
            _cache: layer._cache,
        });
    }

    let mut manifest_refs: Vec<&Manifest> = vec![&leaf.manifest];
    manifest_refs.extend(ancestors.iter().map(|a| &a.manifest));

    let extends::Merged {
        manifest,
        data_origin,
        question_origin,
    } = extends::merge_chain(&manifest_refs)?;

    Ok((ancestors, manifest, data_origin, question_origin))
}

/// Confirm one `[extends]` ancestor's source before it is cloned.
///
/// A local directory is exempt — matches every other "is this a network
/// operation" check in the codebase (`SourceKind::is_network`, the leaf's own
/// source): there is nothing to clone across a network, so nothing to
/// confirm. A `[trust]` entry covering this *ancestor's own* source is prior
/// consent and grants without asking, the same way it does for a `[data]`
/// source — but, deliberately, `[trust]` covering the *leaf* does not: each
/// ancestor is checked against its own source, independently (ADR-034).
fn confirm_ancestor(
    source: &str,
    depth: usize,
    user: &UserConfig,
    trust: &mut Trust<'_>,
) -> Result<(), ExtendsError> {
    if local_path(source).is_some() {
        return Ok(());
    }
    if user.trust.allows(source) {
        return Ok(());
    }

    // A stable, unique name per hop — mostly for the interactive prompt's own
    // display; nothing currently replays a decision across ancestors the way
    // `git tpl test` replays one across cases.
    let name = format!("extends:{depth}");
    let request = RemoteRequest {
        name: name.clone(),
        source: source.to_string(),
        kind: SourceKind::Git,
    };

    let decisions = trust
        .gate()
        .confirm(&[request], REMOTE_LIMIT_BYTES)
        .map_err(|_| ExtendsError::Cancelled)?;

    match decisions.get(&name) {
        Some(Decision::Allow) => Ok(()),
        _ => Err(ExtendsError::Untrusted {
            origin: source.to_string(),
        }),
    }
}

/// `remove` paths made root-relative, for filtering an ancestor's own
/// entries — `remove` is repository-relative (ADR-034), but
/// `GitBackend::list_tree` on a `root_tree` already returns paths relative to
/// `root`.
fn removed_root_relative<'a>(
    root: &str,
    removed: &'a [String],
) -> std::collections::BTreeSet<&'a str> {
    let root = root.trim_end_matches('/');
    removed
        .iter()
        .filter_map(|path| {
            if root.is_empty() || root == "." {
                Some(path.as_str())
            } else {
                path.strip_prefix(root)?.strip_prefix('/')
            }
        })
        .collect()
}

/// Whether an ignored path could have changed the rendering.
///
/// Three things a render reads, and nothing else: the tree under `root`, the
/// partials — every `TEMPLATE_SUFFIX` blob outside it (`collect_partials`) —
/// and the files named by the declared data sources. A path that is none of
/// these is absent from the rendering only in the vacuous sense that it was
/// never a candidate for it.
fn affects_render(path: &str, root: &str, data: &BTreeMap<String, DataSourceDecl>) -> bool {
    // An empty or `.` root renders the whole repository, exactly as `subtree`
    // reads it. Everything is then under the root and nothing is filtered.
    let root = root.trim_end_matches('/');
    if root.is_empty() || root == "." {
        return true;
    }

    if path == root || path.starts_with(&format!("{root}/")) {
        return true;
    }

    // A partial. Outside the root by definition, and its absence changes what
    // an `{% import %}` resolves to.
    if path.ends_with(TEMPLATE_SUFFIX) {
        return true;
    }

    data.values()
        .flat_map(|decl| [Some(decl.source.as_str()), decl.path.as_deref()])
        .flatten()
        .any(|location| declares(path, location))
}

/// Whether a data source declaration reads `path`, or something under it.
///
/// The second half matters because an ignored *directory* is recorded without
/// being descended into: `data/` ignored while `data/licenses.toml` is declared
/// has to warn, or the render fails with no clue where the file went.
fn declares(path: &str, location: &str) -> bool {
    // An expression is not resolvable here — it may depend on answers that do
    // not exist until the prompts run. Comparing the unrendered text would
    // match nothing anyway, so say no rather than guess.
    if location.contains("{{") || location.contains("{%") {
        return false;
    }
    let location = location.trim_start_matches("./");
    location == path || location.starts_with(&format!("{path}/"))
}

/// The path a source refers to, if it is one on this machine.
///
/// Public because `--write` in the test runner has to make the same call
/// `--dirty` does: a snapshot is written to a working tree, and a source with
/// no working tree must be refused before the first case renders. Two locality
/// rules would mean the two flags disagreeing about what "local" means.
pub fn local_path(source: &str) -> Option<PathBuf> {
    if source.contains("://") {
        // `file://` is a URL form of a local path, but treating it as remote
        // means it is cloned rather than opened — which is the safer reading of
        // an explicit URL, and keeps `--dirty` honest about needing a path.
        return None;
    }
    // `git@host:path` is scp-style, not a path containing a colon.
    if source.contains('@') && source.contains(':') {
        return None;
    }

    let path = Path::new(source);
    path.is_dir().then(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("https://github.com/a/b")]
    #[case("git@github.com:a/b.git")]
    #[case("ssh://git@github.com/a/b")]
    #[case("file:///tmp/x")]
    fn a_remote_source_is_not_a_local_path(#[case] source: &str) {
        assert_eq!(local_path(source), None);
    }

    #[test]
    fn an_existing_directory_is_a_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().to_string_lossy().to_string();
        assert_eq!(local_path(&source), Some(dir.path().to_path_buf()));
    }

    #[test]
    fn a_path_that_does_not_exist_is_not_a_local_path() {
        assert_eq!(local_path("/definitely/not/here/at/all"), None);
    }

    /// There is no working tree to read on a remote, so the error must say so
    /// rather than failing later with something about a missing directory.
    #[test]
    fn dirty_on_a_remote_template_is_refused_up_front() {
        // `Resolved` holds a live repository handle and is deliberately not
        // `Debug`, so the error is matched rather than unwrapped.
        let user = UserConfig::default();
        let mut trust = Trust::refuse();
        let result = resolve(
            Request {
                source: "https://github.com/a/b",
                reference: None,
                root: None,
                dirty: true,
            },
            &user,
            &mut trust,
        );

        match result {
            Err(ResolveError::DirtyNeedsLocal { .. }) => {}
            Err(other) => panic!("expected DirtyNeedsLocal, got {other:?}"),
            Ok(_) => panic!("expected an error"),
        }
    }

    /// A declaration reading `data/licenses.toml`, as a template would write it.
    fn declaring(source: &str) -> BTreeMap<String, DataSourceDecl> {
        let decl: DataSourceDecl = toml::from_str(&format!("source = \"{source}\"")).unwrap();
        BTreeMap::from([("licenses".to_string(), decl)])
    }

    #[rstest]
    // Under the render root: the case the warning exists for (#51).
    #[case("template/secret.local", true)]
    #[case("template", true)]
    // Beside it. Never a candidate for the rendering, so never reported (#83).
    #[case(".opencode/plans", false)]
    #[case("docs/usage", false)]
    // A near-miss on the prefix. `templates/` is not `template/`.
    #[case("templates/other", false)]
    // A partial, which lives outside the root by definition.
    #[case("macros.jinja", true)]
    // A declared data file, and the directory holding it — an ignored
    // directory is recorded without being descended into.
    #[case("data/licenses.toml", true)]
    #[case("data", true)]
    #[case("data/unused.toml", false)]
    fn only_a_path_a_render_reads_is_reported(#[case] path: &str, #[case] expected: bool) {
        assert_eq!(
            affects_render(path, "template", &declaring("data/licenses.toml")),
            expected
        );
    }

    /// An expression resolves against answers that do not exist yet, so it
    /// cannot be compared. Saying "no" beats matching on the unrendered text.
    #[test]
    fn an_expression_valued_data_source_matches_nothing() {
        let data = declaring("data/{{ flavour }}.toml");
        assert!(!affects_render("data/one.toml", "template", &data));
    }

    /// `subtree` reads an empty or `.` root as the whole repository, so there
    /// is nothing outside it to filter.
    #[rstest]
    #[case("")]
    #[case(".")]
    fn a_whole_repository_root_filters_nothing(#[case] root: &str) {
        assert!(affects_render(".opencode/plans", root, &BTreeMap::new()));
    }

    #[rstest]
    #[case("template", &["template/a.txt".to_string()], &["a.txt"])]
    #[case("", &["a.txt".to_string()], &["a.txt"])]
    #[case("template", &["other/a.txt".to_string()], &[])]
    fn remove_paths_are_made_root_relative(
        #[case] root: &str,
        #[case] removed: &[String],
        #[case] expected: &[&str],
    ) {
        let stripped = removed_root_relative(root, removed);
        let expected: std::collections::BTreeSet<&str> = expected.iter().copied().collect();
        assert_eq!(stripped, expected);
    }
}
