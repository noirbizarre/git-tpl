//! One module per subcommand.

mod diff;
mod fetch;
mod init;
mod merge;
mod push;
mod show;
mod status;
mod update;

pub use diff::run as diff;
pub use fetch::run as fetch;
pub use init::run as init;
pub use merge::run as merge;
pub use push::run as push;
pub use show::run as show;
pub use status::run as status;
pub use update::run as update;

use std::collections::BTreeMap;
use std::path::PathBuf;

use tpl::git::GitBackend;
use tpl::git::libgit2::LibGit2;
use tpl::ops::OpError;
use tpl::userconfig::UserConfig;

use crate::cli::{AnswerArgs, GlobalArgs};
use crate::theme::Theme;

/// What every command needs: the repository, where it is, and how to talk.
///
/// Named `Session` rather than `Context` because `tpl::Context` is the render
/// context — the answers, computed values and data a template sees — and both
/// are in scope in this crate. Two unrelated domain objects sharing the
/// project's most-used type name is how a reader ends up looking for `answers`
/// on the wrong one.
pub struct Session {
    /// The repository the command runs against.
    pub repo: LibGit2,
    /// Its working directory — where `.config/git.tpl.toml` lives.
    pub root: PathBuf,
    /// How to present output.
    pub theme: Theme,
    /// Global flags.
    pub global: GlobalArgs,
    /// The user's own preferences, read once per command.
    ///
    /// Read here rather than in `ops` so that the library never touches the
    /// environment, and so that the four call sites — `init`, `init --dry-run`,
    /// `update`, `update --dry-run` — cannot come to disagree about which file
    /// they read.
    pub user: UserConfig,
}

impl Session {
    /// Discover the repository containing the current directory.
    pub fn discover(global: &GlobalArgs) -> Result<Self, OpError> {
        let cwd = current_dir()?;
        // Searches upwards, like every Git command, so running from a
        // subdirectory works.
        let repo = LibGit2::discover(&cwd)?;
        let root = repo.workdir()?;
        Ok(Self {
            repo,
            root,
            theme: Theme::resolve(global.color),
            global: global.clone(),
            user: UserConfig::load()?,
        })
    }

    /// Whether to print anything beyond errors.
    ///
    /// `--json` silences the prose as well as `--quiet` does: a caller asking
    /// for a machine-readable answer did not also ask for a narration of how it
    /// was reached, and warnings still get through via [`warn`](Self::warn).
    pub fn speaks(&self) -> bool {
        !self.global.quiet && !self.global.json
    }

    /// Print a line to stderr, unless quiet.
    ///
    /// Human output goes to stderr so that `--json` keeps stdout
    /// machine-readable.
    pub fn say(&self, line: impl AsRef<str>) {
        if self.speaks() {
            eprintln!("{}", line.as_ref());
        }
    }

    /// Print a warning to stderr, whatever the verbosity.
    ///
    /// Deliberately louder than [`say`](Self::say). A warning that `--quiet` or
    /// `--json` swallows is a warning nobody reads, and the two cases that use
    /// this — a deprecated flag, and answers that name no question — are both
    /// things a caller is getting wrong right now. stderr, so a JSON payload on
    /// stdout stays parseable.
    pub fn warn(&self, line: impl AsRef<str>) {
        eprintln!("{}", line.as_ref());
    }

    /// Print a blank line, unless quiet.
    pub fn blank(&self) {
        if self.speaks() {
            eprintln!();
        }
    }
}

/// The current directory, or an error that says which step failed.
///
/// Shared with `init --init`, which needs it before there is a repository to
/// discover — and which had its own copy of this mapping, with the same
/// context string, until the two were one.
pub fn current_dir() -> Result<PathBuf, OpError> {
    std::env::current_dir().map_err(|e| {
        OpError::Git(tpl::git::GitError::Backend {
            context: "determine the current directory".into(),
            reason: e.to_string(),
        })
    })
}

/// Turn `--trust` and the interactivity preferences into what `ops` expects.
///
/// Exactly parallel to [`answering`], and for the same reason: `--defaults` and
/// `tpl.interactive false` both mean there is nobody to ask, and a capability
/// granted by omission on a CI runner is the worst version of this feature.
pub fn trust<'a>(
    args: &AnswerArgs,
    trusted: bool,
    interactive_allowed: bool,
    confirmer: &'a mut dyn tpl::data::TrustGate,
) -> tpl::ops::Trust<'a> {
    if trusted {
        tpl::ops::Trust::always()
    } else if args.defaults || !interactive_allowed {
        tpl::ops::Trust::refuse()
    } else {
        tpl::ops::Trust::Ask(confirmer)
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

/// Report supplied answers that name no question in the template.
///
/// Not an error: an answers file carried over from another generator has
/// `_src_path` in it, and a template drops questions over time. Not silent
/// either, because that is how a typo'd key becomes an afternoon spent
/// wondering why an answer had no effect.
pub fn report_ignored(ctx: &Session, ignored: &[String]) {
    if ignored.is_empty() {
        return;
    }

    ctx.blank();
    ctx.say(crate::theme::warning(
        &ctx.theme,
        "answers ignored: they name no question in this template",
    ));
    for key in ignored {
        ctx.say(format!("  {key}"));
    }
}

/// Everything supplied without a prompt, in precedence order.
///
/// Files first, in the order they were given, then `--answer`. So:
///
/// ```text
/// --answer  >  the last --answers-from  >  earlier --answers-from
///           >  answers in .config/git.tpl.toml  >  the question's default
/// ```
///
/// The last two are applied by `ops::update`, which is the only place that has
/// a recorded configuration to merge. Stated and implemented once here so that
/// the four call sites — `init`, `init --dry-run`, `update`,
/// `update --dry-run` — cannot come to disagree about the order.
pub fn supplied(args: &AnswerArgs) -> Result<BTreeMap<String, tpl::template::Value>, OpError> {
    let mut out = BTreeMap::new();

    for path in &args.answers_from {
        out.extend(tpl::answers::load(path)?);
    }

    out.extend(
        args.parsed()
            .map_err(|message| OpError::InvalidArgument { message })?,
    );

    Ok(out)
}
