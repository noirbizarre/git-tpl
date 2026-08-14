//! Local and per-user preferences, read from Git configuration.
//!
//! The split with `.config/git.tpl.toml` follows one rule: **would a new
//! contributor cloning this repository need it to be true?** The template
//! source, yes — a fresh clone must be understandable without any Git
//! configuration at all. `tpl.autoPush`, no — that is a statement about how one
//! person works. See `docs/adr/010-config-location.md`.

use crate::git::{GitBackend, GitError};

/// The remote used by `fetch` and `push` when none is configured.
pub const DEFAULT_REMOTE: &str = "origin";

/// Keys, namespaced under `tpl.` like every other Git extension.
mod keys {
    pub const REMOTE: &str = "tpl.remote";
    pub const AUTO_PUSH: &str = "tpl.autoPush";
    pub const INTERACTIVE: &str = "tpl.interactive";
}

/// Resolved preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    /// The remote for `fetch` and `push`.
    pub remote: String,
    /// Whether to push the rendered ref after a successful update.
    pub auto_push: bool,
    /// Whether to prompt for unanswered questions.
    pub interactive: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            remote: DEFAULT_REMOTE.to_string(),
            // Both default to the least surprising thing. Pushing a ref nobody
            // asked to share, or silently accepting defaults for questions a
            // template just added, are each worse than the extra command.
            auto_push: false,
            interactive: true,
        }
    }
}

impl Preferences {
    /// Read preferences from a repository.
    ///
    /// libgit2's configuration snapshot applies Git's own precedence —
    /// repository, then user, then system — so `git config tpl.remote` and
    /// git-tpl always agree about what is in effect.
    pub fn load(repo: &impl GitBackend) -> Result<Self, GitError> {
        let defaults = Self::default();
        Ok(Self {
            remote: repo
                .config_string(keys::REMOTE)?
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(defaults.remote),
            auto_push: repo
                .config_bool(keys::AUTO_PUSH)?
                .unwrap_or(defaults.auto_push),
            interactive: repo
                .config_bool(keys::INTERACTIVE)?
                .unwrap_or(defaults.interactive),
        })
    }

    /// Apply command-line overrides, which win over everything.
    pub fn with_overrides(
        mut self,
        remote: Option<&str>,
        push: bool,
        non_interactive: bool,
    ) -> Self {
        if let Some(remote) = remote {
            self.remote = remote.to_string();
        }
        // `--push` can only turn pushing on. There is no `--no-push`, because
        // `tpl.autoPush` is opt-in, so the only way to reach here with it set
        // is to have asked for it.
        if push {
            self.auto_push = true;
        }
        if non_interactive {
            self.interactive = false;
        }
        self
    }

    /// The refspec that fetches template refs into the remote's namespace.
    ///
    /// Passed per-invocation rather than written into `.git/config`, so that a
    /// plain `git fetch` stays plain — including for contributors who never run
    /// git-tpl and never configured anything.
    pub fn fetch_refspec(&self) -> String {
        format!("+refs/tpl/*:refs/remotes/{}/tpl/*", self.remote)
    }
}

/// The refspec that pushes one template ref.
///
/// Explicit and never forced: a rendered ref is history others may have merged
/// from, and overwriting it destroys the merge base their next update needs.
pub fn push_refspec(ref_name: &str) -> String {
    format!("{ref_name}:{ref_name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::libgit2::LibGit2;

    fn repo() -> (tempfile::TempDir, LibGit2) {
        let dir = tempfile::tempdir().unwrap();
        let repo = LibGit2::init(dir.path()).unwrap();
        (dir, repo)
    }

    /// A fresh clone must work with no configuration at all.
    #[test]
    fn an_unconfigured_repository_gets_sensible_defaults() {
        let (_dir, repo) = repo();
        let preferences = Preferences::load(&repo).unwrap();

        assert_eq!(preferences.remote, "origin");
        assert!(!preferences.auto_push, "pushing must be opt-in");
        assert!(preferences.interactive);
    }

    #[test]
    fn configuration_overrides_the_defaults() {
        let (_dir, repo) = repo();
        repo.set_config_str("tpl.remote", "upstream").unwrap();
        repo.set_config_bool("tpl.autoPush", true).unwrap();
        repo.set_config_bool("tpl.interactive", false).unwrap();

        let preferences = Preferences::load(&repo).unwrap();

        assert_eq!(preferences.remote, "upstream");
        assert!(preferences.auto_push);
        assert!(!preferences.interactive);
    }

    #[test]
    fn a_command_line_flag_beats_configuration() {
        let (_dir, repo) = repo();
        repo.set_config_str("tpl.remote", "upstream").unwrap();

        let preferences =
            Preferences::load(&repo)
                .unwrap()
                .with_overrides(Some("fork"), false, false);

        assert_eq!(preferences.remote, "fork");
    }

    #[test]
    fn an_empty_configured_remote_falls_back_to_the_default() {
        let (_dir, repo) = repo();
        repo.set_config_str("tpl.remote", "  ").unwrap();

        assert_eq!(Preferences::load(&repo).unwrap().remote, "origin");
    }

    /// Writing this into `.git/config` would change what a bare `git fetch`
    /// does for everyone who clones, which is precisely what must not happen.
    #[test]
    fn the_fetch_refspec_targets_the_remotes_namespace() {
        let preferences = Preferences {
            remote: "upstream".into(),
            ..Preferences::default()
        };
        assert_eq!(
            preferences.fetch_refspec(),
            "+refs/tpl/*:refs/remotes/upstream/tpl/*"
        );
    }

    /// No leading `+`. A rendered ref is history others may have merged from.
    #[test]
    fn the_push_refspec_is_never_forced() {
        let refspec = push_refspec("refs/tpl/demo");
        assert_eq!(refspec, "refs/tpl/demo:refs/tpl/demo");
        assert!(!refspec.starts_with('+'));
    }
}
