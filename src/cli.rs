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

    /// Run a template's own test cases
    Test(TestArgs),

    /// List a template's questions and their schema
    Questions(QuestionsArgs),

    /// Show what a template sees, and evaluate expressions against it
    Context(ContextArgs),

    /// Show the template, the rendered ref, and what is pending
    Status(StatusArgs),

    /// Show what merging the template would change
    Diff(DiffArgs),
    /// Print the template's version of a file
    Show(ShowArgs),

    /// Merge refs/tpl/<id> into the current branch
    Merge(MergeArgs),

    /// Emit a patch carrying a local fix back to the template
    Backport(BackportArgs),

    /// Retrieve template refs from a remote
    Fetch(RemoteArgs),

    /// Publish template refs to a remote
    Push(RemoteArgs),

    /// Print a shell completion script
    Completion(CompletionArgs),

    /// Generate the man pages, in troff
    //
    // Hidden because it is a packaging tool, not a user command. `git tpl man`
    // reads like "show me the manual" and does nothing of the sort — the way a
    // user reads the manual is `git tpl --help`, which is what the pages this
    // writes are there to make work.
    #[command(hide = true)]
    Man(ManArgs),
}

/// `git tpl init`
#[derive(Debug, clap::Args)]
pub struct InitArgs {
    /// The template — any Git URL, or a path
    pub template: String,

    /// Where to render it — the current directory by default
    ///
    /// It must already be a Git repository, or be one after `--init`. Paths
    /// given to `--answers-from`, and a template given as a path, stay
    /// relative to the directory the command was run from.
    #[arg(value_name = "DIR")]
    pub directory: Option<PathBuf>,

    /// Branch, tag or commit to render
    #[arg(long, value_name = "REF", conflicts_with = "dirty")]
    pub r#ref: Option<String>,

    /// Override the derived template id, and so the ref name
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,

    #[command(flatten)]
    pub answers: AnswerArgs,

    /// Render the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,

    /// Re-ask the questions and re-render over an existing attachment
    //
    // Not the same as `update`: that re-renders with the *recorded* answers,
    // and there was no way to change them short of editing the config by hand.
    #[arg(long)]
    pub force: bool,

    /// Create the rendered ref but do not merge it into the branch
    #[arg(long)]
    pub no_merge: bool,

    /// Create the directory and the repository if there is not one here
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
    #[arg(long, value_name = "REF", conflicts_with = "dirty")]
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

    /// Fail when a supplied answer names no question
    ///
    /// Recorded answers stay lenient whatever this says: a template drops
    /// questions over time, and a project that answered one is not at fault
    /// for it. This is about the answers a caller supplied *now*.
    #[arg(long)]
    pub strict_answers: bool,
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
    #[arg(long, value_name = "REF", conflicts_with = "dirty")]
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
    #[arg(long, value_name = "REF", conflicts_with = "dirty")]
    pub r#ref: Option<String>,

    /// Check this subdirectory instead of the manifest's root
    #[arg(long, value_name = "PATH")]
    pub root: Option<String>,

    /// Check the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,

    /// Fail on a warning: `warnings`, or a single `tpl::lint::*` code
    ///
    /// Repeatable. A named code overrides `warnings`, whichever order the
    /// flags are written in.
    #[arg(short = 'D', long = "deny", value_name = "CODE|warnings")]
    pub deny: Vec<String>,

    /// Do not report a finding at all: `warnings`, or a single `tpl::lint::*` code
    ///
    /// Repeatable. A named code overrides `warnings`, whichever order the
    /// flags are written in.
    #[arg(short = 'A', long = "allow", value_name = "CODE|warnings")]
    pub allow: Vec<String>,
}

/// `git tpl test`
///
/// Deliberately carries no [`AnswerArgs`]. A case file supplies the answers,
/// and that is the point of it — a `--answer` on the command line would change
/// what every case asserts without changing what any case says. See the
/// argument tests at the bottom of this file.
#[derive(Debug, clap::Args)]
pub struct TestArgs {
    /// The template to test; defaults to the current directory
    #[arg(value_name = "TEMPLATE", default_value = ".")]
    pub template: String,

    /// Run only the cases with these names
    ///
    /// A name, not a path: `tests/minimal.toml` is the case `minimal`.
    #[arg(value_name = "CASE")]
    pub cases: Vec<String>,

    /// Read cases from this directory instead of `tests`
    #[arg(long, value_name = "DIR")]
    pub tests: Option<String>,

    /// The branch, tag or commit to test
    #[arg(long, value_name = "REF", conflicts_with = "dirty")]
    pub r#ref: Option<String>,

    /// Test this subdirectory instead of the manifest's root
    #[arg(long, value_name = "PATH")]
    pub root: Option<String>,

    /// Test the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,

    /// Record each case's rendering as its snapshot
    #[arg(long)]
    pub write: bool,

    /// Allow the template's remote data sources without asking
    #[arg(long)]
    pub trust: bool,

    /// Skip a case's `[commands]`, if it has any
    ///
    /// `tpl.testCommands` can already disable this by default; this flag
    /// only disables further — there is no way to force commands back on
    /// from the command line once configuration has said no.
    #[arg(long)]
    pub skip_commands: bool,
}

/// `git tpl backport`
///
/// Deliberately carries no [`AnswerArgs`], for `test`'s reason rather than
/// `push`'s: it renders, so the flags would not be *ignored* — they would be
/// obeyed, and that is worse. The rendering exists to reproduce the tree the
/// project was given, so it must use the recorded answers; a `--answer` would
/// produce a different tree, fail the check against the ref and refuse. A flag
/// whose only possible effect is to break the command does not belong on it.
///
/// Deliberately carries no `--ref`, for the same shape of reason. The
/// comparison is against the revision the project actually rendered; anything
/// else folds the template's own movement into the patch, which sends upstream
/// a revert of upstream.
#[derive(Debug, clap::Args)]
pub struct BackportArgs {
    /// Limit the backport to these paths
    ///
    /// Git pathspecs, matched against the rendered paths. A file the template
    /// does not produce is only considered when named here.
    #[arg(value_name = "PATHSPEC")]
    pub paths: Vec<String>,

    /// Leave these paths out; repeatable
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// Write the patch here instead of to stdout
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Fetch remote data sources without confirming
    ///
    /// Per invocation: nothing is recorded, and the next run asks again.
    #[arg(long)]
    pub trust: bool,

    /// Reverse changed template expressions without confirming
    ///
    /// Reversing a substitution keeps the `{{ }}` and carries only the change
    /// around it. Round-tripping proves the result reproduces *your* file; it
    /// cannot prove it is right for everyone else's answers, which is why each
    /// one is normally shown for confirmation. Pass this to take them all —
    /// and to un-substitute at all under `--json` or in CI, where there is
    /// nobody to ask.
    #[arg(long)]
    pub unsubstitute: bool,

    /// Choose which hunks to send, one file at a time
    ///
    /// The hunks are your own edits, as `git add -p` shows them, and the ones
    /// you keep are what the patch is then built and proved against. Needs a
    /// terminal: under `--json`, in a pipe, or with `tpl.interactive false` it
    /// is refused rather than quietly ignored.
    #[arg(short, long)]
    pub patch: bool,
}

/// `git tpl questions`
#[derive(Debug, clap::Args)]
pub struct QuestionsArgs {
    /// The template to inspect; defaults to the current directory
    #[arg(value_name = "TEMPLATE", default_value = ".")]
    pub template: String,

    /// The branch, tag or commit to inspect
    #[arg(long, value_name = "REF", conflicts_with = "dirty")]
    pub r#ref: Option<String>,

    /// Read this subdirectory instead of the manifest's root
    #[arg(long, value_name = "PATH")]
    pub root: Option<String>,

    /// Inspect the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,
}

/// `git tpl context`
#[derive(Debug, clap::Args)]
pub struct ContextArgs {
    /// The template to resolve; defaults to the current directory
    #[arg(value_name = "TEMPLATE", default_value = ".")]
    pub template: String,

    /// The branch, tag or commit to resolve
    #[arg(long, value_name = "REF", conflicts_with = "dirty")]
    pub r#ref: Option<String>,

    /// Use this subdirectory instead of the manifest's root
    #[arg(long, value_name = "PATH")]
    pub root: Option<String>,

    /// Resolve the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,

    /// Evaluate one expression against the resolved context and print it
    #[arg(long, value_name = "EXPR")]
    pub eval: Option<String>,

    #[command(flatten)]
    pub answers: AnswerArgs,

    /// Allow the template's remote data sources without asking
    #[arg(long)]
    pub trust: bool,
}

/// `git tpl status`
#[derive(Debug, clap::Args)]
pub struct StatusArgs {
    /// Compare against the template's working tree rather than its HEAD
    #[arg(long)]
    pub dirty: bool,
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

    /// Exit 1 when there is a difference, like `git diff --exit-code`
    //
    // Difference, not conflict: a conflicting preview is a correct answer to
    // the question asked, and `febbc37` deliberately kept it at zero.
    #[arg(long)]
    pub exit_code: bool,

    /// Preview the template's working tree rather than the rendered ref
    //
    // This renders, which nothing else in `diff` does. Answers come from the
    // recorded ones; `--answer` overrides them for the preview only, and
    // nothing is written anywhere.
    #[arg(long)]
    pub dirty: bool,

    #[command(flatten)]
    pub answers: AnswerArgs,

    /// Limit to these paths
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<String>,
}

/// `git tpl show`
#[derive(Debug, clap::Args)]
pub struct ShowArgs {
    /// Read from the template's working tree rather than the rendered ref
    #[arg(long)]
    pub dirty: bool,

    #[command(flatten)]
    pub answers: AnswerArgs,

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

/// `git tpl completion`
#[derive(Debug, clap::Args)]
pub struct CompletionArgs {
    /// The shell to generate a script for
    pub shell: clap_complete::Shell,
}

/// `git tpl man`
#[derive(Debug, clap::Args)]
pub struct ManArgs {
    /// Write one page per command into this directory instead of stdout
    #[arg(long, short = 'o', value_name = "DIR")]
    pub out_dir: Option<PathBuf>,
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
            strict_answers: false,
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
            strict_answers: false,
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
            strict_answers: false,
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
            strict_answers: false,
        };
        assert_eq!(
            args.parsed().unwrap()["suffix"],
            Value::String(String::new())
        );
    }

    /// A silently ignored flag is worse than a refused one.
    ///
    /// `diff` and `show` gained `--answer` when they gained `--dirty`: both
    /// now render, and a preview has to be answerable. The commands that still
    /// never render still refuse it.
    #[test]
    fn answer_flags_are_refused_where_they_would_be_ignored() {
        assert!(Cli::try_parse_from(["git-tpl", "push", "--defaults"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "fetch", "--answer", "a=b"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "merge", "--answers-from", "a.toml"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "status", "--defaults"]).is_err());
    }

    /// The other half of the same rule: a command that renders accepts them.
    #[test]
    fn answer_flags_are_accepted_where_a_preview_renders() {
        assert!(Cli::try_parse_from(["git-tpl", "diff", "--dirty", "--answer", "a=b"]).is_ok());
        assert!(Cli::try_parse_from(["git-tpl", "show", "x", "--dirty", "--defaults"]).is_ok());
        assert!(Cli::try_parse_from(["git-tpl", "render", "t", "-o", "out", "--defaults"]).is_ok());
    }

    /// `backport` is the fourth case, and its reason is `test`'s, not `push`'s.
    ///
    /// It renders, so the flags would not be ignored — they would be obeyed.
    /// The rendering exists to reproduce the tree the project was given, so a
    /// `--answer` would produce a different one, fail the check against the
    /// ref, and refuse. A flag whose only possible effect is to break the
    /// command is refused at the parser instead.
    ///
    /// `--ref` goes the same way: the recorded revision is the only baseline
    /// that yields the user's divergence rather than the template's movement.
    #[test]
    fn backport_refuses_the_flags_that_would_change_its_baseline() {
        assert!(Cli::try_parse_from(["git-tpl", "backport", "--answer", "a=b"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "backport", "--defaults"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "backport", "--answers-from", "a.toml"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "backport", "--ref", "main"]).is_err());
    }

    /// Selection is positional, exclusion is a repeatable flag.
    #[test]
    fn backport_takes_pathspecs_and_repeatable_excludes() {
        let cli = Cli::try_parse_from([
            "git-tpl",
            "backport",
            "README.md",
            "src/",
            "--exclude",
            "*.lock",
            "--exclude",
            "docs/**",
        ])
        .unwrap();
        let Command::Backport(args) = cli.command else {
            panic!("expected backport")
        };
        assert_eq!(args.paths, ["README.md", "src/"]);
        assert_eq!(args.exclude, ["*.lock", "docs/**"]);
        assert!(args.output.is_none());
    }

    /// `test` is the third case, and its reason is neither of the other two.
    ///
    /// It renders — repeatedly — so "the flag would be ignored" is not why it
    /// refuses. It refuses because the *case file* is the answer set: a
    /// `--answer` here would silently change what every case asserts while
    /// every case file still said otherwise, and a suite that does not test
    /// what it says it tests is worse than no suite.
    #[test]
    fn test_refuses_answer_flags_because_the_cases_own_the_answers() {
        assert!(Cli::try_parse_from(["git-tpl", "test", "--answer", "a=b"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "test", "--defaults"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "test", "--answers-from", "a.toml"]).is_err());
    }

    /// The template argument is optional, and the positionals after it are
    /// case names rather than a second template.
    #[test]
    fn test_defaults_to_the_current_directory_and_takes_case_names() {
        let cli = Cli::try_parse_from(["git-tpl", "test"]).unwrap();
        let Command::Test(args) = cli.command else {
            panic!("expected test")
        };
        assert_eq!(args.template, ".");
        assert!(args.cases.is_empty());

        let cli = Cli::try_parse_from(["git-tpl", "test", "./tpl", "minimal", "with-ci"]).unwrap();
        let Command::Test(args) = cli.command else {
            panic!("expected test")
        };
        assert_eq!(args.template, "./tpl");
        assert_eq!(args.cases, ["minimal", "with-ci"]);
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

    /// `--dirty` reads the working tree; `--ref` names a committed revision.
    /// Combined, `--dirty` used to win silently and `--ref` had no effect at
    /// all — refusing the combination is better than a flag that is accepted
    /// and then ignored.
    #[test]
    fn ref_and_dirty_are_mutually_exclusive_everywhere_both_exist() {
        assert!(Cli::try_parse_from(["git-tpl", "init", "t", "--ref", "main", "--dirty"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "update", "--ref", "main", "--dirty"]).is_err());
        assert!(
            Cli::try_parse_from([
                "git-tpl", "render", "t", "-o", "out", "--ref", "main", "--dirty"
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["git-tpl", "lint", "t", "--ref", "main", "--dirty"]).is_err());
        assert!(Cli::try_parse_from(["git-tpl", "test", "t", "--ref", "main", "--dirty"]).is_err());
        assert!(
            Cli::try_parse_from(["git-tpl", "questions", "t", "--ref", "main", "--dirty"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["git-tpl", "context", "t", "--ref", "main", "--dirty"]).is_err()
        );
    }

    /// `--skip-commands` can only disable `[commands]`; it takes no value and
    /// is accepted on its own.
    #[test]
    fn test_accepts_skip_commands() {
        let cli = Cli::try_parse_from(["git-tpl", "test", "--skip-commands"]).unwrap();
        let Command::Test(args) = cli.command else {
            panic!("expected test")
        };
        assert!(args.skip_commands);
    }

    #[test]
    fn a_destination_directory_is_an_optional_second_positional() {
        let cli = Cli::try_parse_from(["git-tpl", "init", "../tpl"]).unwrap();
        match cli.command {
            Command::Init(args) => assert_eq!(args.directory, None),
            other => panic!("expected init, got {other:?}"),
        }

        let cli = Cli::try_parse_from(["git-tpl", "init", "../tpl", "my-project"]).unwrap();
        match cli.command {
            Command::Init(args) => {
                assert_eq!(args.directory, Some(PathBuf::from("my-project")));
            }
            other => panic!("expected init, got {other:?}"),
        }
    }
}
