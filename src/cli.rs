//! Argument types only. The implementations live in [`crate::commands`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use tpl::template::Value;

/// Git-native project templates.
#[derive(Debug, Parser)]
#[command(
    name = "git-tpl",
    // What the user typed. Without this, `--help` advertises `git-tpl update`
    // for a tool whose entire premise is that you type `git tpl update`.
    bin_name = "git tpl",
    version,
    about,
    long_about = "Render a template into a Git ref, then merge it like anything else.\n\n\
                  `git tpl update` advances refs/tpl/<id> without touching your branch. \
                  You take the changes with a normal `git merge`.",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    #[command(flatten)]
    pub global: GlobalArgs,
}

/// Flags accepted before or after the subcommand.
#[derive(Debug, Clone, clap::Args)]
pub struct GlobalArgs {
    /// Print more detail
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    /// Print only what is essential
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Emit machine-readable JSON on stdout, including on failure
    //
    // Global rather than per-command, because the failure envelope has to be
    // available everywhere: a caller scripting one command must be able to
    // read *why* any command failed, including the ones that have no success
    // payload of their own.
    #[arg(long, global = true)]
    pub json: bool,

    /// When to colourise output
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
}

/// Whether to colourise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum ColorChoice {
    /// Colour when writing to a terminal.
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

/// The commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Attach a template to this repository and render it
    Init(InitArgs),

    /// Re-render the template and advance refs/tpl/<id>
    Update(UpdateArgs),

    /// Render a template into a directory, with no project and no ref
    Render(RenderArgs),

    /// Check a template for problems, without rendering it
    Lint(LintArgs),

    /// Show the template, the rendered ref, and what is pending
    Status(StatusArgs),

    /// Show what merging the template would change
    Diff(DiffArgs),
    /// Print the template's version of a file
    Show(ShowArgs),

    /// Merge refs/tpl/<id> into the current branch
    Merge(MergeArgs),

    /// Retrieve template refs from a remote
    Fetch(RemoteArgs),

    /// Publish template refs to a remote
    Push(RemoteArgs),
}

/// `git tpl init`
#[derive(Debug, clap::Args)]
pub struct InitArgs {
    /// The template — any Git URL, or a path
    pub template: String,

    /// Branch, tag or commit to render
    #[arg(long, value_name = "REF")]
    pub r#ref: Option<String>,

    /// Override the derived template id, and so the ref name
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,

    #[command(flatten)]
    pub answers: AnswerArgs,

    /// Render the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,

    /// Create the rendered ref but do not merge it into the branch
    #[arg(long)]
    pub no_merge: bool,

    /// Create the repository if there is not one here
    #[arg(long)]
    pub init: bool,

    /// Fetch remote data sources without confirming
    ///
    /// Per invocation: nothing is recorded, and the next run asks again.
    #[arg(long)]
    pub trust: bool,

    /// Report what would happen; create nothing
    #[arg(long)]
    pub dry_run: bool,
}

/// `git tpl update`
#[derive(Debug, clap::Args)]
pub struct UpdateArgs {
    /// Render this revision instead of the configured one
    #[arg(long, value_name = "REF")]
    pub r#ref: Option<String>,

    #[command(flatten)]
    pub answers: AnswerArgs,

    /// Render the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,

    /// Push the rendered ref afterwards
    #[arg(long)]
    pub push: bool,

    /// The remote to push to
    #[arg(long, value_name = "NAME")]
    pub remote: Option<String>,

    /// Fetch remote data sources without confirming
    ///
    /// Per invocation: nothing is recorded, and the next run asks again.
    #[arg(long)]
    pub trust: bool,

    /// Report what would change; write nothing
    #[arg(long)]
    pub dry_run: bool,
}

/// Answer supply, shared by the commands that render.
///
/// Flattened into `init` and `update` rather than made global: it is
/// meaningless to `diff` and `push`, and a flag that is silently ignored is
/// worse than one that is refused.
#[derive(Debug, Clone, clap::Args)]
pub struct AnswerArgs {
    /// Supply an answer, skipping its prompt (repeatable)
    #[arg(long = "answer", value_name = "KEY=VALUE")]
    pub answers: Vec<String>,

    /// Read answers from a TOML, JSON or YAML file (repeatable)
    ///
    /// Later files win over earlier ones, and `--answer` wins over all of them.
    #[arg(long = "answers-from", value_name = "PATH")]
    pub answers_from: Vec<PathBuf>,

    /// Accept every default without prompting
    #[arg(long)]
    pub defaults: bool,
}

impl AnswerArgs {
    /// Parse `--answer key=value` pairs.
    ///
    /// Values arrive as text and are turned into the question's declared type
    /// later, where that type is known — here they are simply carried as
    /// strings, since `--answer port=8080` cannot be distinguished from
    /// `--answer name=8080` without the manifest.
    pub fn parsed(&self) -> Result<BTreeMap<String, Value>, String> {
        let mut out = BTreeMap::new();
        for entry in &self.answers {
            let (key, value) = entry.split_once('=').ok_or_else(|| {
                format!(
                    "`{entry}` is not a `key=value` pair (did you mean `--answer {entry}=...`?)"
                )
            })?;
            if key.trim().is_empty() {
                return Err(format!("`{entry}` has an empty key"));
            }
            out.insert(key.trim().to_string(), Value::String(value.to_string()));
        }
        Ok(out)
    }
}

/// `git tpl render`
#[derive(Debug, clap::Args)]
pub struct RenderArgs {
    /// The template to render: a path, a URL, or a `[shortcuts]` name
    #[arg(value_name = "TEMPLATE")]
    pub template: String,

    /// Where to write the rendered files
    #[arg(long, short, value_name = "DIR")]
    pub output: PathBuf,

    /// The branch, tag or commit to render
    #[arg(long, value_name = "REF")]
    pub r#ref: Option<String>,

    /// Render this subdirectory instead of the manifest's root
    #[arg(long, value_name = "PATH")]
    pub root: Option<String>,

    /// Render the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,

    /// Replace the contents of a non-empty output directory
    #[arg(long)]
    pub force: bool,

    #[command(flatten)]
    pub answers: AnswerArgs,

    /// Allow the template's remote data sources without asking
    #[arg(long)]
    pub trust: bool,
}

/// `git tpl lint`
#[derive(Debug, clap::Args)]
pub struct LintArgs {
    /// The template to check; defaults to the current directory
    #[arg(value_name = "TEMPLATE", default_value = ".")]
    pub template: String,

    /// The branch, tag or commit to check
    #[arg(long, value_name = "REF")]
    pub r#ref: Option<String>,

    /// Check this subdirectory instead of the manifest's root
    #[arg(long, value_name = "PATH")]
    pub root: Option<String>,

    /// Check the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,
}

/// `git tpl status`
#[derive(Debug, clap::Args)]
pub struct StatusArgs {
    // Deprecated: use the global `--json`.
    //
    // Kept for one minor so that a CI job pinned to `--format json` does not
    // break on upgrade without being told why. It is hidden, so it stops being
    // discoverable immediately, and it warns on stderr when used.
    #[arg(long, value_enum, hide = true)]
    pub format: Option<Format>,
}

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum Format {
    /// Human-readable.
    Text,
    /// JSON.
    Json,
}

/// `git tpl diff`
#[derive(Debug, clap::Args)]
pub struct DiffArgs {
    /// Summarise instead of printing the patch
    #[arg(long)]
    pub stat: bool,

    /// Print paths only
    #[arg(long)]
    pub name_only: bool,

    /// Diff the other way, merged to HEAD
    #[arg(long)]
    pub reverse: bool,

    /// Limit to these paths
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<String>,
}

/// `git tpl show`
#[derive(Debug, clap::Args)]
pub struct ShowArgs {
    /// The path, relative to the repository root
    ///
    /// A plain positional rather than `last = true`: `diff` needs `--` only
    /// because it takes a variadic list, and `show` takes exactly one path.
    #[arg(value_name = "PATH")]
    pub path: String,
}

/// `git tpl merge`
#[derive(Debug, clap::Args)]
pub struct MergeArgs {
    /// Merge and stage, but do not commit
    #[arg(long)]
    pub no_commit: bool,

    /// Override the merge commit message
    #[arg(short, long, value_name = "MSG")]
    pub message: Option<String>,

    /// Abort a merge in progress
    #[arg(long, conflicts_with_all = ["no_commit", "message"])]
    pub abort: bool,
}

/// `git tpl fetch` and `git tpl push`
#[derive(Debug, clap::Args)]
pub struct RemoteArgs {
    /// The remote to use
    #[arg(long, value_name = "NAME")]
    pub remote: Option<String>,

    /// Report what would happen; transfer nothing
    #[arg(long)]
    pub dry_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_argument_definition_has_no_conflicting_flags_or_names() {
        Cli::command().debug_assert();
    }

    /// `git tpl` resolves to a `git-tpl` executable, so the help must describe
    /// the invocation the user typed rather than the file name.
    #[test]
    fn the_help_advertises_the_git_subcommand_form() {
        let rendered = Cli::command().render_usage().to_string();
        assert!(rendered.contains("git tpl"), "{rendered}");
    }

    #[test]
    fn a_global_flag_is_accepted_after_the_subcommand() {
        let cli = Cli::try_parse_from(["git-tpl", "status", "--verbose"]).unwrap();
        assert_eq!(cli.global.verbose, 1);
    }

    #[test]
    fn repeating_the_verbose_flag_raises_the_level() {
        let cli = Cli::try_parse_from(["git-tpl", "-vv", "status"]).unwrap();
        assert_eq!(cli.global.verbose, 2);
    }

    /// Asking for both more and less detail is a mistake worth refusing.
    #[test]
    fn quiet_and_verbose_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["git-tpl", "status", "-q", "-v"]).is_err());
    }

    #[test]
    fn answers_parse_into_key_value_pairs() {
        let args = AnswerArgs {
            answers: vec!["name=demo".into(), "ci=true".into()],
            defaults: false,
            answers_from: Vec::new(),
        };
        let parsed = args.parsed().unwrap();

        assert_eq!(parsed["name"], Value::String("demo".into()));
        assert_eq!(parsed["ci"], Value::String("true".into()));
    }

    /// A value may legitimately contain `=`, so only the first one separates.
    #[test]
    fn only_the_first_equals_separates_an_answer() {
        let args = AnswerArgs {
            answers: vec!["motto=a=b=c".into()],
            defaults: false,
            answers_from: Vec::new(),
        };
        assert_eq!(
            args.parsed().unwrap()["motto"],
            Value::String("a=b=c".into())
        );
    }

    #[test]
    fn an_answer_without_an_equals_is_rejected_with_a_hint() {
        let args = AnswerArgs {
            answers: vec!["name".into()],
            defaults: false,
            answers_from: Vec::new(),
        };
        let error = args.parsed().unwrap_err();
        assert!(error.contains("key=value"), "{error}");
    }

    #[test]
    fn an_empty_value_is_a_valid_answer() {
        let args = AnswerArgs {
            answers: vec!["suffix=".into()],
            defaults: false,
            answers_from: Vec::new(),
        };
        assert_eq!(
            args.parsed().unwrap()["suffix"],
            Value::String(String::new())
        );
    }

    /// `--answer` is meaningless to `diff`, and a silently ignored flag is
    /// worse than a refused one.
    #[test]
    fn answer_flags_are_refused_where_they_would_be_ignored() {
        assert!(Cli::try_parse_from(["git-tpl", "diff", "--answer", "a=b"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "push", "--defaults"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "diff", "--answers-from", "a.toml"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "show", "x", "--defaults"]).is_err());
    }

    /// `show` reads one path. A second one is a typo — most likely a `--`
    /// habit carried over from `diff` — and refusing it says so immediately.
    #[test]
    fn show_takes_exactly_one_path() {
        let cli = Cli::try_parse_from(["git-tpl", "show", "src/lib.rs"]).unwrap();
        match cli.command {
            Command::Show(args) => assert_eq!(args.path, "src/lib.rs"),
            other => panic!("expected show, got {other:?}"),
        }

        assert!(Cli::try_parse_from(["git-tpl", "show"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "show", "a", "b"]).is_err());
    }

    #[test]
    fn diff_paths_come_after_a_double_dash() {
        let cli =
            Cli::try_parse_from(["git-tpl", "diff", "--stat", "--", "Cargo.toml", "src/"]).unwrap();
        match cli.command {
            Command::Diff(args) => {
                assert!(args.stat);
                assert_eq!(args.paths, ["Cargo.toml", "src/"]);
            }
            other => panic!("expected diff, got {other:?}"),
        }
    }

    #[test]
    fn merge_abort_excludes_the_flags_it_would_ignore() {
        assert!(Cli::try_parse_from(["git-tpl", "merge", "--abort"]).is_ok());
        assert!(Cli::try_parse_from(["git-tpl", "merge", "--abort", "--no-commit"]).is_err());
    }

    #[test]
    fn init_requires_a_template() {
        assert!(Cli::try_parse_from(["git-tpl", "init"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "init", "../tpl"]).is_ok());
    }
}
