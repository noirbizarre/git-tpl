//! The Git abstraction.
//!
//! libgit2 is the only backend, but it is confined to [`libgit2`] so that the
//! domain, rendering and CLI layers never name a `git2` type. That confinement
//! is enforced by the `git-backend-isolation` prek hook, because a trait alone
//! enforces nothing — see `docs/adr/011-git-backend-isolation.md`.

// `.gitignore` evaluation is ours rather than libgit2's, because libgit2 will
// not let a negation override a lower-precedence ignore file — see ADR-017.
// Private: it answers a question only the working-tree walk asks.
mod ignore;
pub mod libgit2;

use std::fmt;
use std::path::{Path, PathBuf};

use miette::Diagnostic;
use thiserror::Error;

pub use libgit2::LibGit2;

/// A Git object id.
///
/// Ours, not `git2`'s, so that a signature in `ops` or `render` does not name a
/// `git2` type and quietly make the abstraction decorative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid([u8; 20]);

impl Oid {
    /// Build from raw bytes.
    pub fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// The full 40-character hex form.
    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }

    /// The abbreviated form used in output.
    pub fn short(self) -> String {
        self.to_hex()[..7].to_string()
    }

    /// Parse a full hex object id.
    pub fn parse(hex_str: &str) -> Option<Self> {
        let bytes = hex::decode(hex_str).ok()?;
        let array: [u8; 20] = bytes.try_into().ok()?;
        Some(Self(array))
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// A file's mode, as Git records it.
///
/// Git stores nothing else about a file's permissions, which is why the
/// executable bit is the only thing rendering preserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    /// A regular file, `100644`.
    Blob,
    /// An executable file, `100755`.
    BlobExecutable,
    /// A symbolic link, `120000`.
    Link,
    /// A directory, `040000`.
    Tree,
    /// A submodule, `160000`.
    Commit,
}

impl FileMode {
    /// The numeric mode Git writes.
    pub fn as_u32(self) -> u32 {
        match self {
            FileMode::Blob => 0o100644,
            FileMode::BlobExecutable => 0o100755,
            FileMode::Link => 0o120000,
            FileMode::Tree => 0o040000,
            FileMode::Commit => 0o160000,
        }
    }

    /// Interpret a numeric mode.
    pub fn from_u32(mode: u32) -> Option<Self> {
        match mode {
            0o100644 => Some(FileMode::Blob),
            0o100755 => Some(FileMode::BlobExecutable),
            0o120000 => Some(FileMode::Link),
            0o040000 => Some(FileMode::Tree),
            0o160000 => Some(FileMode::Commit),
            _ => None,
        }
    }

    /// Whether this mode holds file content.
    pub fn is_blob(self) -> bool {
        matches!(self, FileMode::Blob | FileMode::BlobExecutable)
    }
}

/// One entry in a flattened tree listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// The path relative to the tree root, with `/` separators.
    pub path: String,
    /// The object it points at.
    pub oid: Oid,
    /// Its mode.
    pub mode: FileMode,
}

/// How a path changed between two trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Present in the new tree only.
    Added,
    /// Present in both, with different content.
    Modified,
    /// Present in the old tree only.
    Deleted,
}

impl ChangeKind {
    /// The label used in command output, padded to a common width so that the
    /// paths beside them line up.
    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Added => "added   ",
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted ",
        }
    }

    /// The machine-readable name, unpadded.
    ///
    /// Separate from [`label`](Self::label) because that one is padded for
    /// column alignment, and a JSON consumer matching on `"added   "` would be
    /// depending on a presentation decision.
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted",
        }
    }
}

/// A single change between two trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// What happened.
    pub kind: ChangeKind,
    /// The path it happened to.
    pub path: String,
}

/// A change between two trees, with how much of it there was.
///
/// Separate from [`Change`] because the line counts cost a walk over every hunk
/// of every delta, and the `init`/`update` reports only ever need the path and
/// the kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    /// What happened.
    pub kind: ChangeKind,
    /// The path it happened to.
    pub path: String,
    /// Lines added. Zero for a binary file — the concept does not apply there,
    /// and inventing a number is worse than saying so.
    pub insertions: usize,
    /// Lines removed. Zero for a binary file, for the same reason.
    pub deletions: usize,
    /// Whether either side is binary, so the caller prints `Bin` rather than
    /// two zeroes that would read as "nothing changed".
    pub binary: bool,
}

/// How a local ref relates to its remote counterpart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AheadBehind {
    /// Commits the local ref has that the remote does not.
    pub ahead: usize,
    /// Commits the remote has that the local ref does not.
    pub behind: usize,
}

impl AheadBehind {
    /// Neither side has anything the other lacks.
    pub fn is_synced(self) -> bool {
        self.ahead == 0 && self.behind == 0
    }

    /// Both sides have commits the other lacks — rendered independently.
    pub fn is_diverged(self) -> bool {
        self.ahead > 0 && self.behind > 0
    }

    /// The relation in words, as `status` and `fetch` both report it.
    ///
    /// One phrasing, defined once. It was built three different ways —
    /// "diverged — 2 ahead, 1 behind", "has diverged: 2 ahead, 1 behind" and
    /// "local is 2 ahead and 1 behind" — for one fact, in three places a user
    /// may well see in the same session.
    pub fn describe(self) -> String {
        if self.is_synced() {
            "in sync".to_string()
        } else if self.is_diverged() {
            format!("diverged — {} ahead, {} behind", self.ahead, self.behind)
        } else if self.ahead > 0 {
            format!("{} ahead", self.ahead)
        } else {
            format!("{} behind", self.behind)
        }
    }
}

/// The outcome of a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Already contained; nothing to do.
    UpToDate,
    /// The branch pointer moved forward.
    FastForward {
        /// The commit the branch now points at.
        to: Oid,
    },
    /// A merge commit was created.
    Merged {
        /// The merge commit.
        commit: Oid,
    },
    /// Conflicts were left in the index for the user to resolve.
    Conflicted {
        /// The conflicting paths.
        paths: Vec<String>,
    },
    /// The merge was staged but not committed, as asked.
    Staged,
}

/// The tree a merge would produce, without performing it.
///
/// `git merge-tree --write-tree`: conflicted files are present in the tree with
/// conflict markers, because that is what a merge would leave in the worktree,
/// and a preview that hid them would understate the work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergePreview {
    /// The merged tree.
    pub tree: Oid,
    /// The paths that conflicted, sorted and deduplicated.
    pub conflicts: Vec<String>,
}

/// A commit, as the domain needs to see it.
#[derive(Debug, Clone)]
pub struct Commit {
    /// Its object id.
    pub oid: Oid,
    /// The tree it points at.
    pub tree: Oid,
    /// Its parents.
    pub parents: Vec<Oid>,
    /// The full commit message, trailers included.
    pub message: String,
}

/// Errors from Git operations.
#[derive(Debug, Error, Diagnostic)]
pub enum GitError {
    /// The path is not inside a Git repository.
    #[error("`{}` is not a Git repository", path.display())]
    #[diagnostic(
        code(tpl::git::not_a_repository),
        help("run `git init` first, or `git tpl init --init` to do both")
    )]
    NotARepository {
        /// The path that was searched from.
        path: PathBuf,
    },

    /// The path is a Jujutsu workspace, but not a colocated one.
    ///
    /// A non-colocated `jj git init` never creates a `.git` — the backing
    /// store at `.jj/repo/store/git` is bare and not discoverable the way
    /// Git itself discovers a repository. Reported separately from
    /// `NotARepository` because the remedy is different: `git init` here
    /// would create a second, unrelated repository that jj does not track,
    /// orphaning the workspace from its own history instead of fixing
    /// anything.
    #[error("`{}` is a Jujutsu workspace, but not a colocated one", path.display())]
    #[diagnostic(
        code(tpl::git::jj_not_colocated),
        help(
            "git-tpl needs a `.git` directory; colocate this workspace with \
             `jj git init --colocate`, or run from one already colocated"
        )
    )]
    JujutsuNotColocated {
        /// The path that was searched from.
        path: PathBuf,
    },

    /// A revision could not be resolved.
    #[error("could not resolve `{reference}` in `{origin}`")]
    #[diagnostic(
        code(tpl::git::no_such_revision),
        help("`ref` must be a branch, tag or commit that exists in the template repository")
    )]
    NoSuchRevision {
        /// The name that was asked for — a branch, tag or SHA.
        reference: String,
        /// The repository it was looked for in.
        // Not named `source`: thiserror reserves that name for `#[source]`.
        origin: String,
    },

    /// Authentication failed.
    ///
    /// Reported separately from other network failures because the remedy is
    /// entirely different, and libgit2's own message ("authentication
    /// required but no callback set") tells a user nothing actionable.
    #[error("authentication failed for `{url}`")]
    #[diagnostic(
        code(tpl::git::auth),
        help(
            "tried: {methods}.\n\
             For SSH, check that an agent is running and holds your key:\n  \
             ssh-add -l\n\
             For HTTPS, check that a credential helper is configured:\n  \
             git config --get credential.helper"
        )
    )]
    Authentication {
        /// The remote that refused.
        url: String,
        /// The methods that were attempted.
        methods: String,
    },

    /// A network operation failed for a reason other than authentication.
    ///
    /// `reason` is rendered, not merely carried: libgit2's message is the only
    /// thing separating a proxy, a typo, a DNS failure and a build missing its
    /// TLS backend, and without it the diagnostic tells the user nothing they
    /// did not already know.
    #[error("could not reach `{url}`")]
    #[diagnostic(
        code(tpl::git::network),
        help(
            "reason: {reason}\n\
             Check the URL, then your network and any proxy:\n  \
             git config --get http.proxy"
        )
    )]
    Network {
        /// The remote.
        url: String,
        /// libgit2's message.
        reason: String,
    },

    /// A clone failed for a local reason — the remote is not implicated.
    ///
    /// Split from `Network` because the two remedies share nothing. `clone`
    /// creates the repository and writes its objects, so a full `$TMPDIR`, a
    /// read-only destination or a refused `mkdir` all surface as an error from
    /// the same call the URL was passed to. Reporting those as "could not
    /// reach" sent a user off to check a proxy that was never involved, and
    /// looked intermittent because it tracked free disk space.
    #[error("could not clone `{url}`")]
    #[diagnostic(
        code(tpl::git::clone),
        help(
            "reason: {reason}\n\
             The remote answered; writing the clone failed. Check the free \
             space and the permissions on the destination:\n  \
             df -h {}",
            path.display()
        )
    )]
    Clone {
        /// The remote that was being cloned.
        url: String,
        /// The destination that could not be written.
        path: PathBuf,
        /// libgit2's message.
        reason: String,
    },

    /// A remote of that name already exists.
    ///
    /// Its own variant because [`GitBackend::remote_url`] reports a non-UTF-8
    /// URL as *absent*, so a caller that checked first can still arrive here.
    /// Distinguishing it lets `ops` leave the existing remote alone, which is
    /// what it would have done had it been able to read the URL.
    #[error("a remote named `{name}` already exists")]
    #[diagnostic(
        code(tpl::git::remote_exists),
        help("inspect it with `git remote -v`; git-tpl never repoints an existing remote")
    )]
    RemoteExists {
        /// The remote's name.
        name: String,
    },

    /// The worktree has uncommitted changes and the operation needs it clean.
    #[error("the working tree has uncommitted changes")]
    #[diagnostic(
        code(tpl::git::dirty_worktree),
        help("commit or stash them first — this operation performs a merge")
    )]
    DirtyWorktree,

    /// Pushing would overwrite commits the remote has and we do not.
    #[error("`{ref_name}` has diverged from the remote")]
    #[diagnostic(
        code(tpl::git::diverged),
        help(
            "local is {ahead} ahead and {behind} behind. Both were rendered independently; reconcile them:\n  \
             git tpl fetch\n  \
             git merge {remote_ref}\n  \
             git tpl push"
        )
    )]
    Diverged {
        /// The local ref.
        ref_name: String,
        /// The remote-tracking ref.
        remote_ref: String,
        /// Commits only we have.
        ahead: usize,
        /// Commits only the remote has.
        behind: usize,
    },

    /// A commit cannot be created without an identity.
    #[error("no Git identity configured")]
    #[diagnostic(
        code(tpl::git::no_identity),
        help(
            "git-tpl creates commits, which need an author:\n  \
             git config --global user.name  \"Your Name\"\n  \
             git config --global user.email \"you@example.com\""
        )
    )]
    NoIdentity,

    /// Anything else libgit2 reported.
    #[error("{context}")]
    #[diagnostic(code(tpl::git::backend))]
    Backend {
        /// What we were trying to do.
        context: String,
        /// libgit2's message.
        reason: String,
    },
}

/// The operations git-tpl needs from a Git implementation.
///
/// Every method takes and returns types defined in this module, never the
/// backend's own. That is what makes the backend replaceable, and it is
/// enforced by a hook rather than by hope.
///
/// The rule for membership is: **if anything above `src/git/` calls it, it
/// belongs here.** Nothing outside this module may reach for an inherent
/// method on a concrete backend, because a capability reachable only through
/// `LibGit2` is a capability the abstraction does not actually cover. Opening,
/// creating and cloning a repository are the exception, and stay inherent —
/// they produce a backend rather than use one.
pub trait GitBackend {
    /// The repository's working directory.
    fn workdir(&self) -> Result<PathBuf, GitError>;

    /// Whether the repository has no commits yet.
    fn is_empty(&self) -> Result<bool, GitError>;

    /// Whether the working tree and index are clean.
    fn is_clean(&self) -> Result<bool, GitError>;

    /// The commit `HEAD` points at, if any.
    fn head_commit(&self) -> Result<Option<Oid>, GitError>;

    /// The short name of the current branch, if any.
    fn head_branch(&self) -> Result<Option<String>, GitError>;

    /// Resolve a ref to the commit it points at.
    fn resolve_ref(&self, name: &str) -> Result<Option<Oid>, GitError>;

    /// Read a commit.
    fn commit(&self, oid: Oid) -> Result<Commit, GitError>;

    /// List a tree, recursively, in Git's canonical (sorted) order.
    ///
    /// Traversal order is Git's rather than the filesystem's, because
    /// `readdir` order varies by filesystem and rendering must be
    /// deterministic.
    fn list_tree(&self, tree: Oid) -> Result<Vec<TreeEntry>, GitError>;

    /// Read a blob's contents.
    fn read_blob(&self, oid: Oid) -> Result<Vec<u8>, GitError>;

    /// Write a blob and return its id.
    fn write_blob(&self, content: &[u8]) -> Result<Oid, GitError>;

    /// Build a tree from a flat list of path/blob/mode entries.
    fn build_tree(&self, entries: &[TreeEntry]) -> Result<Oid, GitError>;

    /// Create a commit without moving any ref.
    fn create_commit(&self, tree: Oid, parents: &[Oid], message: &str) -> Result<Oid, GitError>;

    /// Point a ref at a commit.
    fn set_ref(&self, name: &str, oid: Oid, reflog_message: &str) -> Result<(), GitError>;

    /// The differences between two trees, in path order.
    ///
    /// `paths` limits the diff to those pathspecs; an empty slice means the
    /// whole tree.
    fn diff_trees(
        &self,
        from: Option<Oid>,
        to: Oid,
        paths: &[String],
    ) -> Result<Vec<Change>, GitError>;

    /// The differences between two trees with their line counts, in path order.
    fn diff_stat(
        &self,
        from: Option<Oid>,
        to: Oid,
        paths: &[String],
    ) -> Result<Vec<FileStat>, GitError>;

    /// A textual patch between two trees, as `git diff` would render it.
    fn diff_patch(&self, from: Option<Oid>, to: Oid, paths: &[String]) -> Result<String, GitError>;

    /// Whether `ancestor` is reachable from `descendant`.
    fn is_ancestor(&self, ancestor: Oid, descendant: Oid) -> Result<bool, GitError>;

    /// How two commits relate.
    fn ahead_behind(&self, local: Oid, upstream: Oid) -> Result<AheadBehind, GitError>;

    /// Merge a commit into the current branch.
    ///
    /// Implemented with the backend's own merge. git-tpl contributes no
    /// conflict resolution of its own — see
    /// `docs/adr/002-no-custom-reconciliation.md`.
    ///
    /// `stage` names worktree-relative paths to add to the merge's index before
    /// its tree is written, so that they land *in* the merge commit. Only
    /// honoured on a real, conflict-free merge that this call commits: the
    /// other outcomes produce no commit to carry them. See ADR-021.
    fn merge(
        &self,
        commit: Oid,
        message: &str,
        commit_result: bool,
        stage: &[&Path],
    ) -> Result<MergeOutcome, GitError>;

    /// The tree merging `theirs` into `ours` would produce.
    ///
    /// The merge runs in memory: no ref, no index and no worktree file is
    /// touched. This is what `git tpl diff` compares against, because a plain
    /// `HEAD`-to-template tree diff reports every project-owned file as a
    /// deletion that a merge would never make.
    fn merge_preview(&self, ours: Oid, theirs: Oid) -> Result<MergePreview, GitError>;

    /// Fetch refs matching a refspec from a remote.
    fn fetch_refspec(&self, remote: &str, refspec: &str) -> Result<(), GitError>;

    /// Push refs matching a refspec to a remote.
    fn push_refspec(&self, remote: &str, refspec: &str) -> Result<(), GitError>;

    /// Add a remote. Neither fetches nor pushes.
    ///
    /// Admitted under ADR-019's closure rule: a Git operation, idempotent,
    /// touching no worktree file and spawning no process. Fetching or pushing
    /// here would make a template's declaration reach the network, which is a
    /// different decision and is not this one.
    ///
    /// Paired with [`Self::remote_url`], which `ops` asks first so that a
    /// declared `origin` the repository already has is told apart from a new
    /// one. Note that `remote_url` reports a non-UTF-8 URL as absent, so an
    /// add can still find a remote already there; see `ops::add_remotes`.
    fn add_remote(&self, name: &str, url: &str) -> Result<(), GitError>;

    /// Read a `tpl.*` configuration value.
    fn config_string(&self, key: &str) -> Result<Option<String>, GitError>;

    /// Read a boolean `tpl.*` configuration value.
    fn config_bool(&self, key: &str) -> Result<Option<bool>, GitError>;

    /// Every configuration entry, in Git's own precedence order.
    ///
    /// For prompt seeds only. Enumerated rather than read key by key so that a
    /// `default_from` expression asking for a key that is not set gets
    /// *undefined* — which is what `| default(...)` needs in order to fire.
    /// Reading key by key cannot distinguish "unset" from "a section with no
    /// such leaf", and the fallback would never trigger.
    fn config_entries(&self) -> Result<Vec<(String, String)>, GitError>;

    /// A remote's fetch URL, if that remote exists.
    ///
    /// `Ok(None)` rather than an error for an unknown remote: a project that
    /// has not been pushed yet is an ordinary state, and a template seeding a
    /// prompt from the remote must still render there.
    fn remote_url(&self, name: &str) -> Result<Option<String>, GitError>;

    /// Resolve a reference — branch, tag or SHA — to the commit it names.
    ///
    /// The parameter is a `reference`, not a `revision`: it is the name asked
    /// for, and the `Oid` returned is the revision. The function name says
    /// which direction that goes.
    ///
    /// `origin` names the repository only so that a failure can say where it
    /// looked; it takes no part in the resolution.
    fn resolve_revision(&self, reference: &str, origin: &str) -> Result<Oid, GitError>;

    /// The default branch, used when no `ref` is configured.
    fn default_branch(&self) -> Result<String, GitError>;

    /// The tree of a commit.
    fn commit_tree(&self, commit: Oid) -> Result<Oid, GitError>;

    /// Build a tree from the files in a directory, for `--dirty` renders.
    ///
    /// Reads a working tree rather than a commit, honouring `.gitignore`, so
    /// the result matches what `git add -A` would have staged. The rules are
    /// evaluated by [`ignore`], not by libgit2, which refuses to let a
    /// negation override `core.excludesFile` — see ADR-017.
    ///
    /// Returns the tree and the paths `.gitignore` removed from it. The second
    /// half is reported rather than discarded: the ignore stack includes
    /// `core.excludesFile`, so a global rule can silently drop a file the
    /// author can see, and a render has no `git status` to explain it with.
    fn tree_from_workdir(&self, root: &Path) -> Result<(Oid, Vec<String>), GitError>;

    /// Build a tree from a directory's contents, ignoring `.gitignore` entirely.
    ///
    /// For reading a `--dirty` snapshot back: `--write` writes a snapshot straight
    /// to disk, bypassing Git and its ignore rules on purpose (see
    /// `write_snapshot`), so a symmetric read must not let an ordinary rule
    /// matching a snapshot's own filename — a bare `MANIFEST`, say — make a file
    /// `--write` just produced disappear from what `--dirty` reads back (#116). A
    /// snapshot is recorded data, not a project file `git add -A` would ever
    /// decide about.
    fn tree_from_directory(&self, dir: &Path) -> Result<Oid, GitError>;

    /// Read a file from a tree by path.
    fn read_path(&self, tree: Oid, path: &str) -> Result<Option<Vec<u8>>, GitError>;

    /// The subtree at `path`, if there is one.
    fn subtree(&self, tree: Oid, path: &str) -> Result<Option<Oid>, GitError>;

    /// Stage a path, for `init` writing the configuration file.
    fn stage(&self, relative: &Path) -> Result<(), GitError>;

    /// Commit whatever is staged, moving `HEAD`.
    ///
    /// The one method here that does move `HEAD`. It exists for the
    /// configuration file `init` writes, which is a change to the project and
    /// not to the rendered ref; invariant 1 is about `update`.
    fn commit_index(&self, message: &str) -> Result<Oid, GitError>;

    /// Reset the index and worktree to `HEAD`, discarding a failed merge.
    fn abort_merge(&self) -> Result<(), GitError>;

    /// Set a configuration value in this repository.
    ///
    /// Exists so that tests and `init` can configure a repository without
    /// reaching for `git2` outside `src/git/libgit2.rs` — the
    /// `git-backend-isolation` hook forbids that, and an exception "just for
    /// tests" is how such boundaries rot.
    fn set_config_str(&self, key: &str, value: &str) -> Result<(), GitError>;

    /// Set a boolean configuration value in this repository.
    fn set_config_bool(&self, key: &str, value: bool) -> Result<(), GitError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_oid_round_trips_through_hex() {
        let hex_str = "4f2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29";
        let oid = Oid::parse(hex_str).unwrap();
        assert_eq!(oid.to_hex(), hex_str);
        assert_eq!(oid.short(), "4f2c1a9");
    }

    #[test]
    fn a_malformed_oid_is_rejected_rather_than_truncated() {
        assert_eq!(Oid::parse("abc"), None);
        assert_eq!(Oid::parse("zz2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29"), None);
    }

    /// Git records only the executable bit, which is why it is the only
    /// permission rendering preserves.
    #[test]
    fn every_file_mode_survives_a_conversion_to_git_and_back() {
        for mode in [
            FileMode::Blob,
            FileMode::BlobExecutable,
            FileMode::Link,
            FileMode::Tree,
            FileMode::Commit,
        ] {
            assert_eq!(FileMode::from_u32(mode.as_u32()), Some(mode));
        }
        assert_eq!(FileMode::from_u32(0o100777), None);
    }

    #[test]
    fn change_labels_are_the_same_width_so_paths_align() {
        let widths: Vec<_> = [ChangeKind::Added, ChangeKind::Modified, ChangeKind::Deleted]
            .iter()
            .map(|k| k.label().len())
            .collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}");
    }

    #[test]
    fn divergence_needs_commits_on_both_sides() {
        assert!(
            AheadBehind {
                ahead: 2,
                behind: 1
            }
            .is_diverged()
        );
        assert!(
            !AheadBehind {
                ahead: 2,
                behind: 0
            }
            .is_diverged()
        );
        assert!(
            !AheadBehind {
                ahead: 0,
                behind: 1
            }
            .is_diverged()
        );
        assert!(
            AheadBehind {
                ahead: 0,
                behind: 0
            }
            .is_synced()
        );
    }
}
