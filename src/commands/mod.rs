//! One module per subcommand.

mod diff;
mod fetch;
mod init;
mod merge;
mod push;
mod status;
mod update;

pub use diff::run as diff;
pub use fetch::run as fetch;
pub use init::run as init;
pub use merge::run as merge;
pub use push::run as push;
pub use status::run as status;
pub use update::run as update;

use std::path::PathBuf;

use tpl::git::GitBackend;
use tpl::git::libgit2::LibGit2;
use tpl::ops::OpError;

use crate::cli::{AnswerArgs, GlobalArgs};
use crate::theme::Theme;

/// What every command needs.
pub struct Context {
    /// The repository the command runs against.
    pub repo: LibGit2,
    /// Its working directory — where `.config/git.tpl.toml` lives.
    pub root: PathBuf,
    /// How to present output.
    pub theme: Theme,
    /// Global flags.
    pub global: GlobalArgs,
}

impl Context {
    /// Discover the repository containing the current directory.
    pub fn discover(global: &GlobalArgs) -> Result<Self, OpError> {
        let cwd = std::env::current_dir().map_err(|e| {
            OpError::Git(tpl::git::GitError::Backend {
                context: "determine the current directory".into(),
                reason: e.to_string(),
            })
        })?;
        // Searches upwards, like every Git command, so running from a
        // subdirectory works.
        let repo = LibGit2::discover(&cwd)?;
        let root = repo.workdir()?;
        Ok(Self {
            repo,
            root,
            theme: Theme::resolve(global.color),
            global: global.clone(),
        })
    }

    /// Whether to print anything beyond errors.
    pub fn speaks(&self) -> bool {
        !self.global.quiet
    }

    /// Print a line to stderr, unless quiet.
    ///
    /// Human output goes to stderr so that `--format json` keeps stdout
    /// machine-readable.
    pub fn say(&self, line: impl AsRef<str>) {
        if self.speaks() {
            eprintln!("{}", line.as_ref());
        }
    }

    /// Print a blank line, unless quiet.
    pub fn blank(&self) {
        if self.speaks() {
            eprintln!();
        }
    }
}

/// Turn `--answer` and `--defaults` into what `ops` expects.
pub fn answering<'a>(
    args: &AnswerArgs,
    interactive_allowed: bool,
    prompter: &'a mut dyn tpl::eval::Prompter,
) -> tpl::ops::Answering<'a> {
    if args.defaults || !interactive_allowed {
        tpl::ops::Answering::defaults()
    } else {
        tpl::ops::Answering::Interactive(prompter)
    }
}
