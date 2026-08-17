//! The libgit2 backend.
//!
//! **This is the only file permitted to name `git2`.** The
//! `git-backend-isolation` prek hook fails the commit if `git2::` appears
//! anywhere else under `src/`. See `docs/adr/011-git-backend-isolation.md`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use git2::build::TreeUpdateBuilder;
use git2::{
    AutotagOption, Cred, CredentialType, Delta, DiffFormat, DiffOptions, ErrorClass, ErrorCode,
    FetchOptions, ObjectType, PushOptions, RemoteCallbacks, Repository, RepositoryOpenFlags,
    ResetType,
};

use super::ignore::IgnoreStack;
use super::{
    AheadBehind, Change, ChangeKind, Commit, FileMode, FileStat, GitBackend, GitError,
    MergeOutcome, MergePreview, Oid, TreeEntry,
};

/// A repository opened through libgit2.
pub struct LibGit2 {
    repo: Repository,
}

impl LibGit2 {
    /// The diff between two trees, limited to `paths`.
    ///
    /// The three diff methods below differ only in how they read the result;
    /// building it in one place is what keeps their pathspec handling and their
    /// tree resolution from drifting apart.
    fn tree_diff(
        &self,
        from: Option<Oid>,
        to: Oid,
        paths: &[String],
    ) -> Result<git2::Diff<'_>, GitError> {
        let old = from
            .map(|oid| self.repo.find_tree(from_oid(oid)))
            .transpose()
            .map_err(|e| backend("read the old tree", &e))?;
        let new = self
            .repo
            .find_tree(from_oid(to))
            .map_err(|e| backend("read the new tree", &e))?;

        let mut options = DiffOptions::new();
        for path in paths {
            options.pathspec(path);
        }

        self.repo
            .diff_tree_to_tree(old.as_ref(), Some(&new), Some(&mut options))
            .map_err(|e| backend("diff the trees", &e))
    }

    /// Open the repository containing `path`, searching upwards.
    pub fn discover(path: &Path) -> Result<Self, GitError> {
        // `open_ext` with no ceiling directories searches upwards, which is
        // what every Git command does — running `git tpl status` from a
        // subdirectory must work.
        let repo = Repository::open_ext(
            path,
            RepositoryOpenFlags::empty(),
            std::iter::empty::<&std::ffi::OsStr>(),
        )
        .map_err(|_| GitError::NotARepository {
            path: path.to_path_buf(),
        })?;
        Ok(Self { repo })
    }

    /// Create a repository at `path` and open it.
    pub fn init(path: &Path) -> Result<Self, GitError> {
        let repo = Repository::init(path).map_err(|e| backend("initialise the repository", &e))?;
        Ok(Self { repo })
    }

    /// Open a repository at an exact path, without searching upwards.
    ///
    /// Used for template repositories, where searching upwards could silently
    /// pick up an unrelated enclosing repository.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let repo = Repository::open(path).map_err(|_| GitError::NotARepository {
            path: path.to_path_buf(),
        })?;
        Ok(Self { repo })
    }

    /// Clone a template repository into `into`, as a bare mirror.
    ///
    /// The callbacks are here to attribute a failure, not to report progress.
    /// A clone writes a repository as well as reading a remote, and libgit2
    /// gives both halves `ErrorClass::Os` — "Disk quota exceeded" and
    /// "Connection refused" are indistinguishable by class or code. So the
    /// stage is recorded as it is passed: `remote_create` runs once the
    /// repository exists and before anything connects, and `transfer_progress`
    /// runs once objects are arriving. Where the clone stopped says which half
    /// failed, and no message has to be guessed at.
    pub fn clone_bare(url: &str, into: &Path) -> Result<Self, GitError> {
        let mut builder = git2::build::RepoBuilder::new();
        builder.bare(true);

        let stage = Rc::new(Cell::new(Stage::Creating));

        let creating = Rc::clone(&stage);
        builder.remote_create(move |repo, name, url| {
            creating.set(Stage::Connecting);
            repo.remote(name, url)
        });

        let mut callbacks = credential_callbacks(url);
        let receiving = Rc::clone(&stage);
        callbacks.transfer_progress(move |_| {
            receiving.set(Stage::Receiving);
            true
        });

        let mut fetch = FetchOptions::new();
        fetch.remote_callbacks(callbacks);
        // Tags matter: `ref = "v1.4.0"` is a common way to pin a template, and
        // the default fetch would not bring them.
        fetch.download_tags(AutotagOption::All);
        builder.fetch_options(fetch);

        let repo = builder.clone(url, into).map_err(|e| match stage.get() {
            // Never reached `remote_create`, so nothing was sent anywhere: the
            // repository itself could not be written. This is the case that
            // used to be reported as `tpl::git::network`, telling a user with a
            // full $TMPDIR to go and check their proxy.
            Stage::Creating => GitError::Clone {
                url: url.to_string(),
                path: into.to_path_buf(),
                reason: e.message().to_string(),
            },
            // Past the creation. The two remaining stages differ only in
            // whether there is a destination to blame: while connecting there
            // is nothing written yet, so the wire is the only candidate; once
            // objects are arriving the remote has demonstrably answered, and a
            // failure that is not a transport error is the local write.
            stage => {
                let destination = matches!(stage, Stage::Receiving).then_some(into);
                translate_remote(url, destination, &e)
            }
        })?;
        Ok(Self { repo })
    }

    /// The signature to author commits with.
    fn signature(&self) -> Result<git2::Signature<'_>, GitError> {
        self.repo.signature().map_err(|_| GitError::NoIdentity)
    }
}

impl GitBackend for LibGit2 {
    fn workdir(&self) -> Result<PathBuf, GitError> {
        self.repo
            .workdir()
            .map(Path::to_path_buf)
            .ok_or_else(|| GitError::NotARepository {
                path: self.repo.path().to_path_buf(),
            })
    }

    fn is_empty(&self) -> Result<bool, GitError> {
        Ok(self.repo.head().is_err())
    }

    fn is_clean(&self) -> Result<bool, GitError> {
        let mut options = git2::StatusOptions::new();
        options.include_untracked(false).include_ignored(false);
        let statuses = self
            .repo
            .statuses(Some(&mut options))
            .map_err(|e| backend("read the repository status", &e))?;
        Ok(statuses.is_empty())
    }

    fn head_commit(&self) -> Result<Option<Oid>, GitError> {
        match self.repo.head() {
            Ok(head) => {
                let commit = head
                    .peel_to_commit()
                    .map_err(|e| backend("resolve HEAD", &e))?;
                Ok(Some(to_oid(commit.id())))
            }
            // An unborn branch is a normal state, not an error: `git tpl init`
            // in a fresh `git init` must work.
            Err(e) if e.code() == ErrorCode::UnbornBranch || e.code() == ErrorCode::NotFound => {
                Ok(None)
            }
            Err(e) => Err(backend("resolve HEAD", &e)),
        }
    }

    fn head_branch(&self) -> Result<Option<String>, GitError> {
        match self.repo.head() {
            // git2 0.21 reports a non-UTF-8 branch name as an error; a name we
            // cannot spell is as good as absent to every caller here.
            Ok(head) => Ok(head.shorthand().ok().map(str::to_string)),
            Err(e) if e.code() == ErrorCode::UnbornBranch => {
                // An unborn HEAD still names the branch it will create.
                Ok(self
                    .repo
                    .find_reference("HEAD")
                    .ok()
                    // `Result<Option<&str>>`: `Err` is non-UTF-8, `Ok(None)` is
                    // a direct rather than a symbolic reference.
                    .and_then(|r| r.symbolic_target().ok().flatten().map(str::to_string))
                    .and_then(|t| t.strip_prefix("refs/heads/").map(str::to_string)))
            }
            Err(_) => Ok(None),
        }
    }

    fn resolve_ref(&self, name: &str) -> Result<Option<Oid>, GitError> {
        match self.repo.find_reference(name) {
            Ok(reference) => {
                let commit = reference
                    .peel_to_commit()
                    .map_err(|e| backend("resolve the ref", &e))?;
                Ok(Some(to_oid(commit.id())))
            }
            Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(backend("look up the ref", &e)),
        }
    }

    fn commit(&self, oid: Oid) -> Result<Commit, GitError> {
        let commit = self
            .repo
            .find_commit(from_oid(oid))
            .map_err(|e| backend("read the commit", &e))?;
        Ok(Commit {
            oid,
            tree: to_oid(commit.tree_id()),
            parents: commit.parent_ids().map(to_oid).collect(),
            message: commit.message().unwrap_or_default().to_string(),
        })
    }

    fn list_tree(&self, tree: Oid) -> Result<Vec<TreeEntry>, GitError> {
        let tree = self
            .repo
            .find_tree(from_oid(tree))
            .map_err(|e| backend("read the tree", &e))?;

        let mut entries = Vec::new();
        tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            // `Ok` since git2 0.21. An entry whose name is not UTF-8 is skipped
            // exactly as before: we cannot render a path we cannot spell, and
            // silently dropping it keeps the walk deterministic.
            if let (Ok(name), Some(mode)) =
                (entry.name(), FileMode::from_u32(entry.filemode() as u32))
                && (mode.is_blob() || mode == FileMode::Link)
            {
                entries.push(TreeEntry {
                    path: format!("{dir}{name}"),
                    oid: to_oid(entry.id()),
                    mode,
                });
            }
            git2::TreeWalkResult::Ok
        })
        .map_err(|e| backend("walk the tree", &e))?;

        // Git's tree order sorts directories as if they ended in `/`, so a
        // pre-order walk does not yield paths in plain lexicographic order.
        // Rendering compares and reports these, so normalise it here rather
        // than leaving every caller to remember.
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    fn read_blob(&self, oid: Oid) -> Result<Vec<u8>, GitError> {
        let blob = self
            .repo
            .find_blob(from_oid(oid))
            .map_err(|e| backend("read the blob", &e))?;
        Ok(blob.content().to_vec())
    }

    fn write_blob(&self, content: &[u8]) -> Result<Oid, GitError> {
        self.repo
            .blob(content)
            .map(to_oid)
            .map_err(|e| backend("write the blob", &e))
    }

    fn build_tree(&self, entries: &[TreeEntry]) -> Result<Oid, GitError> {
        // An empty tree still has to exist as an object, or a template that
        // renders nothing would fail rather than producing an empty commit.
        let empty = self
            .repo
            .treebuilder(None)
            .and_then(|b| b.write())
            .map_err(|e| backend("create the empty tree", &e))?;

        if entries.is_empty() {
            return Ok(to_oid(empty));
        }

        let mut builder = TreeUpdateBuilder::new();
        for entry in entries {
            builder.upsert(&entry.path, from_oid(entry.oid), to_filemode(entry.mode));
        }

        let base = self
            .repo
            .find_tree(empty)
            .map_err(|e| backend("read the empty tree", &e))?;
        builder
            .create_updated(&self.repo, &base)
            .map(to_oid)
            .map_err(|e| backend("build the tree", &e))
    }

    fn create_commit(&self, tree: Oid, parents: &[Oid], message: &str) -> Result<Oid, GitError> {
        let signature = self.signature()?;
        let tree = self
            .repo
            .find_tree(from_oid(tree))
            .map_err(|e| backend("read the tree", &e))?;

        let parent_commits: Vec<_> = parents
            .iter()
            .map(|p| self.repo.find_commit(from_oid(*p)))
            .collect::<Result<_, _>>()
            .map_err(|e| backend("read a parent commit", &e))?;
        let parent_refs: Vec<_> = parent_commits.iter().collect();

        // `None` for the ref: creating the commit and moving the ref are
        // separate steps, so that `update` can decide not to move anything if
        // the tree turned out identical.
        self.repo
            .commit(None, &signature, &signature, message, &tree, &parent_refs)
            .map(to_oid)
            .map_err(|e| backend("create the commit", &e))
    }

    fn set_ref(&self, name: &str, oid: Oid, reflog_message: &str) -> Result<(), GitError> {
        self.repo
            .reference(name, from_oid(oid), true, reflog_message)
            .map_err(|e| backend("update the ref", &e))?;
        Ok(())
    }

    fn diff_trees(
        &self,
        from: Option<Oid>,
        to: Oid,
        paths: &[String],
    ) -> Result<Vec<Change>, GitError> {
        let diff = self.tree_diff(from, to, paths)?;

        let mut changes = Vec::new();
        for delta in diff.deltas() {
            changes.push(Change {
                kind: change_kind(delta.status()),
                path: delta_path(&delta),
            });
        }

        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    fn diff_stat(
        &self,
        from: Option<Oid>,
        to: Oid,
        paths: &[String],
    ) -> Result<Vec<FileStat>, GitError> {
        let diff = self.tree_diff(from, to, paths)?;

        // The counts are accumulated by path, because the line callback is
        // handed a delta rather than its index and the two must be joined
        // somehow. Order is imposed by the sort at the end, never by the map.
        let mut order: HashMap<String, usize> = HashMap::new();
        let mut stats: Vec<FileStat> = Vec::new();
        for delta in diff.deltas() {
            let path = delta_path(&delta);
            order.insert(path.clone(), stats.len());
            stats.push(FileStat {
                kind: change_kind(delta.status()),
                path,
                insertions: 0,
                deletions: 0,
                binary: false,
            });
        }

        let counted = RefCell::new(stats);
        diff.foreach(
            &mut |_, _| true,
            None,
            None,
            Some(&mut |delta, _hunk, line| {
                // `>` and `<` mark a missing trailing newline. `git diff --stat`
                // does not count them, and counting them would report "a
                // newline was added" as a changed line.
                let counter = match line.origin() {
                    '+' => 0,
                    '-' => 1,
                    _ => return true,
                };
                if let Some(&i) = order.get(&delta_path(&delta)) {
                    let mut stats = counted.borrow_mut();
                    if counter == 0 {
                        stats[i].insertions += 1;
                    } else {
                        stats[i].deletions += 1;
                    }
                }
                true
            }),
        )
        .map_err(|e| backend("summarise the diff", &e))?;

        let mut stats = counted.into_inner();

        // Read after the walk, not before: libgit2 only sets the binary flag on
        // a delta once it has loaded the file's contents, which `foreach` is
        // what causes. Asked earlier, every file claims to be text.
        for delta in diff.deltas() {
            if delta.flags().is_binary()
                && let Some(&i) = order.get(&delta_path(&delta))
            {
                stats[i].binary = true;
                // A binary delta produces no lines, so any count here would be
                // a leftover, not a measurement.
                stats[i].insertions = 0;
                stats[i].deletions = 0;
            }
        }

        stats.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(stats)
    }

    fn diff_patch(&self, from: Option<Oid>, to: Oid, paths: &[String]) -> Result<String, GitError> {
        let diff = self.tree_diff(from, to, paths)?;

        let mut out = String::new();
        diff.print(DiffFormat::Patch, |_, _, line| {
            match line.origin() {
                '+' | '-' | ' ' => out.push(line.origin()),
                _ => {}
            }
            out.push_str(&String::from_utf8_lossy(line.content()));
            true
        })
        .map_err(|e| backend("format the diff", &e))?;

        Ok(out)
    }

    fn is_ancestor(&self, ancestor: Oid, descendant: Oid) -> Result<bool, GitError> {
        self.repo
            .graph_descendant_of(from_oid(descendant), from_oid(ancestor))
            .or_else(|_| Ok::<bool, GitError>(ancestor == descendant))
            .map(|is_descendant| is_descendant || ancestor == descendant)
    }

    fn ahead_behind(&self, local: Oid, upstream: Oid) -> Result<AheadBehind, GitError> {
        let (ahead, behind) = self
            .repo
            .graph_ahead_behind(from_oid(local), from_oid(upstream))
            .map_err(|e| backend("compare the refs", &e))?;
        Ok(AheadBehind { ahead, behind })
    }

    fn merge(
        &self,
        commit: Oid,
        message: &str,
        commit_result: bool,
    ) -> Result<MergeOutcome, GitError> {
        let their_commit = self
            .repo
            .find_commit(from_oid(commit))
            .map_err(|e| backend("read the commit to merge", &e))?;
        let annotated = self
            .repo
            .find_annotated_commit(from_oid(commit))
            .map_err(|e| backend("prepare the merge", &e))?;

        let head = self.head_commit()?;

        // No commits yet: the template commit simply becomes the branch. This
        // is `git tpl init` in a freshly `git init`ed directory, and produces
        // the cleanest possible history for a generated project.
        let Some(head_oid) = head else {
            let branch = self.head_branch()?.unwrap_or_else(|| "main".to_string());
            self.set_ref(
                &format!("refs/heads/{branch}"),
                commit,
                "tpl: initial template state",
            )?;
            self.repo
                .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
                .map_err(|e| backend("check out the template", &e))?;
            return Ok(MergeOutcome::FastForward { to: commit });
        };

        let (analysis, _) = self
            .repo
            .merge_analysis(&[&annotated])
            .map_err(|e| backend("analyse the merge", &e))?;

        if analysis.is_up_to_date() {
            return Ok(MergeOutcome::UpToDate);
        }

        if analysis.is_fast_forward() {
            let branch = self.head_branch()?.ok_or_else(|| GitError::Backend {
                context: "fast-forward the branch".into(),
                reason: "HEAD is detached".into(),
            })?;
            self.set_ref(
                &format!("refs/heads/{branch}"),
                commit,
                "tpl: fast-forward to the template state",
            )?;
            self.repo
                .set_head(&format!("refs/heads/{branch}"))
                .map_err(|e| backend("move HEAD", &e))?;
            self.repo
                .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
                .map_err(|e| backend("check out the merged tree", &e))?;
            return Ok(MergeOutcome::FastForward { to: commit });
        }

        // A real merge. libgit2's `merge` writes the result into the index and
        // the worktree, leaving conflicts exactly as Git would — which is the
        // whole point: git-tpl contributes no conflict resolution.
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.allow_conflicts(true).conflict_style_merge(true);

        self.repo
            .merge(&[&annotated], None, Some(&mut checkout))
            .map_err(|e| backend("merge", &e))?;

        let mut index = self
            .repo
            .index()
            .map_err(|e| backend("read the index", &e))?;

        if index.has_conflicts() {
            let mut paths: Vec<String> = index
                .conflicts()
                .map_err(|e| backend("read the conflicts", &e))?
                .filter_map(|c| c.ok())
                .filter_map(|c| {
                    c.our
                        .or(c.their)
                        .or(c.ancestor)
                        .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
                })
                .collect();
            paths.sort();
            paths.dedup();
            // The merge state is left in place deliberately, so that
            // `git status`, `git mergetool` and `git merge --abort` all work.
            return Ok(MergeOutcome::Conflicted { paths });
        }

        if !commit_result {
            return Ok(MergeOutcome::Staged);
        }

        let tree_oid = index
            .write_tree_to(&self.repo)
            .map_err(|e| backend("write the merged tree", &e))?;
        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|e| backend("read the merged tree", &e))?;
        let head_commit = self
            .repo
            .find_commit(from_oid(head_oid))
            .map_err(|e| backend("read HEAD", &e))?;
        let signature = self.signature()?;

        let merge_commit = self
            .repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[&head_commit, &their_commit],
            )
            .map_err(|e| backend("create the merge commit", &e))?;

        self.repo
            .cleanup_state()
            .map_err(|e| backend("clean up the merge state", &e))?;

        Ok(MergeOutcome::Merged {
            commit: to_oid(merge_commit),
        })
    }

    fn merge_preview(&self, ours: Oid, theirs: Oid) -> Result<MergePreview, GitError> {
        let our_commit = self
            .repo
            .find_commit(from_oid(ours))
            .map_err(|e| backend("read the commit to merge into", &e))?;
        let their_commit = self
            .repo
            .find_commit(from_oid(theirs))
            .map_err(|e| backend("read the commit to merge", &e))?;

        // `merge_commits` produces an index of its own rather than the
        // repository's. Nothing on disk moves, which is what lets `git tpl diff`
        // preview a merge without touching HEAD, the index or the worktree.
        let mut index = self
            .repo
            .merge_commits(&our_commit, &their_commit, None)
            .map_err(|e| backend("merge in memory", &e))?;

        let mut conflicts: Vec<String> = Vec::new();

        if index.has_conflicts() {
            // Collected before resolving: `conflicts()` borrows the index, and
            // the loop below has to mutate it.
            let pending: Vec<git2::IndexConflict> = index
                .conflicts()
                .map_err(|e| backend("read the conflicts", &e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| backend("read the conflicts", &e))?;

            for conflict in pending {
                let entry = conflict
                    .our
                    .as_ref()
                    .or(conflict.their.as_ref())
                    .or(conflict.ancestor.as_ref())
                    .ok_or_else(|| GitError::Backend {
                        context: "resolve a conflict for the preview".into(),
                        reason: "the conflict names no file on any side".into(),
                    })?;
                let path = String::from_utf8_lossy(&entry.path).into_owned();
                conflicts.push(path.clone());

                // Conflict markers rather than a resolution: this mirrors
                // `git merge-tree --write-tree`, and a preview that quietly
                // picked a side would understate the work a merge leaves.
                let (mode, content) = match (&conflict.ancestor, &conflict.our, &conflict.their) {
                    (Some(ancestor), Some(our), Some(their)) => {
                        let merged = self
                            .repo
                            .merge_file_from_index(ancestor, our, their, None)
                            .map_err(|e| backend("merge a conflicting file", &e))?;
                        (merged.mode(), merged.content().to_vec())
                    }
                    (None, Some(our), Some(their)) => {
                        // Both sides added the file, so there is no ancestor to
                        // diff against. An empty one gives the same markers Git
                        // writes for an add/add conflict.
                        let empty = self
                            .repo
                            .blob(&[])
                            .map_err(|e| backend("write the empty ancestor blob", &e))?;
                        let mut ancestor = copy_entry(our);
                        ancestor.id = empty;
                        let merged = self
                            .repo
                            .merge_file_from_index(&ancestor, our, their, None)
                            .map_err(|e| backend("merge a conflicting file", &e))?;
                        (merged.mode(), merged.content().to_vec())
                    }
                    _ => {
                        // One side deleted what the other changed. Git keeps the
                        // surviving content and leaves the decision to the user;
                        // so does the preview.
                        let blob = self
                            .repo
                            .find_blob(entry.id)
                            .map_err(|e| backend("read a conflicting file", &e))?;
                        (entry.mode, blob.content().to_vec())
                    }
                };

                index
                    .conflict_remove(Path::new(&path))
                    .map_err(|e| backend("clear a conflict", &e))?;
                // The blob is written to the repository first, and the entry
                // added by id: the index `merge_commits` hands back is
                // free-standing, so it cannot write content itself.
                let blob = self
                    .repo
                    .blob(&content)
                    .map_err(|e| backend("write the conflicted file", &e))?;
                index
                    .add(&git2::IndexEntry {
                        ctime: git2::IndexTime::new(0, 0),
                        mtime: git2::IndexTime::new(0, 0),
                        dev: 0,
                        ino: 0,
                        // libgit2 leaves the mode at zero when it cannot agree
                        // on one; a regular file is the only sane fallback for
                        // content it just produced.
                        mode: if mode == 0 { 0o100644 } else { mode },
                        uid: 0,
                        gid: 0,
                        file_size: content.len() as u32,
                        id: blob,
                        // Stage zero: an entry still at a conflict stage cannot
                        // be written into a tree.
                        flags: 0,
                        flags_extended: 0,
                        path: path.clone().into_bytes(),
                    })
                    .map_err(|e| backend("record the conflicted file", &e))?;
            }

            conflicts.sort();
            conflicts.dedup();
        }

        let tree = index
            .write_tree_to(&self.repo)
            .map_err(|e| backend("write the merged tree", &e))?;

        Ok(MergePreview {
            tree: to_oid(tree),
            conflicts,
        })
    }

    fn fetch_refspec(&self, remote: &str, refspec: &str) -> Result<(), GitError> {
        let mut remote_handle = self
            .repo
            .find_remote(remote)
            .map_err(|_| GitError::Backend {
                context: format!("find the remote `{remote}`"),
                reason: format!("no remote named `{remote}` is configured"),
            })?;
        let url = remote_handle.url().unwrap_or(remote).to_string();

        let mut options = FetchOptions::new();
        options.remote_callbacks(credential_callbacks(&url));

        remote_handle
            .fetch(&[refspec], Some(&mut options), None)
            .map_err(|e| translate_remote(&url, None, &e))
    }

    fn push_refspec(&self, remote: &str, refspec: &str) -> Result<(), GitError> {
        let mut remote_handle = self
            .repo
            .find_remote(remote)
            .map_err(|_| GitError::Backend {
                context: format!("find the remote `{remote}`"),
                reason: format!("no remote named `{remote}` is configured"),
            })?;
        let url = remote_handle.url().unwrap_or(remote).to_string();

        let mut options = PushOptions::new();
        options.remote_callbacks(credential_callbacks(&url));

        remote_handle
            .push(&[refspec], Some(&mut options))
            .map_err(|e| translate_remote(&url, None, &e))
    }

    fn add_remote(&self, name: &str, url: &str) -> Result<(), GitError> {
        self.repo.remote(name, url).map(|_| ()).map_err(|e| {
            // Reported as its own kind rather than as a backend failure: the
            // caller asks `remote_url` first, but that reports a non-UTF-8 URL
            // as absent, so an existing remote can still surface here and is
            // not an error the user did anything to cause.
            if e.code() == ErrorCode::Exists {
                return GitError::RemoteExists {
                    name: name.to_string(),
                };
            }
            backend(&format!("add the remote `{name}`"), &e)
        })
    }

    fn config_string(&self, key: &str) -> Result<Option<String>, GitError> {
        // A snapshot reads through Git's own precedence — repository, then
        // user, then system — so `git config tpl.remote` and git-tpl always
        // agree about what is in effect.
        let mut config = self
            .repo
            .config()
            .map_err(|e| backend("read the Git configuration", &e))?;
        let snapshot = config
            .snapshot()
            .map_err(|e| backend("snapshot the Git configuration", &e))?;
        match snapshot.get_str(key) {
            Ok(value) => Ok(Some(value.to_string())),
            Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(backend("read a configuration value", &e)),
        }
    }

    fn config_bool(&self, key: &str) -> Result<Option<bool>, GitError> {
        let mut config = self
            .repo
            .config()
            .map_err(|e| backend("read the Git configuration", &e))?;
        let snapshot = config
            .snapshot()
            .map_err(|e| backend("snapshot the Git configuration", &e))?;
        match snapshot.get_bool(key) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(backend("read a configuration value", &e)),
        }
    }

    fn config_entries(&self) -> Result<Vec<(String, String)>, GitError> {
        let mut config = self
            .repo
            .config()
            .map_err(|e| backend("read the Git configuration", &e))?;
        // The snapshot must outlive the iterator, which borrows it.
        let snapshot = config
            .snapshot()
            .map_err(|e| backend("snapshot the Git configuration", &e))?;
        let entries = snapshot
            .entries(None)
            .map_err(|e| backend("list the Git configuration", &e))?;

        let mut collected = Vec::new();
        entries
            .for_each(|entry| {
                // Non-UTF-8 keys and values are skipped rather than lossily
                // converted: a mangled seed is worse than an absent one,
                // because the user would have to notice and correct it.
                if let (Ok(name), Ok(value)) = (entry.name(), entry.value()) {
                    collected.push((name.to_string(), value.to_string()));
                }
            })
            .map_err(|e| backend("read a configuration entry", &e))?;
        Ok(collected)
    }

    fn remote_url(&self, name: &str) -> Result<Option<String>, GitError> {
        match self.repo.find_remote(name) {
            // A non-UTF-8 URL is skipped for the same reason a non-UTF-8
            // configuration value is: it cannot be shown at a prompt.
            Ok(remote) => Ok(remote.url().ok().map(str::to_string)),
            Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(backend("read a remote", &e)),
        }
    }

    fn resolve_revision(&self, revision: &str, origin: &str) -> Result<Oid, GitError> {
        // `revparse_single` handles SHAs, tags and local branches. A bare
        // clone's branches live under `refs/remotes/origin/`, so a plain
        // `main` needs the fallback below.
        let candidates = [
            revision.to_string(),
            format!("refs/tags/{revision}"),
            format!("refs/remotes/origin/{revision}"),
            format!("refs/heads/{revision}"),
        ];

        for candidate in &candidates {
            if let Ok(object) = self.repo.revparse_single(candidate)
                && let Ok(commit) = object.peel_to_commit()
            {
                return Ok(to_oid(commit.id()));
            }
        }

        Err(GitError::NoSuchRevision {
            reference: revision.to_string(),
            origin: origin.to_string(),
        })
    }

    fn default_branch(&self) -> Result<String, GitError> {
        // `HEAD` in a bare clone points at whatever the remote said its default
        // branch was, which is the correct answer and needs no guessing between
        // `main` and `master`.
        if let Ok(head) = self.repo.head()
            // `Result` since git2 0.21: a non-UTF-8 name is an error rather
            // than a silent `None`. Either way we fall through to the guesses.
            && let Ok(name) = head.shorthand()
        {
            return Ok(name.to_string());
        }
        for candidate in ["main", "master"] {
            if self.resolve_revision(candidate, "").is_ok() {
                return Ok(candidate.to_string());
            }
        }
        Err(GitError::NoSuchRevision {
            reference: "HEAD".into(),
            origin: "the template repository".into(),
        })
    }

    fn commit_tree(&self, commit: Oid) -> Result<Oid, GitError> {
        let commit = self
            .repo
            .find_commit(from_oid(commit))
            .map_err(|e| backend("read the commit", &e))?;
        Ok(to_oid(commit.tree_id()))
    }

    fn tree_from_workdir(&self, root: &Path) -> Result<(Oid, Vec<String>), GitError> {
        // The rules in force above `root`. `root` is the template source, which
        // may sit below the working tree, and the outer rules still apply.
        //
        // `core.excludesFile` comes from libgit2's own config chain rather than
        // from the environment, so a repository-local override means here what
        // it means to Git — and the ignore stack agrees with the value every
        // other part of the tool would read.
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| GitError::Backend {
                context: "read the working tree".into(),
                reason: "the repository is bare".into(),
            })?
            .to_path_buf();
        let excludes_file = self
            .repo
            .config()
            .ok()
            .and_then(|config| config.get_path("core.excludesFile").ok());
        let stack = IgnoreStack::new(&workdir, self.repo.path(), excludes_file.as_deref(), root);

        let mut entries = Vec::new();
        let mut ignored = Vec::new();
        collect_workdir(root, root, &stack, &mut entries, &mut ignored)?;
        ignored.sort();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut blobs = Vec::new();
        for (path, absolute, executable) in entries {
            let content = std::fs::read(&absolute).map_err(|e| GitError::Backend {
                context: format!("read `{}`", absolute.display()),
                reason: e.to_string(),
            })?;
            let oid = self.write_blob(&content)?;
            blobs.push(TreeEntry {
                path,
                oid,
                mode: if executable {
                    FileMode::BlobExecutable
                } else {
                    FileMode::Blob
                },
            });
        }

        Ok((self.build_tree(&blobs)?, ignored))
    }

    fn read_path(&self, tree: Oid, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let tree = self
            .repo
            .find_tree(from_oid(tree))
            .map_err(|e| backend("read the tree", &e))?;
        match tree.get_path(Path::new(path)) {
            Ok(entry) => {
                let blob = self
                    .repo
                    .find_blob(entry.id())
                    .map_err(|e| backend("read the file", &e))?;
                Ok(Some(blob.content().to_vec()))
            }
            Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(backend("look up the path", &e)),
        }
    }

    fn subtree(&self, tree: Oid, path: &str) -> Result<Option<Oid>, GitError> {
        if path.is_empty() || path == "." {
            return Ok(Some(tree));
        }
        let tree = self
            .repo
            .find_tree(from_oid(tree))
            .map_err(|e| backend("read the tree", &e))?;
        match tree.get_path(Path::new(path)) {
            Ok(entry) if entry.kind() == Some(ObjectType::Tree) => Ok(Some(to_oid(entry.id()))),
            Ok(_) => Ok(None),
            Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(backend("look up the subtree", &e)),
        }
    }

    fn stage(&self, relative: &Path) -> Result<(), GitError> {
        let mut index = self
            .repo
            .index()
            .map_err(|e| backend("read the index", &e))?;
        index
            .add_path(relative)
            .map_err(|e| backend("stage the file", &e))?;
        index.write().map_err(|e| backend("write the index", &e))?;
        Ok(())
    }

    fn commit_index(&self, message: &str) -> Result<Oid, GitError> {
        let signature = self.signature()?;
        let mut index = self
            .repo
            .index()
            .map_err(|e| backend("read the index", &e))?;
        let tree_oid = index
            .write_tree()
            .map_err(|e| backend("write the tree", &e))?;
        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|e| backend("read the tree", &e))?;

        let parents: Vec<git2::Commit<'_>> = match self.head_commit()? {
            Some(oid) => vec![
                self.repo
                    .find_commit(from_oid(oid))
                    .map_err(|e| backend("read HEAD", &e))?,
            ],
            None => Vec::new(),
        };
        let parent_refs: Vec<_> = parents.iter().collect();

        self.repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parent_refs,
            )
            .map(to_oid)
            .map_err(|e| backend("create the commit", &e))
    }

    fn abort_merge(&self) -> Result<(), GitError> {
        let head = self
            .repo
            .head()
            .and_then(|h| h.peel(ObjectType::Commit))
            .map_err(|e| backend("resolve HEAD", &e))?;
        self.repo
            .reset(&head, ResetType::Hard, None)
            .map_err(|e| backend("reset the worktree", &e))?;
        self.repo
            .cleanup_state()
            .map_err(|e| backend("clean up the merge state", &e))?;
        Ok(())
    }

    fn set_config_str(&self, key: &str, value: &str) -> Result<(), GitError> {
        let mut config = self
            .repo
            .config()
            .map_err(|e| backend("read the Git configuration", &e))?;
        config
            .set_str(key, value)
            .map_err(|e| backend("write a configuration value", &e))
    }

    fn set_config_bool(&self, key: &str, value: bool) -> Result<(), GitError> {
        let mut config = self
            .repo
            .config()
            .map_err(|e| backend("read the Git configuration", &e))?;
        config
            .set_bool(key, value)
            .map_err(|e| backend("write a configuration value", &e))
    }
}

// --- helpers ---------------------------------------------------------------

fn to_oid(oid: git2::Oid) -> Oid {
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(oid.as_bytes());
    Oid::from_bytes(bytes)
}

fn from_oid(oid: Oid) -> git2::Oid {
    git2::Oid::from_bytes(oid.as_bytes()).expect("a 20-byte array is always a valid oid")
}

fn to_filemode(mode: FileMode) -> git2::FileMode {
    match mode {
        FileMode::Blob => git2::FileMode::Blob,
        FileMode::BlobExecutable => git2::FileMode::BlobExecutable,
        FileMode::Link => git2::FileMode::Link,
        FileMode::Tree => git2::FileMode::Tree,
        FileMode::Commit => git2::FileMode::Commit,
    }
}

/// How a delta reads in our own vocabulary.
///
/// A copy is an addition: the path is new, and where its content came from is
/// not something the reader of a change list can act on.
/// A field-by-field copy of an index entry.
///
/// `git2::IndexEntry` is not `Clone`, and the merge preview needs a modifiable
/// copy of one side to stand in for a missing ancestor.
fn copy_entry(entry: &git2::IndexEntry) -> git2::IndexEntry {
    git2::IndexEntry {
        ctime: entry.ctime,
        mtime: entry.mtime,
        dev: entry.dev,
        ino: entry.ino,
        mode: entry.mode,
        uid: entry.uid,
        gid: entry.gid,
        file_size: entry.file_size,
        id: entry.id,
        flags: entry.flags,
        flags_extended: entry.flags_extended,
        path: entry.path.clone(),
    }
}

fn change_kind(status: Delta) -> ChangeKind {
    match status {
        Delta::Added | Delta::Copied => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        _ => ChangeKind::Modified,
    }
}

/// The path a delta is about, with `/` separators on every platform.
///
/// The new side first, so a rename is reported at its destination; the old side
/// is all a deletion has.
fn delta_path(delta: &git2::DiffDelta<'_>) -> String {
    delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn backend(context: &str, error: &git2::Error) -> GitError {
    GitError::Backend {
        context: format!("could not {context}"),
        reason: error.message().to_string(),
    }
}

/// How far a clone got before it failed.
///
/// Not progress reporting — attribution. See `LibGit2::clone_bare`.
#[derive(Clone, Copy)]
enum Stage {
    /// Writing the repository. Nothing has been sent anywhere yet.
    Creating,
    /// The repository exists; connecting to and negotiating with the remote.
    Connecting,
    /// Objects are arriving, so the remote answered.
    Receiving,
}

/// Turn a failure from a remote operation into something a user can act on.
///
/// libgit2's own message for a failed SSH handshake is "authentication required
/// but no callback set", which is both wrong and useless, so authentication is
/// separated first.
///
/// `destination` is the path being written, and is `Some` only when the caller
/// knows the wire already worked — see `Stage` in `clone_bare`. Given that, an
/// error whose class is not a transport one is the local write, and saying
/// "could not reach" would be a lie. It is deliberately *not* enough to look at
/// the class alone: libgit2 reports both a refused connection and a full disk
/// as `ErrorClass::Os`, which is how a full `$TMPDIR` came to be reported as an
/// unreachable remote in the first place.
fn translate_remote(url: &str, destination: Option<&Path>, error: &git2::Error) -> GitError {
    if is_auth(error) {
        return GitError::Authentication {
            url: url.to_string(),
            methods: describe_attempted_methods(url),
        };
    }

    // The classes that can only come from a transport. `Callback` belongs here
    // because our only callbacks are the credential ones.
    let on_the_wire = matches!(
        error.class(),
        ErrorClass::Net
            | ErrorClass::Http
            | ErrorClass::Ssh
            | ErrorClass::Ssl
            | ErrorClass::Callback
    );

    match destination {
        Some(path) if !on_the_wire => GitError::Clone {
            url: url.to_string(),
            path: path.to_path_buf(),
            reason: error.message().to_string(),
        },
        _ => GitError::Network {
            url: url.to_string(),
            reason: error.message().to_string(),
        },
    }
}

/// Whether a failure was the remote refusing us rather than anything else.
///
/// The message sniff is load-bearing, not belt-and-braces: when
/// `credential_callbacks` exhausts its options it returns a `from_str` error,
/// which carries `ErrorCode::Generic` and would otherwise be misread.
fn is_auth(error: &git2::Error) -> bool {
    let message = error.message().to_lowercase();
    matches!(error.code(), ErrorCode::Auth)
        || message.contains("authentication")
        || message.contains("credentials")
        || message.contains("permission denied")
        || message.contains("access denied")
        || message.contains("401")
        || message.contains("403")
}

fn describe_attempted_methods(url: &str) -> String {
    if url.starts_with("http") {
        "a credential helper, then the URL's own credentials".to_string()
    } else {
        "the SSH agent, then the default key paths (~/.ssh/id_ed25519, ~/.ssh/id_rsa)".to_string()
    }
}

/// Credentials for fetch and push.
///
/// Tried in order of least to most surprising: the agent first, because a key
/// held there needs no passphrase prompt; then default key paths; then a
/// credential helper for HTTPS. libgit2 calls this repeatedly, once per method
/// it is willing to try, so state is kept to avoid offering the same failing
/// method forever.
fn credential_callbacks(url: &str) -> RemoteCallbacks<'static> {
    let mut callbacks = RemoteCallbacks::new();
    let url = url.to_string();
    let mut tried_agent = false;
    let mut tried_default = false;

    callbacks.credentials(move |_url, username, allowed| {
        let username = username.unwrap_or("git");

        if allowed.contains(CredentialType::SSH_KEY) {
            if !tried_agent {
                tried_agent = true;
                // Honours SSH_AUTH_SOCK, which is what makes a
                // passphrase-protected key work without a prompt.
                if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                    return Ok(cred);
                }
            }
            if !tried_default {
                tried_default = true;
                if let Some(home) = std::env::var_os("HOME") {
                    let home = PathBuf::from(home);
                    for name in ["id_ed25519", "id_rsa", "id_ecdsa"] {
                        let key = home.join(".ssh").join(name);
                        if key.exists()
                            && let Ok(cred) = Cred::ssh_key(username, None, &key, None)
                        {
                            return Ok(cred);
                        }
                    }
                }
            }
        }

        if allowed.contains(CredentialType::USERNAME) {
            return Cred::username(username);
        }

        if allowed.contains(CredentialType::DEFAULT) {
            return Cred::default();
        }

        // The credential helper. Last because it may prompt.
        if allowed.contains(CredentialType::USER_PASS_PLAINTEXT)
            && let Ok(config) = git2::Config::open_default()
            && let Ok(cred) = Cred::credential_helper(&config, &url, Some(username))
        {
            return Ok(cred);
        }

        Err(git2::Error::from_str(
            "no usable credentials: tried the SSH agent, the default key paths and the credential helper",
        ))
    });

    callbacks
}

/// Walk a working directory, collecting files Git would track.
///
/// `stack` holds the ignore rules in force *above* `dir`; this call adds
/// `dir`'s own `.gitignore` before looking at anything inside it.
fn collect_workdir(
    root: &Path,
    dir: &Path,
    stack: &IgnoreStack,
    out: &mut Vec<(String, PathBuf, bool)>,
    ignored: &mut Vec<String>,
) -> Result<(), GitError> {
    let stack = stack.entering(dir);

    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| GitError::Backend {
            context: format!("read `{}`", dir.display()),
            reason: e.to_string(),
        })?
        .filter_map(Result::ok)
        .collect();
    // `read_dir` order varies by filesystem; sorting keeps `--dirty` renders
    // deterministic for the same working tree.
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();

        if name == ".git" {
            continue;
        }

        // Stat before asking. A rule written `build/` applies to directories
        // only, and answering without knowing which this is would either keep
        // an ignored directory or drop a file that merely shares its name.
        let file_type = entry.file_type().map_err(|e| GitError::Backend {
            context: format!("stat `{}`", path.display()),
            reason: e.to_string(),
        })?;

        let relative = path.strip_prefix(root).unwrap_or(&path);
        // Honour .gitignore, so a `--dirty` render matches what `git add -A`
        // would have staged rather than including build output.
        //
        // Recorded, not just skipped. The stack spans per-directory
        // `.gitignore`, `.git/info/exclude` *and* `core.excludesFile`, so a
        // global rule set years ago on an unrelated project can silently
        // remove a file the author can see on disk. In a render there is no
        // `git status` to consult, and an unexplained absence is the hardest
        // kind of bug to find.
        //
        // An ignored directory is recorded and left, not walked. That is Git's
        // rule rather than an optimisation: a file cannot be re-included once
        // one of its parent directories is excluded, so descending to look for
        // a negation inside would resurrect files `git add -A` leaves alone.
        if stack.is_ignored(&path, file_type.is_dir()) {
            ignored.push(relative.to_string_lossy().replace('\\', "/"));
            continue;
        }

        if file_type.is_dir() {
            collect_workdir(root, &path, &stack, out, ignored)?;
        } else if file_type.is_file() {
            let relative_str = relative.to_string_lossy().replace('\\', "/");
            out.push((relative_str, path.clone(), is_executable(&path)));
        }
    }

    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    // Windows has no executable bit. Reporting `false` matches what Git records
    // for a file created there, so a tree built on Windows and one built on
    // Linux from the same source agree.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> (tempfile::TempDir, LibGit2) {
        let dir = tempfile::tempdir().unwrap();
        let repo = LibGit2::init(dir.path()).unwrap();
        // libgit2 refuses to build a signature without an identity, and a
        // developer machine may have none.
        repo.set_config_str("user.name", "Test").unwrap();
        repo.set_config_str("user.email", "test@example.invalid")
            .unwrap();
        (dir, repo)
    }

    #[test]
    fn a_fresh_repository_is_empty_and_has_no_head() {
        let (_dir, repo) = scratch();
        assert!(repo.is_empty().unwrap());
        assert_eq!(repo.head_commit().unwrap(), None);
    }

    #[test]
    fn a_tree_can_be_built_and_read_back() {
        let (_dir, repo) = scratch();

        let blob = repo.write_blob(b"hello\n").unwrap();
        let nested = repo.write_blob(b"nested\n").unwrap();
        let tree = repo
            .build_tree(&[
                TreeEntry {
                    path: "README.md".into(),
                    oid: blob,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "src/lib.rs".into(),
                    oid: nested,
                    mode: FileMode::Blob,
                },
            ])
            .unwrap();

        let entries = repo.list_tree(tree).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "README.md");
        assert_eq!(entries[1].path, "src/lib.rs");
        assert_eq!(repo.read_blob(entries[0].oid).unwrap(), b"hello\n");
    }

    /// Rendering must produce the same tree for the same inputs, so listing
    /// must not depend on Git's directory-suffix sort order.
    #[test]
    fn tree_listings_come_back_in_plain_path_order() {
        let (_dir, repo) = scratch();
        let blob = repo.write_blob(b"x").unwrap();

        let entries: Vec<_> = ["src/a.rs", "src-gen/b.rs", "README.md", "src/z/c.rs"]
            .iter()
            .map(|p| TreeEntry {
                path: (*p).to_string(),
                oid: blob,
                mode: FileMode::Blob,
            })
            .collect();

        let tree = repo.build_tree(&entries).unwrap();
        let listed: Vec<_> = repo
            .list_tree(tree)
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();

        let mut expected: Vec<_> = entries.iter().map(|e| e.path.clone()).collect();
        expected.sort();
        assert_eq!(listed, expected);
    }

    #[test]
    fn an_empty_tree_is_buildable() {
        let (_dir, repo) = scratch();
        let tree = repo.build_tree(&[]).unwrap();
        assert!(repo.list_tree(tree).unwrap().is_empty());
    }

    #[test]
    fn the_executable_bit_survives_a_tree_round_trip() {
        let (_dir, repo) = scratch();
        let blob = repo.write_blob(b"#!/bin/sh\n").unwrap();
        let tree = repo
            .build_tree(&[TreeEntry {
                path: "script.sh".into(),
                oid: blob,
                mode: FileMode::BlobExecutable,
            }])
            .unwrap();

        let entries = repo.list_tree(tree).unwrap();
        assert_eq!(entries[0].mode, FileMode::BlobExecutable);
    }

    #[test]
    fn creating_a_commit_does_not_move_any_ref() {
        let (_dir, repo) = scratch();
        let tree = repo.build_tree(&[]).unwrap();

        let commit = repo.create_commit(tree, &[], "test").unwrap();

        assert_eq!(
            repo.head_commit().unwrap(),
            None,
            "HEAD must not have moved"
        );
        assert_eq!(repo.commit(commit).unwrap().tree, tree);
    }

    #[test]
    fn diffing_trees_reports_adds_modifications_and_deletions() {
        let (_dir, repo) = scratch();
        let a = repo.write_blob(b"a\n").unwrap();
        let b = repo.write_blob(b"b\n").unwrap();

        let old = repo
            .build_tree(&[
                TreeEntry {
                    path: "keep".into(),
                    oid: a,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "gone".into(),
                    oid: a,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "change".into(),
                    oid: a,
                    mode: FileMode::Blob,
                },
            ])
            .unwrap();
        let new = repo
            .build_tree(&[
                TreeEntry {
                    path: "keep".into(),
                    oid: a,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "change".into(),
                    oid: b,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "fresh".into(),
                    oid: b,
                    mode: FileMode::Blob,
                },
            ])
            .unwrap();

        let changes = repo.diff_trees(Some(old), new, &[]).unwrap();
        let described: Vec<_> = changes.iter().map(|c| (c.kind, c.path.as_str())).collect();

        assert_eq!(
            described,
            [
                (ChangeKind::Modified, "change"),
                (ChangeKind::Added, "fresh"),
                (ChangeKind::Deleted, "gone"),
            ]
        );

        // The same diff, narrowed. A pathspec that matches one file must leave
        // the other two out rather than merely un-highlighted.
        let narrowed = repo
            .diff_trees(Some(old), new, &["fresh".to_string()])
            .unwrap();
        assert_eq!(narrowed.len(), 1, "{narrowed:?}");
        assert_eq!(narrowed[0].path, "fresh");
    }

    #[test]
    fn a_diff_stat_counts_the_lines_of_each_change() {
        let (_dir, repo) = scratch();
        let one = repo.write_blob(b"a\nb\n").unwrap();
        let two = repo.write_blob(b"a\nB\nc\n").unwrap();

        let old = repo
            .build_tree(&[TreeEntry {
                path: "f".into(),
                oid: one,
                mode: FileMode::Blob,
            }])
            .unwrap();
        let new = repo
            .build_tree(&[TreeEntry {
                path: "f".into(),
                oid: two,
                mode: FileMode::Blob,
            }])
            .unwrap();

        let stats = repo.diff_stat(Some(old), new, &[]).unwrap();

        assert_eq!(stats.len(), 1, "{stats:?}");
        assert_eq!(stats[0].path, "f");
        assert_eq!(stats[0].kind, ChangeKind::Modified);
        // `b` became `B` and `c` appeared: two lines in, one out.
        assert_eq!((stats[0].insertions, stats[0].deletions), (2, 1));
        assert!(!stats[0].binary);
    }

    /// A binary file has no lines to count, and saying so is the point: two
    /// zeroes would read as "nothing changed" for a replaced image.
    #[test]
    fn a_binary_change_is_reported_without_counts() {
        let (_dir, repo) = scratch();
        let blob = repo
            .write_blob(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d")
            .unwrap();

        let empty = repo.build_tree(&[]).unwrap();
        let new = repo
            .build_tree(&[TreeEntry {
                path: "logo.png".into(),
                oid: blob,
                mode: FileMode::Blob,
            }])
            .unwrap();

        let stats = repo.diff_stat(Some(empty), new, &[]).unwrap();

        assert_eq!(stats.len(), 1, "{stats:?}");
        assert!(stats[0].binary, "{stats:?}");
        assert_eq!((stats[0].insertions, stats[0].deletions), (0, 0));
    }

    /// A commit whose tree holds exactly these files.
    fn commit_files(repo: &LibGit2, parents: &[Oid], files: &[(&str, &str)]) -> Oid {
        let entries: Vec<TreeEntry> = files
            .iter()
            .map(|(path, content)| TreeEntry {
                path: (*path).to_string(),
                oid: repo.write_blob(content.as_bytes()).unwrap(),
                mode: FileMode::Blob,
            })
            .collect();
        let tree = repo.build_tree(&entries).unwrap();
        repo.create_commit(tree, parents, "test").unwrap()
    }

    /// The bug this whole preview exists for: a file only one side has is not a
    /// deletion, because the merge base has it too.
    #[test]
    fn a_merge_preview_keeps_files_only_one_side_has() {
        let (_dir, repo) = scratch();

        let base = commit_files(&repo, &[], &[("shared", "one\n")]);
        let ours = commit_files(
            &repo,
            &[base],
            &[("shared", "one\n"), ("mine.md", "notes\n")],
        );
        let theirs = commit_files(&repo, &[base], &[("shared", "two\n")]);

        let preview = repo.merge_preview(ours, theirs).unwrap();

        assert!(preview.conflicts.is_empty(), "{preview:?}");
        let changes = repo
            .diff_trees(Some(repo.commit(ours).unwrap().tree), preview.tree, &[])
            .unwrap();
        let described: Vec<_> = changes.iter().map(|c| (c.kind, c.path.as_str())).collect();
        assert_eq!(
            described,
            [(ChangeKind::Modified, "shared")],
            "only the template's own change should show"
        );
    }

    /// Markers rather than a resolution: the preview shows what a merge would
    /// leave in the worktree, and `git merge-tree --write-tree` does the same.
    #[test]
    fn a_conflicting_merge_preview_writes_markers_and_names_the_path() {
        let (_dir, repo) = scratch();

        let base = commit_files(&repo, &[], &[("f", "base\n")]);
        let ours = commit_files(&repo, &[base], &[("f", "mine\n")]);
        let theirs = commit_files(&repo, &[base], &[("f", "theirs\n")]);

        let preview = repo.merge_preview(ours, theirs).unwrap();

        assert_eq!(preview.conflicts, ["f"]);
        let entries = repo.list_tree(preview.tree).unwrap();
        let content = String::from_utf8(repo.read_blob(entries[0].oid).unwrap()).unwrap();
        assert!(content.contains("<<<<<<<"), "{content}");
        assert!(content.contains("mine"), "{content}");
        assert!(content.contains("theirs"), "{content}");
    }

    /// One side deleted what the other changed. Git keeps the surviving
    /// content and leaves the decision to the user; so must the preview, and it
    /// must still produce a writable tree.
    #[test]
    fn a_delete_modify_conflict_keeps_the_surviving_content() {
        let (_dir, repo) = scratch();

        let base = commit_files(&repo, &[], &[("f", "base\n"), ("other", "x\n")]);
        let ours = commit_files(&repo, &[base], &[("f", "mine\n"), ("other", "x\n")]);
        let theirs = commit_files(&repo, &[base], &[("other", "x\n")]);

        let preview = repo.merge_preview(ours, theirs).unwrap();

        assert_eq!(preview.conflicts, ["f"]);
        let entries = repo.list_tree(preview.tree).unwrap();
        let f = entries.iter().find(|e| e.path == "f").expect("f is kept");
        assert_eq!(repo.read_blob(f.oid).unwrap(), b"mine\n");
    }

    #[test]
    fn a_merge_preview_moves_nothing() {
        let (_dir, repo) = scratch();

        let base = commit_files(&repo, &[], &[("f", "base\n")]);
        repo.set_ref("refs/heads/main", base, "test").unwrap();
        let ours = commit_files(&repo, &[base], &[("f", "mine\n")]);
        let theirs = commit_files(&repo, &[base], &[("f", "theirs\n")]);

        repo.merge_preview(ours, theirs).unwrap();

        assert_eq!(repo.resolve_ref("refs/heads/main").unwrap(), Some(base));
        assert!(
            !repo.repo.index().unwrap().has_conflicts(),
            "the repository index must be untouched"
        );
    }

    #[test]
    fn a_ref_can_be_set_and_resolved() {
        let (_dir, repo) = scratch();
        let tree = repo.build_tree(&[]).unwrap();
        let commit = repo.create_commit(tree, &[], "test").unwrap();

        repo.set_ref("refs/tpl/demo", commit, "test").unwrap();

        assert_eq!(repo.resolve_ref("refs/tpl/demo").unwrap(), Some(commit));
        assert_eq!(repo.resolve_ref("refs/tpl/absent").unwrap(), None);
    }

    #[test]
    fn a_configuration_value_reads_through_gits_own_precedence() {
        let (_dir, repo) = scratch();
        repo.set_config_str("tpl.remote", "upstream").unwrap();
        repo.set_config_bool("tpl.autoPush", true).unwrap();

        assert_eq!(
            repo.config_string("tpl.remote").unwrap().as_deref(),
            Some("upstream")
        );
        assert_eq!(repo.config_bool("tpl.autoPush").unwrap(), Some(true));
        assert_eq!(repo.config_string("tpl.absent").unwrap(), None);
    }

    /// A refused connection and a full disk are both `ErrorClass::Os`, so the
    /// class alone cannot decide. What decides is whether the caller knows the
    /// remote already answered — and when it did, an `Os` error is the local
    /// write, not unreachability. This is the misclassification that reported a
    /// full `$TMPDIR` as `tpl::git::network`.
    #[test]
    fn a_local_failure_after_the_remote_answered_is_a_clone_failure() {
        let error = git2::Error::new(
            ErrorCode::GenericError,
            ErrorClass::Os,
            "failed to initialize repository with template 'info/exclude': Disk quota exceeded",
        );

        let error = translate_remote(
            "https://host.invalid/t.git",
            Some(Path::new("/tmp/x")),
            &error,
        );

        assert!(
            matches!(&error, GitError::Clone { path, reason, .. }
                if path == Path::new("/tmp/x") && reason.contains("Disk quota exceeded")),
            "{error:?}"
        );
    }

    /// The converse: a transport class stays a network failure even once the
    /// remote has answered, because a connection can drop mid-transfer.
    #[test]
    fn a_transport_failure_after_the_remote_answered_is_still_a_network_failure() {
        let error = git2::Error::new(ErrorCode::GenericError, ErrorClass::Net, "connection reset");

        let error = translate_remote(
            "https://host.invalid/t.git",
            Some(Path::new("/tmp/x")),
            &error,
        );

        assert!(
            matches!(&error, GitError::Network { reason, .. } if reason == "connection reset"),
            "{error:?}"
        );
    }

    /// Authentication is decided before anything else, and outranks having a
    /// destination to blame: no amount of disk space fixes a rejected key.
    #[test]
    fn a_rejected_credential_is_an_authentication_failure() {
        let error = git2::Error::new(ErrorCode::Auth, ErrorClass::Http, "401");

        let error = translate_remote(
            "https://host.invalid/t.git",
            Some(Path::new("/tmp/x")),
            &error,
        );

        assert!(
            matches!(&error, GitError::Authentication { methods, .. }
                if methods.contains("credential helper")),
            "{error:?}"
        );
    }

    /// `credential_callbacks` gives up with `Error::from_str`, which carries
    /// `ErrorCode::Generic` and `ErrorClass::None`. Only the message identifies
    /// it, so the message sniff is load-bearing rather than belt-and-braces.
    #[test]
    fn a_callback_that_ran_out_of_credentials_is_recognised_by_its_message() {
        let error = git2::Error::from_str(
            "no usable credentials: tried the SSH agent, the default key paths and the credential helper",
        );
        assert!(is_auth(&error));

        let error = translate_remote("git@host.invalid:t.git", None, &error);

        assert!(
            matches!(&error, GitError::Authentication { methods, .. } if methods.contains("SSH agent")),
            "{error:?}"
        );
    }
}
