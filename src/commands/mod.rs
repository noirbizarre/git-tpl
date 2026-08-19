//! One module per subcommand.

mod backport;
mod completion;
mod context;
mod diff;
mod fetch;
mod init;
mod lint;
mod man;
mod merge;
mod push;
mod questions;
mod render;
mod show;
mod status;
mod test;
mod update;

pub use backport::run as backport;
pub use completion::run as completion;
pub use context::run as context;
pub use diff::run as diff;
pub use fetch::run as fetch;
pub use init::run as init;
pub use lint::run as lint;
pub use man::run as man;
pub use merge::run as merge;
pub use push::run as push;
pub use questions::run as questions;
pub use render::run as render;
pub use show::run as show;
pub use status::run as status;
pub use test::run as test;
pub use update::run as update;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tpl::git::GitBackend;
use tpl::git::libgit2::LibGit2;
use tpl::ops::OpError;
use tpl::userconfig::UserConfig;

use crate::cli::{AnswerArgs, GlobalArgs};
use crate::theme::Theme;

/// How a command talks, without any assumption that a repository exists.
///
/// `Session` is this plus the repository. The split is what lets `render`,
/// `lint`, `questions` and `context` run against a template alone: they answer
/// questions *about a template*, and requiring a project to ask would be the
/// scaffolding tax those commands exist to remove.
pub struct Reporter {
    /// How to present output.
    pub theme: Theme,
    /// Global flags.
    pub global: GlobalArgs,
}

impl Reporter {
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
    /// `--json` swallows is a warning nobody reads, and the cases that use this
    /// — a deprecated flag, answers that name no question, files a `.gitignore`
    /// removed from a render — are all things a caller is getting wrong right
    /// now. stderr, so a JSON payload on stdout stays parseable.
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

/// A command that needs no repository.
///
/// The user configuration is still read: `[defaults]`, `[shortcuts]` and
/// `[trust]` all apply to a template resolved on its own.
pub struct Standalone {
    /// How to talk.
    pub out: Reporter,
    /// The user's own preferences, read once per command.
    pub user: UserConfig,
}

impl Standalone {
    /// Everything a project-free command needs.
    pub fn new(global: &GlobalArgs) -> Result<Self, OpError> {
        Ok(Self {
            out: Reporter {
                theme: Theme::resolve(global.color),
                global: global.clone(),
            },
            user: UserConfig::load()?,
        })
    }
}

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
    /// How to talk.
    //
    // The `Reporter` itself rather than a copy of its fields: verbosity policy
    // is one decision, and a `Session` that reimplemented `say`/`warn`/`blank`
    // meant changing it in two places. The doc comments had already drifted.
    pub out: Reporter,
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
        Self::discover_at(&current_dir()?, global)
    }

    /// Discover the repository containing `path`.
    ///
    /// `init` is the only command that takes a destination, and it passes it
    /// here rather than changing the process directory: a relative template
    /// path and a relative `--answers-from` are relative to where the command
    /// was typed, and a `chdir` would reinterpret both without saying so.
    pub fn discover_at(path: &Path, global: &GlobalArgs) -> Result<Self, OpError> {
        // Searches upwards, like every Git command, so running from a
        // subdirectory works.
        let repo = LibGit2::discover(path)?;
        let root = repo.workdir()?;
        Ok(Self {
            repo,
            root,
            out: Reporter {
                theme: Theme::resolve(global.color),
                global: global.clone(),
            },
            user: UserConfig::load()?,
        })
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

/// Refuse supplied answers that name no question, under `--strict-answers`.
///
/// The lenient default exists for *recorded* answers: a template drops
/// questions over time, and a project that answered one is not at fault. A
/// hand-written `--answers-from` is a different trust level — there a typo'd
/// key silently swaps in the default, and for a boolean that deletes a whole
/// conditional subtree while the warning scrolls past.
pub fn enforce_strict_answers(
    args: &AnswerArgs,
    ignored: &[String],
    known: impl IntoIterator<Item = String>,
) -> Result<(), OpError> {
    if !args.strict_answers {
        return Ok(());
    }
    let Some(key) = ignored.first() else {
        return Ok(());
    };
    let known: Vec<String> = known.into_iter().collect();
    let suggestion = tpl::suggest::closest(key, known.iter().map(String::as_str))
        .map(|close| format!("Did you mean `{close}`? "))
        .unwrap_or_default();
    Err(tpl::answers::AnswersError::UnknownKey {
        key: key.clone(),
        suggestion,
    }
    .into())
}

/// Say what `.gitignore` removed from a `--dirty` render.
///
/// A count always, the paths under `-v`. The ignore stack includes
/// `core.excludesFile`, so this is regularly a global rule the author forgot
/// they wrote — and inside a render there is no `git status` to reveal it.
///
/// Called by every command that resolves with `--dirty`, not just `render`:
/// the field is populated identically for all of them, and an `init --dirty`
/// that silently dropped the files `render --dirty` warns about is the same
/// afternoon lost.
pub fn report_ignored_paths(out: &Reporter, ignored: &[String]) {
    if ignored.is_empty() {
        return;
    }
    out.warn(crate::theme::warning(
        &out.theme,
        &format!(
            "{} file(s) skipped by .gitignore and absent from the rendering",
            ignored.len()
        ),
    ));
    if out.global.verbose > 0 {
        for path in ignored {
            out.warn(crate::theme::muted(&out.theme, &format!("  {path}")));
        }
    } else {
        out.warn(crate::theme::muted(
            &out.theme,
            "  run with -v to list them, or `git check-ignore -v <path>` to find the rule",
        ));
    }
}

/// Report supplied answers that name no question in the template.
///
/// Not an error: an answers file carried over from another generator has
/// `_src_path` in it, and a template drops questions over time. Not silent
/// either, because that is how a typo'd key becomes an afternoon spent
/// wondering why an answer had no effect.
///
/// Takes the `Reporter` rather than the `Session`, so the project-free
/// commands reach the same function: there is one wording for this warning.
pub fn report_ignored(out: &Reporter, ignored: &[String]) {
    if ignored.is_empty() {
        return;
    }

    // `warn`, not `say`: under `--json` this is the difference between a caller
    // discovering their typo now and discovering it when the rendered file is
    // wrong. The JSON payload carries it too, in `ignoredAnswers`.
    out.warn(crate::theme::warning(
        &out.theme,
        "answers ignored: they name no question in this template",
    ));
    for key in ignored {
        out.warn(format!("  {key}"));
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
