//! `.gitignore` evaluation for the `--dirty` working-tree walk.
//!
//! This exists because libgit2 gets one case wrong. `git_ignore_path_is_ignored`
//! will not let a negation in a repository `.gitignore` override a rule that
//! came from a *lower-precedence* ignore file — `core.excludesFile` or
//! `.git/info/exclude`. Git will. So a template that ships
//!
//! ```gitignore
//! !mise.toml
//! ```
//!
//! against the widespread global rule hiding mise configuration loses the
//! rendered `mise.toml` from a `--dirty` render, while `git add -A` stages it.
//! The rendering then differs by flag, which is the one thing `--dirty` is
//! careful about. See <https://github.com/noirbizarre/git-tpl/issues/51> and
//! ADR-017.
//!
//! libgit2 does not report *which* rule matched, so the answer cannot be
//! post-corrected; the stack has to be evaluated here. Nothing in this module
//! touches `git2`, which also keeps the `git-backend-isolation` hook satisfied
//! and makes the precedence rules unit-testable without a repository.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// The ignore rules in force for one directory, outermost layer first.
///
/// Cheap to clone: a layer is shared, and the walk is depth-first, so each
/// directory derives its own stack rather than mutating a shared one. A
/// push/pop pair would be one early return away from leaking a layer into a
/// sibling directory, which is the kind of bug that shows up as one file
/// missing from one render.
#[derive(Clone, Default)]
pub(crate) struct IgnoreStack {
    layers: Vec<Arc<Gitignore>>,
}

impl IgnoreStack {
    /// Build the stack in force *above* `start`.
    ///
    /// Order is Git's, weakest first: `core.excludesFile`, then
    /// `.git/info/exclude`, then every `.gitignore` from the working-tree root
    /// down to but excluding `start` — the walk pushes `start`'s own on entry.
    ///
    /// The chain from the working-tree root matters: a template source can be a
    /// subdirectory of its repository, and the rules above it still apply.
    pub(crate) fn new(
        workdir: &Path,
        gitdir: &Path,
        excludes_file: Option<&Path>,
        start: &Path,
    ) -> Self {
        let mut stack = Self::default();

        // Anchored at the working-tree root, like Git: a pattern such as
        // `/target` in a global ignore file means the repository's `target`,
        // not one relative to wherever the file happens to live.
        if let Some(path) = excludes_file
            .map(PathBuf::from)
            .or_else(default_excludes_file)
        {
            stack.push_file(workdir, &path);
        }
        stack.push_file(workdir, &gitdir.join("info").join("exclude"));

        // Only the directories strictly between the root and `start`. If
        // `start` is not under `workdir` there is no chain to walk, and the
        // per-directory files the walk itself pushes are all that apply.
        if let Ok(relative) = start.strip_prefix(workdir) {
            let mut dir = workdir.to_path_buf();
            for component in relative.components() {
                stack.push_dir(&dir);
                dir.push(component);
            }
        }

        stack
    }

    /// The stack in force inside `dir`, with `dir`'s own `.gitignore` on top.
    pub(crate) fn entering(&self, dir: &Path) -> Self {
        let mut stack = self.clone();
        stack.push_dir(dir);
        stack
    }

    /// Whether Git would leave this path out of `git add -A`.
    ///
    /// The innermost layer with an opinion wins, and within a layer the last
    /// matching pattern wins. That layering is the whole point of this module:
    /// it is precisely what libgit2 does not do.
    ///
    /// `path` is absolute; each layer strips its own root before matching.
    pub(crate) fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        for layer in self.layers.iter().rev() {
            match layer.matched(path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    }

    /// Push `dir/.gitignore`, anchored at `dir`.
    fn push_dir(&mut self, dir: &Path) {
        self.push_file(dir, &dir.join(".gitignore"));
    }

    /// Push one ignore file, anchoring its patterns at `root`.
    ///
    /// Silent on failure, deliberately. A missing file is the normal case, and
    /// Git itself never refuses to operate over a `.gitignore` it could not
    /// parse — it drops the offending line. Aborting a render because a stale
    /// glob three directories up is malformed would trade a wrong file list for
    /// no file list at all.
    fn push_file(&mut self, root: &Path, file: &Path) {
        if !file.is_file() {
            return;
        }
        let mut builder = GitignoreBuilder::new(root);
        builder.add(file);
        if let Ok(matcher) = builder.build()
            && !matcher.is_empty()
        {
            self.layers.push(Arc::new(matcher));
        }
    }
}

/// Git's fallback ignore file when `core.excludesFile` is unset.
///
/// Resolved here rather than by the `ignore` crate's own global lookup, which
/// parses `~/.gitconfig` and the XDG config directly and so would disagree with
/// the value the caller resolved through libgit2's full config chain — most
/// obviously a repository-local `core.excludesFile`, which no amount of
/// environment inspection would find.
fn default_excludes_file() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("git").join("ignore"));
    }
    // `USERPROFILE` is the Windows fallback libgit2 itself uses. Git for
    // Windows usually exports `HOME`, but nothing guarantees it, and without
    // this a Windows user's global ignore file would simply not be found —
    // silently including files `git add -A` leaves out.
    let home = ["HOME", "USERPROFILE"]
        .into_iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("git")
            .join("ignore"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    /// Lay out a working tree: `(relative path, contents)`, directories
    /// created as needed.
    fn tree(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().expect("temporary directory");
        for (path, contents) in files {
            let target = dir.path().join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(&target, contents).expect("write");
        }
        dir
    }

    /// Resolve `relative` against a stack rooted at the working tree, entering
    /// each directory on the way down exactly as the walk does.
    fn ignored(root: &Path, relative: &str, is_dir: bool) -> bool {
        let gitdir = root.join(".git");
        let mut stack = IgnoreStack::new(root, &gitdir, None, root);
        let mut path = root.to_path_buf();
        let components: Vec<_> = Path::new(relative).components().collect();
        let (last, parents) = components.split_last().expect("a path to test");
        for component in parents {
            stack = stack.entering(&path);
            path.push(component);
        }
        stack = stack.entering(&path);
        path.push(last);
        stack.is_ignored(&path, is_dir)
    }

    /// The bug this module exists for. A repository `.gitignore` negation has
    /// to beat `core.excludesFile`, or a template shipping `!mise.toml` renders
    /// differently with `--dirty` than without it.
    #[test]
    fn a_negation_overrides_a_lower_precedence_ignore_file() {
        let dir = tree(&[
            (".gitignore", "!mise.toml\n"),
            ("mise.toml", ""),
            ("mise.lock", ""),
            ("global-ignore", "mise.toml\nmise.lock\n"),
        ]);
        let root = dir.path();
        let excludes = root.join("global-ignore");
        let gitdir = root.join(".git");

        let stack = IgnoreStack::new(root, &gitdir, Some(&excludes), root).entering(root);

        assert!(
            !stack.is_ignored(&root.join("mise.toml"), false),
            "the negation did not override the global rule"
        );
        // And the global rule still does its job for everything it covers,
        // which is the half a blunt fix would have broken.
        assert!(stack.is_ignored(&root.join("mise.lock"), false));
    }

    /// Within one file the last matching pattern wins — the rule that makes
    /// `*.log` plus `!keep.log` mean what an author expects.
    #[test]
    fn the_last_matching_pattern_in_a_file_wins() {
        let dir = tree(&[
            (".gitignore", "*.log\n!keep.log\n"),
            ("drop.log", ""),
            ("keep.log", ""),
        ]);

        assert!(ignored(dir.path(), "drop.log", false));
        assert!(!ignored(dir.path(), "keep.log", false));
    }

    /// A nested `.gitignore` overrides the one above it, whichever way it
    /// decides.
    #[test]
    fn an_inner_gitignore_overrides_an_outer_one() {
        let dir = tree(&[
            (".gitignore", "*.txt\n"),
            ("sub/.gitignore", "!notes.txt\n"),
            ("sub/notes.txt", ""),
            ("other.txt", ""),
        ]);

        assert!(!ignored(dir.path(), "sub/notes.txt", false));
        assert!(ignored(dir.path(), "other.txt", false));
    }

    /// `build/` names a directory and must not match a file called `build`.
    /// The walk therefore has to stat before it asks, which is why the check
    /// happens after `file_type()` rather than before it.
    #[test]
    fn a_directory_only_rule_needs_the_directory_flag() {
        let dir = tree(&[(".gitignore", "build/\n"), ("build", "")]);

        assert!(ignored(dir.path(), "build", true));
        assert!(!ignored(dir.path(), "build", false));
    }

    /// A leading slash anchors a pattern to the file's own directory; without
    /// one it matches at any depth. Getting this backwards silently removes
    /// either too much or too little.
    #[test]
    fn a_leading_slash_anchors_a_pattern_to_its_directory() {
        let dir = tree(&[
            (".gitignore", "/target\nnested\n"),
            ("target", ""),
            ("sub/target", ""),
            ("sub/nested", ""),
        ]);

        assert!(ignored(dir.path(), "target", false));
        assert!(!ignored(dir.path(), "sub/target", false));
        assert!(ignored(dir.path(), "sub/nested", false));
    }

    /// `.git/info/exclude` outranks `core.excludesFile` and is outranked by a
    /// tracked `.gitignore`.
    #[test]
    fn info_exclude_sits_between_the_global_file_and_the_repository_one() {
        let dir = tree(&[
            (".git/info/exclude", "!from-global\nfrom-exclude\n"),
            (".gitignore", "!from-exclude\n"),
            ("global-ignore", "from-global\n"),
            ("from-global", ""),
            ("from-exclude", ""),
        ]);
        let root = dir.path();
        let excludes = root.join("global-ignore");

        let stack =
            IgnoreStack::new(root, &root.join(".git"), Some(&excludes), root).entering(root);

        assert!(!stack.is_ignored(&root.join("from-global"), false));
        assert!(!stack.is_ignored(&root.join("from-exclude"), false));
    }

    /// A template source can be a subdirectory of its repository, and the
    /// rules above it still apply. Before this module they did not: the walk
    /// handed libgit2 a path relative to the template root rather than to the
    /// working tree, so an outer rule matched the wrong thing or nothing.
    #[test]
    fn rules_above_the_starting_directory_still_apply() {
        let dir = tree(&[
            (".gitignore", "*.local\n"),
            ("tpl/secret.local", ""),
            ("tpl/kept.txt", ""),
        ]);
        let root = dir.path();
        let start = root.join("tpl");

        let stack = IgnoreStack::new(root, &root.join(".git"), None, &start).entering(&start);

        assert!(stack.is_ignored(&start.join("secret.local"), false));
        assert!(!stack.is_ignored(&start.join("kept.txt"), false));
    }
}
