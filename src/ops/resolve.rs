//! Template resolution: fetching a template repository and reading its
//! manifest at a chosen revision.

use std::path::{Path, PathBuf};

use miette::Diagnostic;
use thiserror::Error;

use crate::git::libgit2::LibGit2;
use crate::git::{GitBackend, GitError, Oid, TreeEntry};
use crate::provenance::WORKTREE_REF;
use crate::template::{MANIFEST_NAME, Manifest, ManifestError};

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

/// A template, resolved to a revision and ready to render.
pub struct Resolved {
    /// The repository holding it. Kept alive because trees are read from it.
    pub repo: LibGit2,
    /// The manifest.
    pub manifest: Manifest,
    /// The whole template tree, for reading data files.
    pub tree: Oid,
    /// The subtree that gets rendered.
    pub root_tree: Oid,
    /// The commit the revision resolved to.
    pub revision: Oid,
    /// The revision as configured — a branch, tag, SHA, or `<worktree>`.
    pub reference: String,
    /// Whether an uncommitted working tree was read.
    pub dirty: bool,
    /// Kept so the temporary clone outlives the resolution.
    _cache: Option<tempfile::TempDir>,
}

impl Resolved {
    /// The flattened entries of the rendered subtree, in Git tree order.
    pub fn entries(&self) -> Result<Vec<TreeEntry>, GitError> {
        self.repo.list_tree(self.root_tree)
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

/// Fetch and resolve a template.
pub fn resolve(request: Request<'_>) -> Result<Resolved, ResolveError> {
    let local = local_path(request.source);

    if request.dirty && local.is_none() {
        return Err(ResolveError::DirtyNeedsLocal {
            origin: request.source.to_string(),
        });
    }

    let (repo, cache) = match &local {
        // A local template is opened in place. Cloning it would cost a copy
        // and, worse, would hide uncommitted changes that `--dirty` needs.
        Some(path) => (LibGit2::open(path)?, None),
        None => {
            // A fresh temporary clone per run. Caching between runs would be
            // faster, but a stale cache silently rendering an old template is a
            // far worse failure than a slow fetch — and the alternative is a
            // cache-invalidation problem nobody wants in a tool whose entire
            // premise is reproducibility.
            let dir = tempfile::tempdir().map_err(|source| ResolveError::Cache {
                path: std::env::temp_dir(),
                source,
            })?;
            let repo = LibGit2::clone_bare(request.source, dir.path())?;
            (repo, Some(dir))
        }
    };

    let (revision, tree, reference, dirty) = if request.dirty {
        let path = local.as_ref().expect("checked above");
        // The HEAD it is based on, recorded so the rendering is at least
        // attributable even though it is not reproducible.
        let base = repo.head_commit()?.unwrap_or_else(|| {
            // A template repository with no commits at all. Rendering its
            // working tree is still meaningful; there is simply no base.
            Oid::from_bytes([0; 20])
        });
        let tree = repo.tree_from_workdir(path)?;
        (base, tree, WORKTREE_REF.to_string(), true)
    } else {
        let reference = match request.reference {
            Some(reference) => reference.to_string(),
            None => repo.default_branch()?,
        };
        let revision = repo.resolve_revision(&reference, request.source)?;
        let tree = repo.commit_tree(revision)?;
        (revision, tree, reference, false)
    };

    let manifest_bytes =
        repo.read_path(tree, MANIFEST_NAME)?
            .ok_or_else(|| ManifestError::Missing {
                origin: request.source.to_string(),
            })?;
    let manifest_text = String::from_utf8_lossy(&manifest_bytes);
    let manifest = Manifest::parse(&manifest_text, MANIFEST_NAME)?;

    let root = request.root.unwrap_or(&manifest.root).to_string();
    let root_tree = repo
        .subtree(tree, &root)?
        .ok_or(ResolveError::MissingRoot { root })?;

    Ok(Resolved {
        repo,
        manifest,
        tree,
        root_tree,
        revision,
        reference,
        dirty,
        _cache: cache,
    })
}

/// The path a source refers to, if it is one on this machine.
fn local_path(source: &str) -> Option<PathBuf> {
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
        let result = resolve(Request {
            source: "https://github.com/a/b",
            reference: None,
            root: None,
            dirty: true,
        });

        match result {
            Err(ResolveError::DirtyNeedsLocal { .. }) => {}
            Err(other) => panic!("expected DirtyNeedsLocal, got {other:?}"),
            Ok(_) => panic!("expected an error"),
        }
    }
}
