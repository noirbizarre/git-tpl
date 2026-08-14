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

use std::collections::BTreeMap;
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
pub fn report_ignored(ctx: &Context, ignored: &[String]) {
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
