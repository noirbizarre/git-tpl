//! `git tpl test` — running a template's own test cases.
//!
//! A case is *data*: an answer set, a set of expectations, since ADR-027 a
//! list of commands, and since ADR-028 whether it trusts the template's own
//! declared remote data sources — written in the template repository and read
//! by the same parsers `--answers-from` uses. The assertion vocabulary is
//! closed to `files`, `absent`, `contains`, `lacks`, `error` and a snapshot. A
//! case's `[commands]` are the one place invariant 5 has a narrow, deliberate
//! exception — the harness spawns the process, never a `render`, `init` or an
//! `update`. See ADR-016, ADR-027, ADR-028 and ADR-030.
//!
//! The area of every diagnostic here is `testing` rather than `test`, because
//! `tests/diagnostics.rs` deliberately ignores codes whose area is `test` —
//! `src/report.rs` uses `tpl::test::*` as fixtures, and a runner claiming that
//! area would drop itself out of the coverage guard. [`TestingError`] follows
//! the same `testing`, not `test`, and so also matches the crate's usual
//! `<file-name>Error` convention (every other error enum is named after its
//! declaring file) — it was once `TestError`, named after the command instead.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use miette::Diagnostic;
use thiserror::Error;

use super::{
    Answering, OpError, RenderedOnce, Resolved, Target, Trust, enforce_strict_answers,
    render_resolved, resolve,
};
use crate::data::format::{Format, parse_value};
use crate::data::{Decision, declared_remotes};
use crate::git::{ChangeKind, FileMode, GitBackend, GitError, LibGit2, Oid};
use crate::render::Rendered;
use crate::suggest::closest;
use crate::template::{Value, is_expression};
use crate::userconfig::UserConfig;

/// The directory cases are read from when `--tests` does not say otherwise.
pub const DEFAULT_TESTS_DIR: &str = "tests";

/// The environment variable every `[commands]` entry sees, set to the
/// resolved template's root on disk — the working tree for `--dirty`, the
/// same working tree for a local `--ref` (`test` never resolves a remote;
/// see ADR-030). Distinct from `Resolved::root`, which names the manifest's
/// declared render subdirectory, not a filesystem path. See "`TEMPLATE_ROOT`
/// exposes the resolved template's root" in ADR-027 (issue #134).
pub const TEMPLATE_ROOT_ENV: &str = "TEMPLATE_ROOT";

/// The subdirectory of the tests directory holding recorded snapshots.
pub const SNAPSHOTS_DIR: &str = "__snapshots__";

/// The file recording each snapshot's paths, modes and digests.
pub const SNAPSHOT_MANIFEST: &str = "MANIFEST";

/// The subdirectory of a snapshot holding the rendered bytes verbatim.
pub const SNAPSHOT_FILES: &str = "files";

/// The snapshot format's version, written as the manifest's first line.
///
/// Present so a later version can recognise and migrate an older snapshot
/// rather than reporting every file as changed. See ADR-016.
const SNAPSHOT_VERSION: &str = "# git-tpl snapshot 1";

/// The subdirectory of the tests directory holding recorded case durations.
///
/// Nested under a `/`, like [`SNAPSHOTS_DIR`], for the same reason:
/// `discover`'s top-level-only scan already excludes anything under it, so a
/// durations file is never mistaken for a case. See ADR-036.
pub const DURATIONS_DIR: &str = "__timings__";

/// The file recording every case's last measured duration.
pub const DURATIONS_FILE: &str = "durations";

/// The durations format's version, written as the file's first line.
///
/// Mirrors [`SNAPSHOT_VERSION`] — same reasoning, same mechanism, a
/// different file. See ADR-036.
const DURATIONS_VERSION: &str = "# git-tpl durations 1";

/// How many bytes are sniffed for a NUL before a file is called binary.
///
/// Git's own heuristic. A binary file is stored verbatim like any other, but no
/// patch is attempted for it.
const BINARY_SNIFF_LEN: usize = 8000;

/// How much of a command's stdout/stderr is kept in a [`Failure::CommandFailed`].
///
/// Far smaller than `REMOTE_LIMIT_BYTES`: that bounds a network response this
/// process reads once; this bounds a diagnostic a person reads on a terminal
/// or in a CI log, and 64 KiB of a failing build's tail is already generous
/// for either.
const COMMAND_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;

/// Errors that stop a test run.
///
/// Only these. An unmet expectation is a [`Failure`] carried in the report, for
/// the same reason a lint finding is not an error: twelve failing cases must
/// all be reported, and an error that aborts at the first would report one.
#[derive(Debug, Error, Diagnostic)]
pub enum TestingError {
    /// The tests directory does not exist at the resolved revision.
    #[error("`{dir}` does not exist in the template at {revision_description}")]
    #[diagnostic(
        code(tpl::testing::no_tests),
        help(
            "a test case is a TOML, JSON or YAML file in `{dir}/`.\n\
             Create `{dir}/minimal.toml` with an `[answers]` table, or pass `--tests <DIR>`."
        )
    )]
    NoTests {
        /// The directory that was looked for.
        dir: String,
        /// The revision it was looked for at, as `reference (revision)`.
        //
        // `*_description`, not `revision`: the naming rule reserves `revision`
        // for an `Oid`, and this is the printable pair `describe_revision`
        // produces.
        revision_description: String,
    },

    /// A positional filter matches no case.
    ///
    /// Refused rather than reported as an empty run: a mistyped case name that
    /// exits zero having run nothing is the worst outcome this command has.
    #[error("no test case is named `{filter}`")]
    #[diagnostic(
        code(tpl::testing::no_such_case),
        help("{suggestion}available: {available}")
    )]
    NoSuchCase {
        /// The name that matched nothing.
        filter: String,
        /// A "did you mean?" prefix, or empty.
        suggestion: String,
        /// The case names that do exist.
        available: String,
    },

    /// A case file is not valid in its format.
    #[error("could not parse the test case `{path}`")]
    #[diagnostic(
        code(tpl::testing::case_parse),
        help("format: {format}\nreason: {reason}")
    )]
    CaseParse {
        /// The case file, relative to the template root.
        path: String,
        /// The format it was parsed as.
        format: String,
        /// What the parser said.
        reason: String,
    },

    /// A case file parses but is not a coherent case.
    #[error("`{path}` is not a valid test case")]
    #[diagnostic(code(tpl::testing::case_shape), help("{reason}"))]
    CaseShape {
        /// The case file, relative to the template root.
        path: String,
        /// What is wrong with it, and what to do.
        reason: String,
    },

    /// The template is not a local checkout.
    ///
    /// `test` never resolves a remote source — with or without `--ref` —
    /// unlike every other command: there is no committed-revision story for
    /// it the way there is for `render`, only a working tree, and a clone has
    /// none to read. Checked once, up front, so `--write`, `--ref` and the
    /// implicit dirty read all fail identically instead of three rules that
    /// could disagree.
    #[error("`test` only runs against a local checkout")]
    #[diagnostic(
        code(tpl::testing::remote_not_supported),
        help(
            "`{origin}` is remote, and `test` reads the working tree directly \
             rather than resolving a ref against a clone.\n\
             Clone it locally and run `git tpl test` there instead."
        )
    )]
    RemoteNotSupported {
        /// The source that is not a local checkout.
        origin: String,
    },

    /// A snapshot could not be written to the working tree.
    #[error("could not write the snapshot for `{case}`")]
    #[diagnostic(
        code(tpl::testing::snapshot_write),
        help("path:   {path}\nreason: {reason}")
    )]
    SnapshotWrite {
        /// The case whose snapshot was being written.
        case: String,
        /// The path that failed.
        path: String,
        /// What the operating system reported.
        reason: String,
    },

    /// A recorded snapshot cannot be read, or contradicts itself.
    #[error("the snapshot for `{case}` is unreadable")]
    #[diagnostic(
        code(tpl::testing::snapshot_read),
        help("path:   {path}\nreason: {reason}\nre-record it with `git tpl test --write {case}`")
    )]
    SnapshotRead {
        /// The case whose snapshot could not be read.
        case: String,
        /// The snapshot path.
        path: String,
        /// What is wrong with it.
        reason: String,
    },

    /// A case's sandbox — the scratch directory `[commands]` and the
    /// materialised rendering share — could not be created.
    ///
    /// A fact about the machine running the suite, not about the template, so
    /// it aborts the run rather than being reported per case. See ADR-027.
    #[error("could not create a sandbox for `{case}`")]
    #[diagnostic(
        code(tpl::testing::sandbox_failed),
        help("reason: {reason}\ncheck that a temporary directory can be created (see $TMPDIR)")
    )]
    SandboxFailed {
        /// The case whose sandbox could not be created.
        case: String,
        /// What the operating system reported.
        reason: String,
    },

    /// The rendering could not be materialised into a case's sandbox.
    #[error("could not write into the sandbox for `{case}`")]
    #[diagnostic(
        code(tpl::testing::sandbox_write),
        help("path:   {path}\nreason: {reason}")
    )]
    SandboxWrite {
        /// The case whose sandbox could not be written to.
        case: String,
        /// The path that failed.
        path: String,
        /// What the operating system reported.
        reason: String,
    },

    /// `--shard` was not `INDEX/TOTAL`, both 1-based, `INDEX <= TOTAL`.
    #[error("`--shard {spec}` is not usable")]
    #[diagnostic(code(tpl::testing::malformed_shard), help("{reason}"))]
    MalformedShard {
        /// The raw `--shard` value.
        spec: String,
        /// What is wrong with it, and how to write it correctly.
        reason: String,
    },

    /// A shard's split selected no cases at all.
    ///
    /// Refused rather than reported as an empty, green run — the same
    /// reasoning [`TestingError::NoSuchCase`] already gives: a matrix whose
    /// `strategy.job-total` outgrew the suite must not exit `0` having
    /// tested nothing. See ADR-036.
    #[error("shard {index}/{total} selected no cases ({available} available)")]
    #[diagnostic(
        code(tpl::testing::empty_shard),
        help("pick a smaller `--shard` TOTAL, or check it matches `strategy.job-total`")
    )]
    EmptyShard {
        /// The 1-based index that was asked for.
        index: usize,
        /// The 1-based total that was asked for.
        total: usize,
        /// How many cases existed before `--shard` narrowed them down.
        available: usize,
    },

    /// The recorded durations file cannot be read, or contradicts itself.
    #[error("the recorded durations at `{path}` are unreadable")]
    #[diagnostic(
        code(tpl::testing::durations_read),
        help("reason: {reason}\nre-record it with `git tpl test --record-durations`")
    )]
    DurationsRead {
        /// The durations file's path.
        path: String,
        /// What is wrong with it.
        reason: String,
    },

    /// The durations file could not be written.
    #[error("could not write the recorded durations")]
    #[diagnostic(
        code(tpl::testing::durations_write),
        help("path: {path}\nreason: {reason}")
    )]
    DurationsWrite {
        /// The path that failed.
        path: String,
        /// What the operating system reported.
        reason: String,
    },
}

/// One test case, as written in the template repository.
///
/// Deliberately not a `Deserialize` impl. The bytes go through
/// [`parse_value`], the same parser `--answers-from` uses, so a `.yaml` case
/// and a `.yaml` answers file cannot come to disagree about what `no` means —
/// and [`Value`] is `#[serde(untagged)]`, which makes a derived impl guess.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// The case name: the file stem, which is what a filter matches and what
    /// names the snapshot directory.
    pub name: String,
    /// The file it was read from, relative to the template root.
    pub path: String,
    /// The answers to render with.
    pub answers: BTreeMap<String, Value>,
    /// What the rendering must look like.
    pub expect: Expect,
    /// What to run before, around and after the rendering. See ADR-027.
    pub commands: Commands,
    /// The case's isolated Git sandbox, if it asked for one.
    ///
    /// `None` on omission: today's plain scratch directory, unchanged. See
    /// ADR-033.
    pub git: Option<GitSandbox>,
    /// Whether this case is tested against a recorded snapshot at all.
    ///
    /// `false` on omission: writing and comparing a snapshot are both
    /// explicit opt-ins, not a side effect of a directory happening to exist
    /// on disk — a case that never asked for one must never start being
    /// tested against one merely because `--write` ran for another case.
    pub snapshot: bool,
    /// Whether this case's render may reach the template's declared remote
    /// data sources.
    ///
    /// `true` on omission: a case renders for real unless it says otherwise,
    /// because the point of a suite is to prove what the template's own
    /// output actually looks like — an untested remote source is a gap, not a
    /// safety margin. `trust = false` is the deliberate opt-out, for a case
    /// whose whole point is proving the refused path
    /// (`tpl::data::untrusted`), deterministically and without a network.
    /// See ADR-028.
    pub trust: bool,
}

/// What a case asserts.
///
/// An empty `Expect` is a real assertion — "this answer set renders at all" —
/// which is why a case with no `[expect]` block is valid rather than useless.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Expect {
    /// Paths the rendering must contain.
    pub files: Vec<String>,
    /// Paths the rendering must not contain.
    pub absent: Vec<String>,
    /// Path to the substrings that must appear in it.
    ///
    /// A `BTreeMap` so a failure report lists paths in one order whatever order
    /// the case file wrote them in: reordering a case file must not reorder the
    /// output.
    pub contains: BTreeMap<String, Vec<String>>,
    /// Path to the substrings that must not appear in it.
    ///
    /// A missing path is a failure rather than a vacuous pass — "this file
    /// does not mention X" must not go green because the file stopped
    /// rendering entirely.
    pub lacks: BTreeMap<String, Vec<String>>,
    /// A diagnostic code the render must fail with.
    ///
    /// Present makes this a *failure* case: a successful render fails it. A
    /// code and never a message, because messages are expected to improve and
    /// a suite that pins prose makes every improvement a breaking change.
    pub error: Option<String>,
}

/// A case's setup, teardown, and everything run against the merged sandbox.
///
/// Not gated behind `Option`: an absent `[commands]` and an empty one mean
/// the same thing — nothing runs — and `Default` already gives every list an
/// empty `Vec`, the same choice `Expect::default()` makes for the same
/// reason. See ADR-027.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Commands {
    /// Run before anything is rendered, in an empty sandbox.
    pub before: CommandList,
    /// Run after the rendering is materialised onto the sandbox, before
    /// `expect` is checked.
    pub rendered: CommandList,
    /// Run after `expect` and the snapshot are checked.
    pub after: CommandList,
    /// Run always, last, regardless of anything above. Best-effort: every
    /// entry runs even when an earlier one in this same list failed.
    pub finally: CommandList,
}

/// One `[commands]` list: the commands themselves, and the environment
/// merged into every one of them.
///
/// Split out of `Commands` rather than kept as a bare `Vec<String>` so a list
/// can carry its own `env` override on top of `commands.env`, scoped to
/// itself alone. See "`env` scopes a command's environment" in ADR-027.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommandList {
    /// The commands themselves, in order.
    pub commands: Vec<String>,
    /// `commands.env`, merged with this list's own override if it wrote one
    /// — the value already resolved at parse time, so nothing downstream
    /// needs to know two tables were ever involved.
    pub env: BTreeMap<String, String>,
}

impl Commands {
    /// Whether every list is empty — the case declared no `[commands]`, or
    /// wrote one with nothing in it.
    pub fn is_empty(&self) -> bool {
        self.before.commands.is_empty()
            && self.rendered.commands.is_empty()
            && self.after.commands.is_empty()
            && self.finally.commands.is_empty()
    }

    fn parse(
        table: &BTreeMap<String, Value>,
        shape: &impl Fn(String) -> TestingError,
    ) -> Result<Self, TestingError> {
        for key in table.keys() {
            if !matches!(
                key.as_str(),
                "before" | "rendered" | "after" | "finally" | "env"
            ) {
                let hint = closest(key, ["before", "rendered", "after", "finally", "env"])
                    .map(|near| format!(" Did you mean `{near}`?"))
                    .unwrap_or_default();
                return Err(shape(format!(
                    "`commands.{key}` is not a command list.{hint} \
                     A case may run `before`, `rendered`, `after` or `finally`, \
                     and set `env` for all of them."
                )));
            }
        }
        // Merged into every list below, before that list's own override (if
        // any) is applied on top — see `CommandList::parse`.
        let env = string_map(table.get("env"), "commands.env", shape)?;
        Ok(Commands {
            before: CommandList::parse(table.get("before"), "commands.before", &env, shape)?,
            rendered: CommandList::parse(table.get("rendered"), "commands.rendered", &env, shape)?,
            after: CommandList::parse(table.get("after"), "commands.after", &env, shape)?,
            finally: CommandList::parse(table.get("finally"), "commands.finally", &env, shape)?,
        })
    }
}

impl CommandList {
    /// Parse one list: the existing bare array (no override, just
    /// `inherited`), or a table with its own `run` and `env`.
    ///
    /// `inherited` is `commands.env`, already parsed once by the caller
    /// rather than re-parsed per list — a case with four lists and a typo in
    /// `commands.env` should get one diagnostic, not four identical ones.
    fn parse(
        value: Option<&Value>,
        key: &str,
        inherited: &BTreeMap<String, String>,
        shape: &impl Fn(String) -> TestingError,
    ) -> Result<Self, TestingError> {
        match value {
            None | Some(Value::Array(_)) => Ok(CommandList {
                commands: string_array(value, key, shape)?,
                env: inherited.clone(),
            }),
            Some(Value::Table(inner)) => {
                for inner_key in inner.keys() {
                    if !matches!(inner_key.as_str(), "run" | "env") {
                        let hint = closest(inner_key, ["run", "env"])
                            .map(|near| format!(" Did you mean `{near}`?"))
                            .unwrap_or_default();
                        return Err(shape(format!(
                            "`{key}.{inner_key}` is not a command list key.{hint} \
                             A list written as a table has `run` and `env`."
                        )));
                    }
                }
                // The list's own value wins over `commands.env` for the same
                // key: the more specific table is the one the case's author
                // was looking at when they wrote it.
                let mut env = inherited.clone();
                env.extend(string_map(inner.get("env"), &format!("{key}.env"), shape)?);
                Ok(CommandList {
                    commands: string_array(inner.get("run"), &format!("{key}.run"), shape)?,
                    env,
                })
            }
            Some(other) => Err(shape(format!(
                "`{key}` must be an array of commands, or a table with `run` and `env`, not {}.",
                other.type_name()
            ))),
        }
    }
}

/// A case's isolated Git sandbox: the identity its one seed commit — and any
/// later commit a case's own `[commands]` makes inside it — is authored
/// with. See ADR-033.
#[derive(Debug, Clone, PartialEq)]
pub struct GitSandbox {
    /// `user.name`, written to the sandbox's local `.git/config`.
    pub user_name: String,
    /// `user.email`, alongside `user_name`.
    pub user_email: String,
}

impl GitSandbox {
    /// The identity used when a case writes `git = true` rather than
    /// overriding it. Synthetic and `.invalid` (RFC 2606), so nobody mistakes
    /// it for a real address — the same convention
    /// `tests/common/mod.rs::Repo::configure` already uses for every
    /// repository this project's own test suite builds.
    fn default_identity() -> Self {
        GitSandbox {
            user_name: "git-tpl test".to_string(),
            user_email: "test@git-tpl.invalid".to_string(),
        }
    }

    /// Parse the table form: `[git]` with an optional `user` sub-table.
    /// `user.name`/`user.email` nest that way because they are TOML dotted
    /// keys under `[git]`, not a single `git.user.name` string.
    fn parse(
        table: &BTreeMap<String, Value>,
        shape: &impl Fn(String) -> TestingError,
    ) -> Result<Self, TestingError> {
        for key in table.keys() {
            if key != "user" {
                let hint = closest(key, ["user"])
                    .map(|near| format!(" Did you mean `{near}`?"))
                    .unwrap_or_default();
                return Err(shape(format!(
                    "`git.{key}` is not a git sandbox key.{hint} \
                     A case may override `git.user.name` and `git.user.email`; \
                     everything else uses the built-in default identity."
                )));
            }
        }

        let defaults = Self::default_identity();
        let (user_name, user_email) = match table.get("user") {
            None => (defaults.user_name, defaults.user_email),
            Some(Value::Table(user)) => {
                for key in user.keys() {
                    if !matches!(key.as_str(), "name" | "email") {
                        let hint = closest(key, ["name", "email"])
                            .map(|near| format!(" Did you mean `{near}`?"))
                            .unwrap_or_default();
                        return Err(shape(format!(
                            "`git.user.{key}` is not a git sandbox key.{hint} \
                             A case may set `git.user.name` and `git.user.email`."
                        )));
                    }
                }
                let name = match user.get("name") {
                    None => defaults.user_name,
                    Some(Value::String(name)) => name.clone(),
                    Some(other) => {
                        return Err(shape(format!(
                            "`git.user.name` must be a string, not {}.",
                            other.type_name()
                        )));
                    }
                };
                let email = match user.get("email") {
                    None => defaults.user_email,
                    Some(Value::String(email)) => email.clone(),
                    Some(other) => {
                        return Err(shape(format!(
                            "`git.user.email` must be a string, not {}.",
                            other.type_name()
                        )));
                    }
                };
                (name, email)
            }
            Some(other) => {
                return Err(shape(format!(
                    "`git.user` must be a table, not {}.",
                    other.type_name()
                )));
            }
        };

        Ok(GitSandbox {
            user_name,
            user_email,
        })
    }
}

/// Which `[commands]` list a [`Failure::CommandFailed`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStep {
    /// `commands.before`.
    Before,
    /// `commands.rendered`.
    Rendered,
    /// `commands.after`.
    After,
    /// `commands.finally`.
    Finally,
}

impl CommandStep {
    /// The machine-readable name, matching the case file's own key.
    pub fn as_str(self) -> &'static str {
        match self {
            CommandStep::Before => "before",
            CommandStep::Rendered => "rendered",
            CommandStep::After => "after",
            CommandStep::Finally => "finally",
        }
    }
}

/// Which stream a live chunk of a running command's output came from.
#[derive(Debug, Clone, Copy)]
pub enum Stream {
    /// The command's own stdout.
    Stdout,
    /// The command's own stderr.
    Stderr,
}

/// What phase a case is in, for a caller reporting progress live.
pub enum Status<'a> {
    /// Rendering, before anything in `[commands]` (if any) has been checked.
    Rendering,
    /// One `[commands]` entry is about to run.
    Command {
        /// Which list it came from.
        step: CommandStep,
        /// The command line itself, unparsed.
        command: &'a str,
    },
    /// Comparing (or writing) the case's recorded snapshot.
    Snapshot,
}

/// What the harness may report about a running case, live — never anything
/// that affects the run itself, only what a caller may want to show on a
/// terminal while it happens.
///
/// `testing.rs` must not know a terminal exists (nothing below `ops` does —
/// see the layering doc at the top of `src/lib.rs`), so this is an observer a
/// caller implements, the same pattern [`crate::eval::Prompter`] and
/// [`crate::data::TrustGate`] already use. `commands::test` is the only
/// implementation today; every method defaults to nothing, so a caller with
/// nothing to say — every integration test, any future library consumer —
/// pays nothing for it.
pub trait Progress {
    /// A case is about to run.
    fn case_started(&mut self, _name: &str) {}
    /// The case entered a new phase.
    fn case_status(&mut self, _name: &str, _status: Status<'_>) {}
    /// A `[commands]` entry finished; `ok` says whether it succeeded.
    ///
    /// Separate from [`case_status`](Self::case_status), which fires *before*
    /// a command runs and so cannot say how it turned out — a caller that
    /// wants to say "this one failed" needs this second event once the
    /// answer exists.
    fn command_finished(&mut self, _name: &str, _step: CommandStep, _command: &str, _ok: bool) {}
    /// A chunk of a running command's own stdout/stderr, as it is produced.
    ///
    /// Raw bytes, not lossily converted: a caller forwarding them to a
    /// terminal must see exactly what the command wrote, ANSI escapes
    /// included, not the `String::from_utf8_lossy` the final report already
    /// builds for its own display.
    fn command_output(&mut self, _name: &str, _stream: Stream, _chunk: &[u8]) {}
    /// The case is done.
    fn case_finished(&mut self, _outcome: &CaseOutcome) {}
}

/// Says nothing. The default for a caller with no terminal to update.
pub struct Silent;
impl Progress for Silent {}

impl Case {
    /// Parse a case file.
    pub fn parse(name: &str, path: &str, bytes: &[u8]) -> Result<Self, TestingError> {
        let format = Format::infer(path);
        let value = parse_value(format, bytes).map_err(|reason| TestingError::CaseParse {
            path: path.to_string(),
            format: format!("{format:?}").to_lowercase(),
            reason,
        })?;

        let shape = |reason: String| TestingError::CaseShape {
            path: path.to_string(),
            reason,
        };

        let Value::Table(table) = value else {
            return Err(shape(
                "a test case is a table with an `[answers]` and an `[expect]` section.".to_string(),
            ));
        };

        // Strict about unknown keys, unlike `--answers-from`. An answers file
        // may legitimately carry keys from another generator; a case file is
        // written for this one purpose, and a typo'd `[expects]` would be a
        // test that passes forever having asserted nothing.
        for key in table.keys() {
            if !matches!(
                key.as_str(),
                "answers" | "trust" | "expect" | "commands" | "snapshot" | "git"
            ) {
                let hint = closest(
                    key,
                    ["answers", "trust", "expect", "commands", "snapshot", "git"],
                )
                .map(|near| format!(" Did you mean `{near}`?"))
                .unwrap_or_default();
                return Err(shape(format!(
                    "`{key}` is not a test case key.{hint} \
                     A case has `answers`, `trust`, `expect`, `commands`, `snapshot` and `git`."
                )));
            }
        }

        let answers = match table.get("answers") {
            None => BTreeMap::new(),
            Some(Value::Table(answers)) => answers.clone(),
            Some(other) => {
                return Err(shape(format!(
                    "`answers` must be a table of answers, not {}.",
                    other.type_name()
                )));
            }
        };

        // `true` on omission, unlike `snapshot`: a case renders for real
        // unless it deliberately says otherwise. See ADR-028.
        let trust = match table.get("trust") {
            None => true,
            Some(Value::Bool(trust)) => *trust,
            Some(other) => {
                return Err(shape(format!(
                    "`trust` must be `true` or `false`, not {}.",
                    other.type_name()
                )));
            }
        };

        let expect = match table.get("expect") {
            None => Expect::default(),
            Some(Value::Table(expect)) => Expect::parse(expect, &shape)?,
            Some(other) => {
                return Err(shape(format!(
                    "`expect` must be a table, not {}.",
                    other.type_name()
                )));
            }
        };

        let commands = match table.get("commands") {
            None => Commands::default(),
            Some(Value::Table(commands)) => Commands::parse(commands, &shape)?,
            Some(other) => {
                return Err(shape(format!(
                    "`commands` must be a table, not {}.",
                    other.type_name()
                )));
            }
        };

        let snapshot = match table.get("snapshot") {
            None => false,
            Some(Value::Bool(snapshot)) => *snapshot,
            Some(other) => {
                return Err(shape(format!(
                    "`snapshot` must be `true` or `false`, not {}.",
                    other.type_name()
                )));
            }
        };

        // `None` on omission, same as `snapshot`: an isolated sandbox is an
        // extra assertion a case opts into, not a side effect of anything
        // else it wrote. See ADR-033.
        let git = match table.get("git") {
            None | Some(Value::Bool(false)) => None,
            Some(Value::Bool(true)) => Some(GitSandbox::default_identity()),
            Some(Value::Table(inner)) => Some(GitSandbox::parse(inner, &shape)?),
            Some(other) => {
                return Err(shape(format!(
                    "`git` must be `true`, `false`, or a table overriding \
                     `user.name`/`user.email`, not {}.",
                    other.type_name()
                )));
            }
        };

        // A case expecting an error has no rendering for `commands.rendered`
        // or `commands.after` to run against — the same vacuous-assertion
        // refusal `Expect::parse` already applies to `files`/`absent`/
        // `contains`/`lacks`, extended to the two lists that need a rendering
        // to run against at all.
        if expect.error.is_some()
            && (!commands.rendered.commands.is_empty() || !commands.after.commands.is_empty())
        {
            return Err(shape(
                "`expect.error` says the render fails, so there is nothing for \
                 `commands.rendered` or `commands.after` to run against. \
                 Move them to `commands.before` or `commands.finally`, or split into two cases."
                    .to_string(),
            ));
        }

        // Likewise, a snapshot of a rendering that is never expected to exist
        // is not a coherent request.
        if expect.error.is_some() && snapshot {
            return Err(shape(
                "`expect.error` says the render fails, so there is nothing for \
                 `snapshot` to record or compare. Split into two cases."
                    .to_string(),
            ));
        }

        Ok(Case {
            name: name.to_string(),
            path: path.to_string(),
            answers,
            trust,
            expect,
            commands,
            git,
            snapshot,
        })
    }
}

impl Expect {
    fn parse(
        table: &BTreeMap<String, Value>,
        shape: &impl Fn(String) -> TestingError,
    ) -> Result<Self, TestingError> {
        for key in table.keys() {
            if !matches!(
                key.as_str(),
                "files" | "absent" | "contains" | "lacks" | "error"
            ) {
                let hint = closest(key, ["files", "absent", "contains", "lacks", "error"])
                    .map(|near| format!(" Did you mean `{near}`?"))
                    .unwrap_or_default();
                return Err(shape(format!(
                    "`expect.{key}` is not an expectation.{hint} \
                     A case may expect `files`, `absent`, `contains`, `lacks` or `error`."
                )));
            }
        }

        let files = string_array(table.get("files"), "expect.files", shape)?;
        let absent = string_array(table.get("absent"), "expect.absent", shape)?;
        let contains = substring_map(table, "contains", "expected text", shape)?;
        let lacks = substring_map(table, "lacks", "forbidden text", shape)?;

        let error = match table.get("error") {
            None => None,
            Some(Value::String(code)) => {
                // A misspelt code would make a failure case pass whenever *any*
                // error occurred. Validating the shape catches half of that
                // here; the other half is that the code must actually appear in
                // the failure, which the runner checks.
                let segments: Vec<&str> = code.split("::").collect();
                if segments.len() != 3 || segments.iter().any(|s| s.is_empty()) {
                    return Err(shape(format!(
                        "`expect.error` must be a diagnostic code of the form \
                         `tpl::<area>::<kind>`, not `{code}`. \
                         See docs/reference/diagnostics.md for the catalogue."
                    )));
                }
                Some(code.clone())
            }
            Some(other) => {
                return Err(shape(format!(
                    "`expect.error` must be a diagnostic code, not {}.",
                    other.type_name()
                )));
            }
        };

        // A case that expects an error has no rendering to assert about, so a
        // case asking for both is a mistake rather than a combination.
        if error.is_some()
            && (!files.is_empty()
                || !absent.is_empty()
                || !contains.is_empty()
                || !lacks.is_empty())
        {
            return Err(shape(
                "`expect.error` says the render fails, so there is no rendering for \
                 `files`, `absent`, `contains` or `lacks` to describe. Split them into two cases."
                    .to_string(),
            ));
        }

        Ok(Expect {
            files,
            absent,
            contains,
            lacks,
            error,
        })
    }
}

/// Parse a `path -> substring(s)` table, shared by `contains` and `lacks`.
///
/// Both take the same shape and the same bare-string-or-array coercion, so a
/// second copy of that logic would only be a place for the two to drift.
fn substring_map(
    table: &BTreeMap<String, Value>,
    key: &str,
    noun: &str,
    shape: &impl Fn(String) -> TestingError,
) -> Result<BTreeMap<String, Vec<String>>, TestingError> {
    match table.get(key) {
        None => Ok(BTreeMap::new()),
        Some(Value::Table(entries)) => {
            let mut out = BTreeMap::new();
            for (path, value) in entries {
                // A bare string as well as an array, because
                // `"a.toml" = 'name = "x"'` is what people write and
                // refusing it would teach nothing.
                let needles = match value {
                    Value::String(needle) => vec![needle.clone()],
                    other => string_array(Some(other), &format!("expect.{key}.\"{path}\""), shape)?,
                };
                out.insert(path.clone(), needles);
            }
            Ok(out)
        }
        Some(other) => Err(shape(format!(
            "`expect.{key}` must be a table of path to {noun}, not {}.",
            other.type_name()
        ))),
    }
}

fn string_array(
    value: Option<&Value>,
    what: &str,
    shape: &impl Fn(String) -> TestingError,
) -> Result<Vec<String>, TestingError> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(s) => Ok(s.clone()),
                other => Err(shape(format!(
                    "`{what}` must contain strings, not {}.",
                    other.type_name()
                ))),
            })
            .collect(),
        Some(other) => Err(shape(format!(
            "`{what}` must be an array of strings, not {}.",
            other.type_name()
        ))),
    }
}

/// Parse a `name -> value` table of plain strings — the shape `commands.env`
/// and `commands.<list>.env` share.
///
/// Unlike `string_array`'s callers, a bare-string coercion would not help
/// here: an environment variable's value already is one, so there is nothing
/// to coerce from.
fn string_map(
    value: Option<&Value>,
    what: &str,
    shape: &impl Fn(String) -> TestingError,
) -> Result<BTreeMap<String, String>, TestingError> {
    match value {
        None => Ok(BTreeMap::new()),
        Some(Value::Table(entries)) => entries
            .iter()
            .map(|(name, value)| match value {
                Value::String(value) => Ok((name.clone(), value.clone())),
                other => Err(shape(format!(
                    "`{what}.{name}` must be a string, not {}.",
                    other.type_name()
                ))),
            })
            .collect(),
        Some(other) => Err(shape(format!(
            "`{what}` must be a table of strings, not {}.",
            other.type_name()
        ))),
    }
}

/// One unmet expectation.
///
/// Data rather than an error, so that every failing case in a run is reported
/// and the author fixes them together rather than one per invocation.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// `expect.files` named a path the rendering does not have.
    MissingFile {
        /// The path that is missing.
        path: String,
        /// A rendered path close enough to be the intended one.
        closest: Option<String>,
    },
    /// `expect.absent` named a path the rendering does have.
    UnexpectedFile {
        /// The path that should not be there.
        path: String,
    },
    /// `expect.contains` named a path the rendering does not have.
    ContainsMissingFile {
        /// The path that is missing.
        path: String,
    },
    /// A substring is not in the file.
    ContainsMissing {
        /// The file that was searched.
        path: String,
        /// The text that is not in it.
        needle: String,
    },
    /// `expect.contains` named a file that is not text.
    ContainsNotUtf8 {
        /// The file that is not text.
        path: String,
    },
    /// `expect.lacks` named a path the rendering does not have.
    ///
    /// A missing path is a failure rather than a vacuous pass: "this file does
    /// not mention X" must not go green because the file stopped rendering
    /// entirely.
    LacksMissingFile {
        /// The path that is missing.
        path: String,
    },
    /// A forbidden substring is in the file.
    LacksPresent {
        /// The file that was searched.
        path: String,
        /// The text that must not be in it but is.
        needle: String,
    },
    /// `expect.lacks` named a file that is not text.
    LacksNotUtf8 {
        /// The file that is not text.
        path: String,
    },
    /// `expect.error` was set and the render succeeded.
    ExpectedError {
        /// The code that was expected.
        code: String,
    },
    /// The render failed and no `expect.error` said it should.
    UnexpectedError {
        /// The outermost diagnostic code, if the failure carried one.
        code: Option<String>,
        /// The rendered message.
        message: String,
    },
    /// The render failed with a code the case did not name.
    WrongError {
        /// The code the case expected.
        expected: String,
        /// Every code in the failure and its causes, outermost first.
        actual: Vec<String>,
        /// The rendered message.
        message: String,
    },
    /// A snapshot exists and the rendering differs from it.
    SnapshotDiff {
        /// The differing paths.
        changes: Vec<SnapshotChange>,
    },
    /// `snapshot = true`, but nothing has ever been recorded for this case.
    ///
    /// Snapshots are opt-in, but not silently so: a case that asks for one
    /// and never gets it would report false confidence, exactly the gap
    /// explicit opt-in is meant to close. `--write` records one.
    SnapshotMissing,
    /// A command declared in `[commands]` exited nonzero, or could not be run
    /// at all.
    ///
    /// One variant for both: a program that does not exist and a program
    /// that ran and failed are the same fact from the case's point of view —
    /// this list did not do what the author said it would. See ADR-027.
    CommandFailed {
        /// Which list the command came from.
        step: CommandStep,
        /// The command exactly as written in the case file.
        command: String,
        /// The exit code, or `None` if the process could not be spawned at
        /// all (no such program, not executable) or was killed by a signal.
        code: Option<i32>,
        /// Captured stdout, capped at [`COMMAND_OUTPUT_LIMIT_BYTES`], tail
        /// kept — a failing build prints progress before its error.
        stdout: String,
        /// Captured stderr, capped the same way, or — when `code` is `None`
        /// — the operating system's reason the process never ran at all.
        stderr: String,
    },
}

/// How a rendered path differs from the recorded snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotChange {
    /// The rendered path.
    pub path: String,
    /// What happened to it.
    pub kind: ChangeKind,
    /// Set when the bytes are identical and only the executable bit moved.
    pub mode_only: bool,
    /// A unified diff, when both sides are text and this is a modification.
    pub patch: Option<String>,
}

/// What the snapshot step did for a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOutcome {
    /// The case did not ask for a snapshot (`snapshot` is `false` or absent)
    /// on a plain run, or asked for one but its render failed before there
    /// was anything to record or compare.
    ///
    /// Not a failure on its own — snapshots are opt-in per case, so a
    /// template with three cases and one `snapshot = true` is a normal state
    /// — but the "asked for one, none recorded, not `--write`" case also
    /// carries a [`Failure::SnapshotMissing`] alongside this outcome.
    None,
    /// `--write` did nothing for this case: it does not declare
    /// `snapshot = true`, so it was never rendered, never checked, and never
    /// counted as run. See ADR-032.
    Skipped,
    /// Compared against a recorded snapshot.
    Compared,
    /// `--write` created it.
    Written,
    /// `--write` replaced it.
    Updated,
    /// `--write` found it already correct.
    Unchanged,
}

impl SnapshotOutcome {
    /// The machine-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            SnapshotOutcome::None => "none",
            SnapshotOutcome::Skipped => "skipped",
            SnapshotOutcome::Compared => "compared",
            SnapshotOutcome::Written => "written",
            SnapshotOutcome::Updated => "updated",
            SnapshotOutcome::Unchanged => "unchanged",
        }
    }
}

/// What one case did.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseOutcome {
    /// The case name.
    pub name: String,
    /// The file it was read from.
    pub path: String,
    /// Empty when the case passed.
    pub failures: Vec<Failure>,
    /// What the snapshot step did.
    pub snapshot: SnapshotOutcome,
    /// How many files the rendering produced. Zero for a case that expected a
    /// failure and got one.
    pub files: usize,
    /// How many `[commands]` entries actually ran, across all four lists.
    /// Zero for a case with no `[commands]`, zero when commands were
    /// disabled for the run, and always zero under `--write` (ADR-032: a
    /// case's `[commands]` never run while `--write` is recording it).
    pub commands_run: usize,
    /// What `--write` changed against a previously recorded snapshot,
    /// non-empty only when `snapshot` is [`SnapshotOutcome::Updated`].
    ///
    /// This is the data a verbose `--write` prints in place of the live
    /// `[commands]` output it no longer produces, since nothing runs to
    /// stream. See ADR-032.
    pub snapshot_changes: Vec<SnapshotChange>,
    /// Wall-clock time `run` spent on this case, in whole milliseconds.
    ///
    /// Always measured, not gated behind `--record-durations`: two
    /// `Instant::now()` calls cost nothing, and a plain run's own report
    /// becomes more useful for free. Milliseconds, not `Duration`: this is a
    /// plain report field serialised straight to JSON, and sub-millisecond
    /// precision means nothing once a case has spawned a process. See
    /// ADR-036.
    pub duration_ms: u64,
}

impl CaseOutcome {
    /// Whether every expectation was met.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The `--shard` facts about a run. `None` on a plain run. See ADR-036.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardInfo {
    /// The 1-based index that was asked for.
    pub index: usize,
    /// The 1-based total that was asked for.
    pub total: usize,
    /// How many cases existed before this shard narrowed them down —
    /// `discover`'s count, after the positional filter, before `--shard`.
    pub cases_total: usize,
    /// Whether a recorded durations file existed and was used to balance
    /// the split; `false` means the deterministic even-by-count fallback
    /// fired instead.
    pub balanced: bool,
}

/// The result of a whole run.
pub struct Report {
    /// The template, resolved once for every case.
    pub template: Resolved,
    /// The tests directory that was read, relative to the template root.
    pub tests_dir: String,
    /// One per case, in name order.
    pub cases: Vec<CaseOutcome>,
    /// Whether `[commands]` ran at all for this run — `false` when
    /// `--skip-commands` or `tpl.testCommands = false` disabled them.
    pub commands_enabled: bool,
    /// `--shard` was given, and this is how it split the suite.
    pub shard: Option<ShardInfo>,
    /// Whether this run measured for `--record-durations` — distinct from
    /// [`CaseOutcome::duration_ms`], which is populated regardless: this
    /// says whether the durations *file* was written at the end.
    pub durations_recorded: bool,
}

impl Report {
    /// How many cases met every expectation.
    pub fn passed(&self) -> usize {
        self.cases.iter().filter(|case| case.passed()).count()
    }

    /// How many cases did not.
    pub fn failed(&self) -> usize {
        self.cases.len() - self.passed()
    }

    /// Whether the run should fail the command.
    pub fn is_failure(&self) -> bool {
        self.failed() > 0
    }

    /// How many snapshots `--write` created or replaced.
    pub fn snapshots_written(&self) -> usize {
        self.cases
            .iter()
            .filter(|case| {
                matches!(
                    case.snapshot,
                    SnapshotOutcome::Written | SnapshotOutcome::Updated
                )
            })
            .count()
    }

    /// How many snapshots were compared against.
    pub fn snapshots_compared(&self) -> usize {
        self.cases
            .iter()
            .filter(|case| case.snapshot == SnapshotOutcome::Compared)
            .count()
    }

    /// How many `[commands]` entries ran, across every case.
    pub fn commands_run(&self) -> usize {
        self.cases.iter().map(|case| case.commands_run).sum()
    }
}

/// Everything about *how* a run behaves — as opposed to [`Target`] (*what*
/// is under test) and `&mut dyn Progress` (*how it is reported*).
///
/// Grouped for the reason [`Target`] already was: `run` was about to carry
/// five positional bools/`Option`s where it had three, and two adjacent
/// `bool`s transposed at a call site is a bug the compiler cannot catch — a
/// named field cannot be transposed silently.
pub struct RunOptions<'a> {
    /// Read cases from this directory instead of [`DEFAULT_TESTS_DIR`].
    pub tests_dir: Option<&'a str>,
    /// Run only these case names.
    pub filter: &'a [String],
    /// Record each case's rendering as its snapshot; see ADR-032.
    pub write: bool,
    /// Whether `[commands]` (and `git`, ADR-033) run at all this run.
    pub run_commands: bool,
    /// Told to `[commands]` children via `CLICOLOR_FORCE`/`FORCE_COLOR`.
    ///
    /// Sourced from the caller's own colour decision (`Theme::is_colored`),
    /// never decided here: `ops` has no terminal to ask.
    pub color: bool,
    /// `--shard`'s raw `INDEX/TOTAL`, unparsed — parsed inside `run`, not by
    /// the caller, so a malformed value is a [`TestingError`] like every
    /// other case-shape mistake. See ADR-036.
    pub shard: Option<&'a str>,
    /// Time every case and record it to the durations file at the end.
    ///
    /// Conflicts with `shard` and `write` at the CLI layer (`src/cli.rs`),
    /// enforced there, not re-checked here. See ADR-036.
    pub record_durations: bool,
}

/// Run a template's test cases.
///
/// The template is resolved **once**, so a report saying "12 cases at abc1234"
/// is telling the truth even if `HEAD` moved mid-run.
pub fn run(
    target: Target<'_>,
    options: RunOptions<'_>,
    user: &UserConfig,
    progress: &mut dyn Progress,
) -> Result<Report, OpError> {
    // Checked before anything is resolved, and unconditionally — not only for
    // `--write` — so a remote `TEMPLATE` fails the same way whether or not
    // `--ref` or `--write` is also given, rather than reaching a temporary
    // clone under some flag combinations and not others.
    if resolve::local_path(target.source).is_none() {
        return Err(TestingError::RemoteNotSupported {
            origin: target.source.to_string(),
        }
        .into());
    }

    // The leaf is always local (checked above), but an `[extends]` ancestor
    // can still be remote. This resolution happens once, before any case is
    // even read, so it cannot follow a case's own `trust` field the way
    // declared `[data]` sources do (ADR-028) — an ancestor is confirmed
    // against `[trust]` alone, the same as `lint`/`questions`/`status`, none
    // of which have a `--trust` of `test`'s own to fall back on either.
    let mut trust = Trust::refuse();
    let template = resolve::resolve(
        super::Request {
            source: target.source,
            reference: target.reference,
            root: target.root,
            dirty: target.dirty,
        },
        user,
        &mut trust,
    )?;

    let tests_dir = options
        .tests_dir
        .unwrap_or(DEFAULT_TESTS_DIR)
        .trim_end_matches('/');
    let cases = discover(&template, tests_dir, options.filter)?;
    let cases_total = cases.len();

    // Read once, before the loop, regardless of which of `--shard`/
    // `--record-durations` is in play — the two are mutually exclusive at
    // the CLI layer, so this is never a wasted read in practice, but keeping
    // the two features' code paths independent here avoids coupling them.
    let previous_durations = if options.shard.is_some() || options.record_durations {
        read_durations(&template, tests_dir)?
    } else {
        None
    };

    let (cases, shard) = match options.shard {
        None => (cases, None),
        Some(spec) => {
            let (index, total) = parse_shard(spec)?;
            let (selected, balanced) =
                shard_cases(cases, index, total, previous_durations.as_ref());
            if selected.is_empty() {
                return Err(TestingError::EmptyShard {
                    index,
                    total,
                    available: cases_total,
                }
                .into());
            }
            (
                selected,
                Some(ShardInfo {
                    index,
                    total,
                    cases_total,
                    balanced,
                }),
            )
        }
    };

    // Nobody is asked. A case decides for itself, in the file, via its own
    // `trust` field — the same authority ADR-027 already gives a case's
    // `[commands]`, extended by ADR-028 to a template's declared remote data
    // sources. Both outcomes are built once, up front, and every case simply
    // picks between them: "allow everything declared" or "skip everything
    // declared", never a mix, because `declared_remotes` is read from the
    // manifest once and a case's `trust` is all-or-nothing over that set.
    let requests = declared_remotes(&template.manifest.data);
    let trusted: BTreeMap<String, Decision> = requests
        .iter()
        .map(|request| (request.name.clone(), Decision::Allow))
        .collect();
    let untrusted: BTreeMap<String, Decision> = requests
        .iter()
        .map(|request| (request.name.clone(), Decision::Skip))
        .collect();

    // The persistent `[trust]` list (ADR-013) exists for `init`/`update`,
    // which act on a real project one person owns. A case's `trust` has to
    // mean the same thing on every machine that runs it, so it is decided
    // without ever consulting that list — a config file on the machine
    // running the suite must not be able to turn a `trust = false` case into
    // a pass, or a `trust = true` case into a surprise on someone else's.
    let user = &UserConfig {
        trust: crate::userconfig::Trust::default(),
        ..user.clone()
    };

    let mut outcomes = Vec::with_capacity(cases.len());
    for case in &cases {
        let decisions = if case.trust { &trusted } else { &untrusted };
        progress.case_started(&case.name);
        let started = std::time::Instant::now();
        let mut outcome = run_case(
            &template,
            target.source,
            tests_dir,
            case,
            options.write,
            options.run_commands,
            user,
            decisions,
            options.color,
            progress,
        )?;
        // `u128` → `u64`: a case running longer than 584 million years is
        // not a truncation bug worth a `Result` for.
        outcome.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        progress.case_finished(&outcome);
        outcomes.push(outcome);
    }

    let durations_recorded = options.record_durations;
    if durations_recorded {
        let measured: BTreeMap<String, u64> = outcomes
            .iter()
            .map(|outcome| (outcome.name.clone(), outcome.duration_ms))
            .collect();
        write_durations(&template, tests_dir, previous_durations.as_ref(), &measured)?;
    }

    Ok(Report {
        template,
        tests_dir: tests_dir.to_string(),
        cases: outcomes,
        commands_enabled: options.run_commands,
        shard,
        durations_recorded,
    })
}

/// Read the cases out of the resolved tree.
///
/// From the tree and not the filesystem, so `--ref v1` runs *that tag's* cases
/// and the implicit no-`--ref` default runs the uncommitted ones — the same
/// meaning a resolved "dirty" tree has everywhere else in the tool. No special
/// case is needed here: `tree_from_workdir` has already built a synthetic tree
/// of the working directory before `discover` ever runs.
fn discover(template: &Resolved, tests_dir: &str, filter: &[String]) -> Result<Vec<Case>, OpError> {
    // `Resolved.tree` is the repository root, not `root_tree`: the tests
    // directory is outside the render root, exactly like the manifest and the
    // partials.
    let Some(dir) = template.repo.subtree(template.tree, tests_dir)? else {
        return Err(TestingError::NoTests {
            dir: tests_dir.to_string(),
            revision_description: super::describe_revision(&template.reference, template.revision),
        }
        .into());
    };

    let mut cases: Vec<Case> = Vec::new();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();

    for entry in template.repo.list_tree(dir)? {
        // Top level only. `list_tree` is recursive, so without this every file
        // under `__snapshots__/` would arrive as a candidate case — and a
        // `tests/fixtures/` of scratch files is not a suite.
        if entry.path.contains('/') {
            continue;
        }
        if entry.mode == FileMode::Link {
            continue;
        }
        // An allow-list, because we are globbing rather than being handed a
        // path: `Format::infer` defaults an unknown extension to TOML, which is
        // right for `--answers-from` and would turn a `tests/README.md` into a
        // broken case here.
        let Some((name, extension)) = entry.path.rsplit_once('.') else {
            continue;
        };
        if !matches!(
            extension.to_ascii_lowercase().as_str(),
            "toml" | "json" | "yaml" | "yml"
        ) {
            continue;
        }

        let path = format!("{tests_dir}/{}", entry.path);

        // Two files with one stem collide on one name and one snapshot
        // directory. Preferring either silently would be a case that never
        // runs.
        if let Some(previous) = seen.get(name) {
            return Err(TestingError::CaseShape {
                path: path.clone(),
                reason: format!(
                    "`{previous}` is also the case `{name}`. \
                     Two files cannot share a case name: they would share a snapshot."
                ),
            }
            .into());
        }
        seen.insert(name.to_string(), path.clone());

        let bytes = template.repo.read_blob(entry.oid)?;
        cases.push(Case::parse(name, &path, &bytes)?);
    }

    if cases.is_empty() {
        return Err(TestingError::NoTests {
            dir: tests_dir.to_string(),
            revision_description: super::describe_revision(&template.reference, template.revision),
        }
        .into());
    }

    // `list_tree` is Git-canonical order, which is already deterministic; sorted
    // by name so the report reads alphabetically whatever Git's byte order did
    // with the extensions.
    cases.sort_by(|a, b| a.name.cmp(&b.name));

    if filter.is_empty() {
        return Ok(cases);
    }

    let known: Vec<&str> = cases.iter().map(|case| case.name.as_str()).collect();
    for wanted in filter {
        if !known.contains(&wanted.as_str()) {
            return Err(TestingError::NoSuchCase {
                filter: wanted.clone(),
                suggestion: closest(wanted, known.iter().copied())
                    .map(|near| format!("Did you mean `{near}`?\n"))
                    .unwrap_or_default(),
                available: known.join(", "),
            }
            .into());
        }
    }

    Ok(cases
        .into_iter()
        .filter(|case| filter.contains(&case.name))
        .collect())
}

/// `(failures, snapshot outcome, snapshot changes, files rendered, commands
/// run)` — everything [`run_case`]'s sandboxed branch has to carry out of its
/// try/finally closure. Named so the closure's own type annotation reads as
/// one thing rather than a five-tuple clippy would otherwise flag.
type CaseRunOutcome = (
    Vec<Failure>,
    SnapshotOutcome,
    Vec<SnapshotChange>,
    usize,
    usize,
);

#[allow(clippy::too_many_arguments)]
fn run_case(
    template: &Resolved,
    source: &str,
    tests_dir: &str,
    case: &Case,
    write: bool,
    run_commands: bool,
    user: &UserConfig,
    decisions: &BTreeMap<String, Decision>,
    color: bool,
    progress: &mut dyn Progress,
) -> Result<CaseOutcome, OpError> {
    // `--write` has nothing to do for a case that never opted into a
    // snapshot: not rendered, not checked, not counted as run — `--write`'s
    // only job is writing snapshots, and this case has none to write. See
    // ADR-032.
    if write && !case.snapshot {
        return Ok(CaseOutcome {
            name: case.name.clone(),
            path: case.path.clone(),
            failures: Vec::new(),
            snapshot: SnapshotOutcome::Skipped,
            files: 0,
            commands_run: 0,
            snapshot_changes: Vec::new(),
            // Overwritten by the caller (`run`'s loop), which times the
            // whole `run_case` call uniformly across every dispatch path.
            duration_ms: 0,
        });
    }

    // No `[commands]` and no `git`, commands disabled for this run, or
    // `--write`: the exact pre-existing behaviour, unchanged, with no
    // sandbox created at all. `--write` never runs a case's `[commands]`
    // (ADR-032); by construction whenever `write` is true here,
    // `case.snapshot` is true (the check above already returned for the
    // case where it is not), so only its rendering needs to happen — never a
    // sandbox. `!run_commands` (`--skip-commands`/`tpl.testCommands`) skips
    // `git` too, a corollary rather than a separate toggle: it disables
    // everything the sandbox exists to serve, not only `[commands]` itself.
    // See ADR-033.
    if write || !run_commands || (case.commands.is_empty() && case.git.is_none()) {
        return run_case_plain(
            template, source, tests_dir, case, write, user, decisions, progress,
        );
    }

    let sandbox = tempfile::tempdir().map_err(|error| TestingError::SandboxFailed {
        case: case.name.clone(),
        reason: error.to_string(),
    })?;
    let cwd = sandbox.path();

    // Resolved once per case, not per command: every list below sees the
    // same value, and `--dirty`/a local `--ref` share the same working
    // directory here (`test` never resolves a remote — ADR-030), so there is
    // exactly one answer regardless of which one is running.
    let root = template.repo.workdir()?;

    // Seed the sandbox as an isolated Git repository before `before` runs,
    // if the case asked for one. A failure here is a fact about the machine
    // running the suite, not about the template — the same class of failure
    // `SandboxFailed` two lines above already reports with this code — so it
    // aborts the run rather than being recorded as one more case failure.
    // See ADR-033.
    if let Some(git) = &case.git {
        init_git_sandbox(cwd, git).map_err(|error| TestingError::SandboxFailed {
            case: case.name.clone(),
            reason: error.to_string(),
        })?;
    }

    // `GIT_CONFIG_NOSYSTEM`/`GIT_CONFIG_GLOBAL`, merged under every list's
    // own resolved `env` — never over it, so a case that deliberately sets
    // either key still wins, the same override precedent `TEMPLATE_ROOT` and
    // the colour variables already have. Empty, and so a no-op `extend`,
    // for a case that did not ask for `git`. See ADR-033.
    let git_isolation = case.git.as_ref().map(|_| git_isolation_env(cwd));
    let env_for = |list_env: &BTreeMap<String, String>| match &git_isolation {
        None => list_env.clone(),
        Some(isolation) => {
            let mut merged = isolation.clone();
            merged.extend(list_env.clone());
            merged
        }
    };

    // Rust has no try/finally. This closure is the substitute: everything
    // that can fail with a genuine `?` — a materialised write, a snapshot
    // read or write — runs inside it, so its *value* is captured here rather
    // than unwinding straight past the `finally` list below, which still has
    // to run against whatever state the sandbox is in.
    let outcome: Result<CaseRunOutcome, OpError> = (|| {
        let mut failures = Vec::new();
        let mut snapshot = SnapshotOutcome::None;
        let mut snapshot_changes = Vec::new();
        let mut files = 0;

        // `before`, into an empty sandbox: nothing has been rendered or
        // materialised yet.
        let mut commands_run = execute_commands(
            CommandStep::Before,
            &case.commands.before.commands,
            cwd,
            &root,
            &env_for(&case.commands.before.env),
            &case.answers,
            true,
            &mut failures,
            &case.name,
            color,
            progress,
        );
        let before_failed = failures.iter().any(|failure| {
            matches!(
                failure,
                Failure::CommandFailed {
                    step: CommandStep::Before,
                    ..
                }
            )
        });

        // A failed `before` means the precondition the case described was
        // never reached. Rendering onto it and checking `expect` against a
        // sandbox that never got set up correctly would assert about a state
        // that does not exist — skip straight to `finally`.
        if !before_failed {
            // Always defaults, never a prompt: there is nobody to ask in CI,
            // and a prompt in a test runner is a hang. A question the case
            // leaves unanswered with no default fails as
            // `tpl::eval::unanswered`, which is a true statement about the
            // template.
            progress.case_status(&case.name, Status::Rendering);
            let rendered = render_resolved(
                template,
                source,
                None,
                case.answers.clone(),
                user,
                Answering::defaults(),
                // Replaying this case's own `trust` field, decided in `run`
                // without asking anybody. See ADR-028.
                Trust::decided(decisions.clone()),
            );

            match rendered {
                Err(error) => {
                    // No merged tree exists either way: `commands.rendered`
                    // and `commands.after` have nothing to run against, for
                    // the same reason a `before` failure skips them.
                    classify_render_error(error, &case.expect.error, &mut failures);
                }
                Ok(RenderedOnce {
                    files: rendered_files,
                    ignored_answers,
                    ..
                }) => {
                    // Unconditional: a case has no `--strict-answers` to opt
                    // into and never will (`test`'s answers are the case
                    // file, not a recorded set that might outlive a dropped
                    // question) — see ADR-029 and #135. A violation is
                    // classified exactly like a render that failed outright:
                    // there is still nothing rendered for `expect` to check.
                    if let Err(error) = enforce_strict_answers(
                        true,
                        &ignored_answers,
                        template.manifest.questions.keys().cloned(),
                    ) {
                        classify_render_error(error, &case.expect.error, &mut failures);
                    } else {
                        files = rendered_files.len();

                        // Not `clear_directory`: that removes the directory
                        // first, erasing exactly what `before` just seeded.
                        // `materialise` alone only ever creates and
                        // overwrites the paths the rendering names, which is
                        // the render's tree laid on top of the sandbox's
                        // existing state — the whole point of seeding it.
                        let case_name = case.name.clone();
                        super::materialise(
                            cwd,
                            rendered_files.iter().map(|file| {
                                (file.path.as_str(), file.content.as_slice(), file.executable)
                            }),
                            &|path, verb, io_error| TestingError::SandboxWrite {
                                case: case_name.clone(),
                                path: path.display().to_string(),
                                reason: format!("could not {verb} it: {io_error}"),
                            },
                        )?;

                        // `rendered`: after materialising, before `expect`.
                        commands_run += execute_commands(
                            CommandStep::Rendered,
                            &case.commands.rendered.commands,
                            cwd,
                            &root,
                            &env_for(&case.commands.rendered.env),
                            &case.answers,
                            true,
                            &mut failures,
                            &case.name,
                            color,
                            progress,
                        );

                        if let Some(expected) = &case.expect.error {
                            failures.push(Failure::ExpectedError {
                                code: expected.clone(),
                            });
                        } else {
                            check(&case.expect, &rendered_files, &mut failures);
                        }
                        if case.snapshot {
                            progress.case_status(&case.name, Status::Snapshot);
                        }
                        // Always `write == false` here: the dispatcher above
                        // sends every `--write` run through `run_case_plain`
                        // instead (ADR-032), so this branch only ever
                        // compares. `snapshot_step` still returns the tuple
                        // shape unconditionally, so nothing here needs to
                        // know that.
                        (snapshot, snapshot_changes) = snapshot_step(
                            template,
                            tests_dir,
                            case,
                            &rendered_files,
                            write,
                            &mut failures,
                        )?;

                        // `after`: once `expect` and the snapshot are settled.
                        commands_run += execute_commands(
                            CommandStep::After,
                            &case.commands.after.commands,
                            cwd,
                            &root,
                            &env_for(&case.commands.after.env),
                            &case.answers,
                            true,
                            &mut failures,
                            &case.name,
                            color,
                            progress,
                        );
                    }
                }
            }
        }

        Ok((failures, snapshot, snapshot_changes, files, commands_run))
    })();

    // `finally` always runs, against whatever the sandbox holds, and every
    // entry in it runs even if an earlier one failed — cleanup left half done
    // because a step before it failed is a worse outcome than one more
    // reported failure. Note this runs *before* `outcome?` below: that
    // ordering is the whole mechanism, since nothing between the closure and
    // this point can skip it.
    let mut finally_failures = Vec::new();
    let finally_ran = execute_commands(
        CommandStep::Finally,
        &case.commands.finally.commands,
        cwd,
        &root,
        &env_for(&case.commands.finally.env),
        &case.answers,
        false,
        &mut finally_failures,
        &case.name,
        color,
        progress,
    );
    // `sandbox` is dropped when it goes out of scope below, deleting the
    // temporary directory.

    let (mut failures, snapshot, snapshot_changes, files, mut commands_run) = outcome?;
    failures.extend(finally_failures);
    commands_run += finally_ran;

    Ok(CaseOutcome {
        name: case.name.clone(),
        path: case.path.clone(),
        failures,
        snapshot,
        files,
        commands_run,
        snapshot_changes,
        // Overwritten by the caller; see the other two `CaseOutcome`
        // construction sites in this function.
        duration_ms: 0,
    })
}

/// Turn a case's freshly created sandbox into an isolated Git repository —
/// one empty commit, authored with the case's own identity (or the built-in
/// default) — never the ambient `user.name`/`user.email`/`commit.gpgsign` of
/// the machine running the suite.
///
/// Seeded entirely through [`GitBackend`], never a spawned `git` process, so
/// no hook, alias or ambient identity is ever consulted for this: the
/// identity is written to the sandbox's own local `.git/config` before
/// anything asks for a signature, and Git's own precedence — local always
/// outranks global and system — makes it authoritative regardless of what
/// either ambient level says. See ADR-033 and issue #155.
fn init_git_sandbox(cwd: &Path, git: &GitSandbox) -> Result<(), GitError> {
    let repo = LibGit2::init_isolated(cwd)?;
    repo.set_config_str("user.name", &git.user_name)?;
    repo.set_config_str("user.email", &git.user_email)?;
    // A literal `git commit` a case's own `[commands]` runs later would try
    // to GPG-sign under an ambient `commit.gpgsign = true` and hang waiting
    // for an agent with no TTY to answer — the exact failure issue #155
    // reported. Pinned locally so that cannot happen regardless of the
    // machine running the suite.
    repo.set_config_bool("commit.gpgsign", false)?;
    let empty_tree = repo.build_tree(&[])?;
    let seed = repo.create_commit(empty_tree, &[], "git tpl test sandbox")?;
    // The branch `init_isolated` pinned HEAD at, so it has a commit to point
    // at rather than staying unborn — the whole reason a case asks for this:
    // something like `pdm-backend`'s `source = "scm"` needs a commit to
    // describe from, not merely a `.git` directory.
    repo.set_ref("refs/heads/main", seed, "tpl: seed the isolated sandbox")?;
    Ok(())
}

/// Where `GIT_CONFIG_GLOBAL` points a case's own `[commands]` at, when it
/// asked for `git`: inside the sandbox's own `.git/`, never created, so real
/// Git treats the "global" level as empty rather than reading the running
/// machine's own `~/.gitconfig`. Mirrors `tests/common/mod.rs::scrub_git_env`,
/// written for the identical reason one layer further out — isolating this
/// project's own test suite from the developer's `~/.gitconfig`.
const GIT_ISOLATION_ABSENT_GLOBAL_CONFIG: &str = "git-tpl-test-absent-global-config";

/// `GIT_CONFIG_NOSYSTEM`/`GIT_CONFIG_GLOBAL`, scoped to a case that asked for
/// `git`: the two variables that make a literal `git` a case's own
/// `[commands]` spawns see nothing beyond the sandbox's own local
/// `.git/config` — never the global or system config of the machine running
/// `git tpl test`. See ADR-033.
fn git_isolation_env(cwd: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string()),
        (
            "GIT_CONFIG_GLOBAL".to_string(),
            cwd.join(".git")
                .join(GIT_ISOLATION_ABSENT_GLOBAL_CONFIG)
                .display()
                .to_string(),
        ),
    ])
}

/// `run_case`, before ADR-027: no sandbox, no `[commands]`, no `git`, the
/// render checked directly against `expect` and the snapshot. Kept as its
/// own function so a case with neither `[commands]` nor `git` — the
/// overwhelming majority — takes a path that is byte-for-byte what it always
/// has been, with no temporary directory created for nothing.
///
/// Since ADR-032, this is also the *only* path a `--write` run ever takes,
/// for every case it does not skip outright: `[commands]` never run under
/// `--write`, and this function never touches them, so routing every write
/// through here rather than the sandboxed path above is what keeps them from
/// running at all.
#[allow(clippy::too_many_arguments)]
fn run_case_plain(
    template: &Resolved,
    source: &str,
    tests_dir: &str,
    case: &Case,
    write: bool,
    user: &UserConfig,
    decisions: &BTreeMap<String, Decision>,
    progress: &mut dyn Progress,
) -> Result<CaseOutcome, OpError> {
    // Always defaults, never a prompt: there is nobody to ask in CI, and a
    // prompt in a test runner is a hang. A question the case leaves unanswered
    // with no default fails as `tpl::eval::unanswered`, which is a true
    // statement about the template.
    progress.case_status(&case.name, Status::Rendering);
    let rendered = render_resolved(
        template,
        source,
        None,
        case.answers.clone(),
        user,
        Answering::defaults(),
        // Replaying this case's own `trust` field, decided in `run` without
        // asking anybody. See ADR-028.
        Trust::decided(decisions.clone()),
    );

    let mut failures = Vec::new();
    let mut snapshot = SnapshotOutcome::None;
    let mut snapshot_changes = Vec::new();
    let mut files = 0;

    match rendered {
        Err(error) => {
            classify_render_error(error, &case.expect.error, &mut failures);
        }
        Ok(RenderedOnce {
            files: rendered,
            ignored_answers,
            ..
        }) => {
            // Unconditional: a case has no `--strict-answers` to opt into
            // and never will — see ADR-029 and #135. A violation is
            // classified exactly like a render that failed outright: there
            // is still nothing rendered for `expect`/the snapshot to check.
            if let Err(error) = enforce_strict_answers(
                true,
                &ignored_answers,
                template.manifest.questions.keys().cloned(),
            ) {
                classify_render_error(error, &case.expect.error, &mut failures);
            } else {
                files = rendered.len();

                if let Some(expected) = &case.expect.error {
                    failures.push(Failure::ExpectedError {
                        code: expected.clone(),
                    });
                } else if !write {
                    // `--write` records what a case's rendering *is*; it
                    // does not also prove it. Checking `expect` here would
                    // make every recording pay the same cost as a full run,
                    // exactly what made `--write` slow enough that reaching
                    // for it separately stopped being worth it. A plain
                    // `git tpl test` afterward still runs this. See ADR-032.
                    check(&case.expect, &rendered, &mut failures);
                }

                if case.snapshot {
                    progress.case_status(&case.name, Status::Snapshot);
                }
                (snapshot, snapshot_changes) =
                    snapshot_step(template, tests_dir, case, &rendered, write, &mut failures)?;
            }
        }
    }

    Ok(CaseOutcome {
        name: case.name.clone(),
        path: case.path.clone(),
        failures,
        snapshot,
        files,
        snapshot_changes,
        commands_run: 0,
        // Overwritten by the caller; see `run_case`'s own construction sites.
        duration_ms: 0,
    })
}

/// Run one list of shell-like command strings in `cwd`.
///
/// `stop_on_failure` decides what happens once one fails: `before`,
/// `rendered` and `after` are sequential — a case writes `mkdir -p src` then
/// `touch src/existing.rs` because the second assumes the first worked — so
/// `true` there ends the list rather than running commands whose own
/// precondition just failed to appear. `finally` passes `false`: it is
/// cleanup, and a step left undone because an earlier one failed is a worse
/// outcome than one more line in the report.
///
/// `root` is the resolved template's on-disk location, computed once by the
/// caller and passed unchanged into every list — see [`TEMPLATE_ROOT_ENV`].
///
/// Returns how many commands were actually attempted, so a caller can add it
/// to [`CaseOutcome::commands_run`] regardless of how the list ended.
#[allow(clippy::too_many_arguments)]
fn execute_commands(
    step: CommandStep,
    commands: &[String],
    cwd: &Path,
    root: &Path,
    env: &BTreeMap<String, String>,
    answers: &BTreeMap<String, Value>,
    stop_on_failure: bool,
    failures: &mut Vec<Failure>,
    name: &str,
    color: bool,
    progress: &mut dyn Progress,
) -> usize {
    let mut run = 0;
    for command in commands {
        run += 1;
        progress.case_status(name, Status::Command { step, command });
        let result = run_one(command, cwd, root, env, answers, color, name, progress);
        progress.command_finished(name, step, command, result.is_ok());
        if let Err((code, stdout, stderr)) = result {
            failures.push(Failure::CommandFailed {
                step,
                command: command.clone(),
                code,
                stdout,
                stderr,
            });
            if stop_on_failure {
                break;
            }
        }
    }
    run
}

/// Render `command` as a MiniJinja template before it is word-split, against
/// `env` — mirroring exactly what this command is about to be spawned with,
/// in the same precedence order `run_one` builds below, so `{{ env.X }}`
/// here and `$X` inside the process always agree — and `answers`, the
/// case's own `[answers]` table, unresolved. Not `computed`/`data`/
/// `template`: those only exist once a render has actually run, unavailable
/// to `before`, and exposing them would blur "a case's `[commands]` are
/// data, not a render" (ADR-016) further than the one thing this fixes
/// needs. See "Commands are MiniJinja templates" in ADR-027 (issue #153).
///
/// Skipped entirely when `command` carries no `{{`/`{%` — the same
/// `is_expression` gate every other optional-templating site in this crate
/// uses — so a plain command costs nothing and behaves exactly as before
/// this existed.
///
/// Strict undefined: a typo'd `answers.x` or `env.X` is this command's own
/// failure, not a silently empty path. ADR-014's direction, kept from day
/// one here rather than staged, since nothing depended on the lenient
/// behaviour before this function existed.
fn expand_command(
    command: &str,
    root: &Path,
    color: bool,
    env: &BTreeMap<String, String>,
    answers: &BTreeMap<String, Value>,
) -> Result<String, String> {
    if !is_expression(command) {
        return Ok(command.to_string());
    }

    // Same three layers `run_one` applies to the real `Command` below, in
    // the same order, so this mirror can never disagree with what the
    // process actually receives.
    let mut ctx_env: BTreeMap<String, minijinja::Value> = std::env::vars()
        .map(|(key, value)| (key, minijinja::Value::from(value)))
        .collect();
    ctx_env.insert(
        TEMPLATE_ROOT_ENV.to_string(),
        minijinja::Value::from(root.display().to_string()),
    );
    if color {
        ctx_env.insert("CLICOLOR_FORCE".to_string(), minijinja::Value::from("1"));
        ctx_env.insert("FORCE_COLOR".to_string(), minijinja::Value::from("1"));
    }
    for (key, value) in env {
        ctx_env.insert(key.clone(), minijinja::Value::from(value.as_str()));
    }

    let mut root_value: BTreeMap<String, minijinja::Value> = BTreeMap::new();
    root_value.insert("env".to_string(), minijinja::Value::from_iter(ctx_env));
    root_value.insert("answers".to_string(), Value::Table(answers.clone()).into());

    let jinja =
        crate::eval::environment_with(crate::eval::no_partials(), crate::eval::Undefined::Strict);
    jinja
        .render_str(command, minijinja::Value::from_iter(root_value))
        .map_err(|error| format!("could not expand `{command}`: {error}"))
}

/// Word-split `command` and run it directly — never through a shell.
///
/// `shlex::split` honours quotes and backslash escapes and nothing else: no
/// pipe, no glob, no redirection, no `$VAR` expansion. A case file is the
/// same untrusted-repository input invariant 5 already governs everywhere
/// else, and a real shell would hand every one of those to it for free. See
/// ADR-027. `{{ }}`/`{% %}` are a separate mechanism — [`expand_command`],
/// above — run before this, not a form of shell expansion.
///
/// `env` is merged on top of the inherited environment — `Command::envs`,
/// never `.env_clear()` — so a case that sets none behaves exactly as before
/// `env` existed. See "`env` scopes a command's environment" in ADR-027.
///
/// `root` is set as [`TEMPLATE_ROOT_ENV`] before `env` is applied, so a case
/// that deliberately sets that same key in its own `env`/`commands.env`
/// still wins — the same override precedent `env` itself already has over
/// the inherited environment. See "`TEMPLATE_ROOT` exposes the resolved
/// template's root" in ADR-027.
///
/// Spawned rather than `.output()`'d, so `progress.command_output` can be
/// called with each chunk as it is produced — but the full stdout/stderr is
/// still collected exactly as before, and `cap_output` still tail-caps it,
/// so a failure carries the identical data it always has.
#[allow(clippy::too_many_arguments)]
fn run_one(
    command: &str,
    cwd: &Path,
    root: &Path,
    env: &BTreeMap<String, String>,
    answers: &BTreeMap<String, Value>,
    color: bool,
    name: &str,
    progress: &mut dyn Progress,
) -> Result<(), (Option<i32>, String, String)> {
    let expanded = match expand_command(command, root, color, env, answers) {
        Ok(expanded) => expanded,
        Err(reason) => return Err((None, String::new(), reason)),
    };

    let argv = match shlex::split(&expanded) {
        Some(argv) if !argv.is_empty() => argv,
        // Empty, or an unterminated quote. Reported like any other failed
        // command: a fact about this one entry, not a reason to stop the run.
        // Named by the original `command`, not `expanded`: a case author
        // recognises their own text, even once it has been templated.
        _ => {
            return Err((
                None,
                String::new(),
                format!("`{command}` is not a runnable command"),
            ));
        }
    };

    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(cwd)
        .env(TEMPLATE_ROOT_ENV, root)
        // Never inherited: a command reading from a terminal that is not
        // there — because this one is piped for capture — would otherwise
        // hang the whole run instead of failing. The same guarantee
        // `Command::output()` already gave us.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if color {
        // A child talking to a pipe rather than a terminal otherwise assumes
        // nobody can see colour and silently prints in black and white —
        // exactly backwards when that output is about to be shown on a real
        // terminal, live under `-v` or in a failure report either way.
        // `env` below is applied after, so a case's own declaration still
        // wins for either key.
        cmd.env("CLICOLOR_FORCE", "1").env("FORCE_COLOR", "1");
    }
    cmd.envs(env);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        // No such program, not executable, or `cwd` could not be entered.
        // `code: None` for the same reason a signal-killed process gets one:
        // there is nothing to report but the operating system's own words.
        Err(error) => {
            return Err((
                None,
                String::new(),
                format!("could not run `{command}`: {error}"),
            ));
        }
    };

    // Read both pipes concurrently: a command with a chatty stdout and a
    // quiet stderr (or the reverse) would otherwise fill one pipe's buffer
    // while nobody drains it, deadlocking the child against its own output.
    // Only this thread ever touches `progress` — the reader threads below
    // send bytes, never the trait object — so `Progress` need not be `Send`.
    let stdout_pipe = child.stdout.take().expect("stdout was piped above");
    let stderr_pipe = child.stderr.take().expect("stderr was piped above");
    let (tx, rx) = std::sync::mpsc::channel::<(Stream, Vec<u8>)>();
    let stdout_thread = spawn_reader(stdout_pipe, Stream::Stdout, tx.clone());
    let stderr_thread = spawn_reader(stderr_pipe, Stream::Stderr, tx.clone());
    // Our own clones: without dropping them, `rx`'s loop below would never
    // see the channel close, since a sender is still alive even after both
    // reader threads finish.
    drop(tx);

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    for (stream, chunk) in rx {
        progress.command_output(name, stream, &chunk);
        match stream {
            Stream::Stdout => stdout_buf.extend_from_slice(&chunk),
            Stream::Stderr => stderr_buf.extend_from_slice(&chunk),
        }
    }
    // Already finished, by construction: a reader thread only exits its loop
    // once its pipe is at EOF or erroring, which is also what closes `rx`.
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            return Err((
                None,
                cap_output(&stdout_buf),
                format!("could not wait for `{command}`: {error}"),
            ));
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err((
            status.code(),
            cap_output(&stdout_buf),
            cap_output(&stderr_buf),
        ))
    }
}

/// Read `pipe` to EOF on its own thread, forwarding each chunk as it arrives.
///
/// Generic over `ChildStdout`/`ChildStderr`, which are distinct types, so
/// [`run_one`] needs only one reading loop rather than two copies of it.
fn spawn_reader<R: Read + Send + 'static>(
    mut pipe: R,
    stream: Stream,
    tx: std::sync::mpsc::Sender<(Stream, Vec<u8>)>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((stream, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

/// Keep the last [`COMMAND_OUTPUT_LIMIT_BYTES`] of a stream, not the first.
///
/// A failing build prints progress before its error. A head-truncated capture
/// would show the progress and lose the one line that explains why it failed.
fn cap_output(bytes: &[u8]) -> String {
    let tail = if bytes.len() > COMMAND_OUTPUT_LIMIT_BYTES {
        &bytes[bytes.len() - COMMAND_OUTPUT_LIMIT_BYTES..]
    } else {
        bytes
    };
    String::from_utf8_lossy(tail).into_owned()
}

/// Check a rendering against a case's expectations.
fn check(expect: &Expect, rendered: &[Rendered], failures: &mut Vec<Failure>) {
    let by_path: BTreeMap<&str, &Rendered> = rendered
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();

    for path in &expect.files {
        if !by_path.contains_key(path.as_str()) {
            failures.push(Failure::MissingFile {
                path: path.clone(),
                closest: closest(path, by_path.keys().copied()),
            });
        }
    }

    for path in &expect.absent {
        if by_path.contains_key(path.as_str()) {
            failures.push(Failure::UnexpectedFile { path: path.clone() });
        }
    }

    for (path, needles) in &expect.contains {
        let Some(file) = by_path.get(path.as_str()) else {
            failures.push(Failure::ContainsMissingFile { path: path.clone() });
            continue;
        };
        let Ok(text) = std::str::from_utf8(&file.content) else {
            failures.push(Failure::ContainsNotUtf8 { path: path.clone() });
            continue;
        };
        for needle in needles {
            if !text.contains(needle.as_str()) {
                failures.push(Failure::ContainsMissing {
                    path: path.clone(),
                    needle: needle.clone(),
                });
            }
        }
    }

    for (path, needles) in &expect.lacks {
        let Some(file) = by_path.get(path.as_str()) else {
            failures.push(Failure::LacksMissingFile { path: path.clone() });
            continue;
        };
        let Ok(text) = std::str::from_utf8(&file.content) else {
            failures.push(Failure::LacksNotUtf8 { path: path.clone() });
            continue;
        };
        for needle in needles {
            if text.contains(needle.as_str()) {
                failures.push(Failure::LacksPresent {
                    path: path.clone(),
                    needle: needle.clone(),
                });
            }
        }
    }
}

/// Every diagnostic code in an error and its cause chain, outermost first.
///
/// A case names the code that describes what went wrong, and that is usually
/// the innermost one: `tpl::render::content` says a file failed, and only the
/// `tpl::eval::expression` beneath it says why. Matching only the outer code
/// would make every case have to know which wrapper its failure happens to
/// arrive in, which is not part of the stable surface.
///
/// Deliberately parallel to `report::diagnostic` in the binary, which walks the
/// same chain by the same rule to build the `--json` envelope.
fn codes(error: &dyn Diagnostic) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(code) = error.code() {
            out.push(code.to_string());
        }
        current = error.diagnostic_source();
    }
    out
}

/// Classify a render failure against `expect.error`.
///
/// Used both for a render that failed outright, and — since #135 — for one
/// that succeeded but named an answer that matches no question: a case's
/// `[answers]` is hand-authored and lives next to the manifest it must track,
/// so an unrecognised key is unconditionally the case's own mistake, never a
/// stale-but-innocent leftover the way a recorded `--answers-from` file can
/// be. Either way there is no rendering for `expect`/the snapshot to check.
fn classify_render_error(error: OpError, expected: &Option<String>, failures: &mut Vec<Failure>) {
    let codes = codes(&error);
    match expected {
        Some(expected) if codes.iter().any(|code| code == expected) => {}
        Some(expected) => failures.push(Failure::WrongError {
            expected: expected.clone(),
            actual: codes,
            message: error.to_string(),
        }),
        None => failures.push(Failure::UnexpectedError {
            code: codes.into_iter().next(),
            message: error.to_string(),
        }),
    }
}

/// A file as a snapshot records it.
#[derive(Debug, Clone, PartialEq)]
struct SnapshotEntry {
    content: Vec<u8>,
    executable: bool,
}

/// Whether Git would call these bytes binary.
fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF_LEN)].contains(&0)
}

/// Compare, and record if asked.
///
/// The second half of the return value is the diff `--write` found against a
/// previously recorded snapshot — empty unless the outcome is
/// [`SnapshotOutcome::Updated`] — so a caller can show it in place of the
/// live `[commands]` output `--write` no longer produces (ADR-032). The
/// compare-mode diff (`write == false`) is not duplicated here: it is already
/// carried by the [`Failure::SnapshotDiff`] pushed below.
fn snapshot_step(
    template: &Resolved,
    tests_dir: &str,
    case: &Case,
    rendered: &[Rendered],
    write: bool,
    failures: &mut Vec<Failure>,
) -> Result<(SnapshotOutcome, Vec<SnapshotChange>), OpError> {
    // Snapshots are opt-in per case, and now explicitly so: a case that never
    // wrote `snapshot = true` is never written to and never compared, no
    // matter what `--write` does or what happens to be sitting on disk. This
    // is the whole point of making the opt-in explicit rather than inferred
    // from a directory's existence — reading the recorded snapshot is skipped
    // entirely, so a case that doesn't want one never even touches disk for
    // it.
    //
    // Note this is not reached at all for a case `--write` skips outright
    // (ADR-032): that decision is made by the caller, before rendering.
    if !case.snapshot {
        return Ok((SnapshotOutcome::None, Vec::new()));
    }

    let recorded = read_snapshot(template, tests_dir, case)?;

    let current: BTreeMap<String, SnapshotEntry> = rendered
        .iter()
        .map(|file| {
            (
                file.path.clone(),
                SnapshotEntry {
                    content: file.content.clone(),
                    executable: file.executable,
                },
            )
        })
        .collect();

    if !write {
        let Some(recorded) = recorded else {
            // `snapshot = true` asked for one, and none is recorded yet.
            // Silently skipping this — the pre-opt-in behaviour — would
            // report false confidence, exactly what the explicit flag exists
            // to prevent.
            failures.push(Failure::SnapshotMissing);
            return Ok((SnapshotOutcome::None, Vec::new()));
        };
        let changes = compare(&recorded, &current);
        if !changes.is_empty() {
            failures.push(Failure::SnapshotDiff { changes });
        }
        return Ok((SnapshotOutcome::Compared, Vec::new()));
    }

    // Compared *before* writing, so `--write` on a green suite says "unchanged"
    // rather than claiming to have rewritten twelve files that a reviewer would
    // then have to check. Also the source of what a verbose `--write` shows in
    // place of the live command output it no longer produces (ADR-032).
    let changes = recorded
        .as_ref()
        .map(|recorded| compare(recorded, &current));
    let outcome = match (&recorded, changes.as_ref()) {
        (None, _) => SnapshotOutcome::Written,
        (Some(_), Some(changes)) if changes.is_empty() => SnapshotOutcome::Unchanged,
        (Some(_), Some(_)) => SnapshotOutcome::Updated,
        (Some(_), None) => unreachable!("changes is computed above whenever recorded is Some"),
    };

    if outcome != SnapshotOutcome::Unchanged {
        write_snapshot(template, tests_dir, case, &current)?;
    }

    Ok((outcome, changes.unwrap_or_default()))
}

/// Read a recorded snapshot out of the resolved tree.
///
/// From the tree, like the cases themselves, so `--ref v1.2.0` compares against
/// that tag's snapshots. Reading them off the filesystem would make `--ref` a
/// lie.
///
/// The implicit no-`--ref` default is the one exception, and reads the
/// snapshot's own directory straight off disk rather than through
/// `template.tree` — the whole-repository dirty tree, already filtered by
/// `.gitignore`. A snapshot is data `--write` puts on disk directly, bypassing
/// Git on purpose; letting an ordinary rule matching its own filename, a bare
/// `MANIFEST` say, make that file disappear on read-back would disagree with
/// what `--write` just produced (#116). There is no `--ref` to be a lie about
/// here: no `--ref` already means "read the workdir", for the snapshot
/// exactly as for everything else it reads.
fn read_snapshot(
    template: &Resolved,
    tests_dir: &str,
    case: &Case,
) -> Result<Option<BTreeMap<String, SnapshotEntry>>, OpError> {
    let dir = snapshot_path(tests_dir, &case.name);

    let unreadable = |reason: String| TestingError::SnapshotRead {
        case: case.name.clone(),
        path: dir.clone(),
        reason,
    };

    let tree = if template.dirty {
        let workdir = template.repo.workdir()?;
        let path = workdir.join(&dir);
        if !path.is_dir() {
            return Ok(None);
        }
        Some(template.repo.tree_from_directory(&path)?)
    } else {
        template.repo.subtree(template.tree, &dir)?
    };
    let Some(tree) = tree else {
        return Ok(None);
    };

    let entries = template.repo.list_tree(tree)?;

    let manifest_oid = entries
        .iter()
        .find(|entry| entry.path == SNAPSHOT_MANIFEST)
        .map(|entry| entry.oid);
    let Some(manifest_oid) = manifest_oid else {
        return Err(unreadable(format!("there is no `{SNAPSHOT_MANIFEST}`")).into());
    };

    let manifest = template.repo.read_blob(manifest_oid)?;
    let manifest = parse_manifest(&manifest, &unreadable)?;

    let prefix = format!("{SNAPSHOT_FILES}/");
    let mut files: BTreeMap<String, (Oid, bool)> = BTreeMap::new();
    for entry in &entries {
        let Some(path) = entry.path.strip_prefix(&prefix) else {
            continue;
        };
        files.insert(
            path.to_string(),
            (entry.oid, entry.mode == FileMode::BlobExecutable),
        );
    }

    // The manifest is authoritative for the file list and the modes; `files/`
    // is authoritative for content. Trusting either half alone would let a
    // hand-edited snapshot drift into asserting nothing.
    let mut out = BTreeMap::new();
    for (path, recorded) in &manifest {
        let Some((oid, _)) = files.get(path) else {
            return Err(unreadable(format!(
                "`{SNAPSHOT_MANIFEST}` lists `{path}`, which is not under `{SNAPSHOT_FILES}/`"
            ))
            .into());
        };
        let content = template.repo.read_blob(*oid)?;
        if content.len() as u64 != recorded.size {
            return Err(unreadable(format!(
                "`{path}` is {} bytes, but `{SNAPSHOT_MANIFEST}` records {}",
                content.len(),
                recorded.size
            ))
            .into());
        }
        if let Some(digest) = &recorded.digest
            && sha256(&content) != *digest
        {
            return Err(unreadable(format!(
                "`{path}` does not match the digest in `{SNAPSHOT_MANIFEST}`"
            ))
            .into());
        }
        out.insert(
            path.clone(),
            SnapshotEntry {
                content,
                executable: recorded.executable,
            },
        );
    }

    for path in files.keys() {
        if !manifest.contains_key(path) {
            return Err(unreadable(format!(
                "`{SNAPSHOT_FILES}/{path}` is not listed in `{SNAPSHOT_MANIFEST}`"
            ))
            .into());
        }
    }

    Ok(Some(out))
}

/// A manifest line.
struct RecordedFile {
    executable: bool,
    /// `None` for a binary file, which records no digest.
    digest: Option<String>,
    size: u64,
}

fn parse_manifest(
    bytes: &[u8],
    unreadable: &impl Fn(String) -> TestingError,
) -> Result<BTreeMap<String, RecordedFile>, TestingError> {
    let text = std::str::from_utf8(bytes).map_err(|_| unreadable("it is not UTF-8".to_string()))?;

    let mut out = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // The path is last so that a path containing spaces needs no quoting.
        let mut parts = line.splitn(4, ' ');
        let (Some(mode), Some(digest), Some(size), Some(path)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(unreadable(format!("`{line}` is not a manifest entry")));
        };
        let executable = match mode {
            "100644" => false,
            "100755" => true,
            other => return Err(unreadable(format!("`{other}` is not a file mode"))),
        };
        let size = size
            .parse::<u64>()
            .map_err(|_| unreadable(format!("`{size}` is not a byte count")))?;
        out.insert(
            path.to_string(),
            RecordedFile {
                executable,
                digest: (digest != "binary").then(|| digest.to_string()),
                size,
            },
        );
    }
    Ok(out)
}

fn render_manifest(case: &str, files: &BTreeMap<String, SnapshotEntry>) -> String {
    let mut out = String::new();
    out.push_str(SNAPSHOT_VERSION);
    out.push('\n');
    out.push_str(&format!("# case: {case}\n"));
    out.push_str("# Written by `git tpl test --write`. Do not edit by hand.\n");
    for (path, entry) in files {
        let mode = if entry.executable {
            FileMode::BlobExecutable
        } else {
            FileMode::Blob
        };
        // `binary` in place of the digest, so the comparator knows not to try a
        // patch — the same distinction `FileStat.binary` makes for `diff`.
        let digest = if is_binary(&entry.content) {
            "binary".to_string()
        } else {
            sha256(&entry.content)
        };
        out.push_str(&format!(
            "{:06o} {digest} {} {path}\n",
            mode.as_u32(),
            entry.content.len()
        ));
    }
    out
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn snapshot_path(tests_dir: &str, case: &str) -> String {
    format!("{tests_dir}/{SNAPSHOTS_DIR}/{case}")
}

/// Compare a recorded snapshot with a rendering.
fn compare(
    recorded: &BTreeMap<String, SnapshotEntry>,
    current: &BTreeMap<String, SnapshotEntry>,
) -> Vec<SnapshotChange> {
    let paths: BTreeSet<&String> = recorded.keys().chain(current.keys()).collect();

    let mut changes = Vec::new();
    for path in paths {
        match (recorded.get(path), current.get(path)) {
            (None, Some(_)) => changes.push(SnapshotChange {
                path: path.clone(),
                kind: ChangeKind::Added,
                mode_only: false,
                patch: None,
            }),
            (Some(_), None) => changes.push(SnapshotChange {
                path: path.clone(),
                kind: ChangeKind::Deleted,
                mode_only: false,
                patch: None,
            }),
            (Some(before), Some(after)) => {
                if before.content == after.content {
                    if before.executable != after.executable {
                        changes.push(SnapshotChange {
                            path: path.clone(),
                            kind: ChangeKind::Modified,
                            mode_only: true,
                            patch: None,
                        });
                    }
                    continue;
                }
                changes.push(SnapshotChange {
                    path: path.clone(),
                    kind: ChangeKind::Modified,
                    mode_only: false,
                    patch: patch(&before.content, &after.content),
                });
            }
            (None, None) => unreachable!("the path came from one of the two maps"),
        }
    }
    changes
}

/// A unified diff, when both sides are text.
///
/// Produced in process rather than by `GitBackend::diff_patch`, which needs two
/// *trees* — using it would mean writing blobs and trees into the template
/// repository to answer a question that reads nothing.
fn patch(before: &[u8], after: &[u8]) -> Option<String> {
    if is_binary(before) || is_binary(after) {
        return None;
    }
    let (Ok(before), Ok(after)) = (std::str::from_utf8(before), std::str::from_utf8(after)) else {
        return None;
    };
    let diff = similar::TextDiff::from_lines(before, after);
    Some(
        diff.unified_diff()
            .context_radius(3)
            .header("snapshot", "rendered")
            .to_string(),
    )
}

/// Record a snapshot in the template's working tree.
///
/// The one thing this command writes, and it goes to the working tree rather
/// than into Git: a snapshot has to be reviewable with `git diff` and committed
/// deliberately. Nothing is staged and nothing is committed.
fn write_snapshot(
    template: &Resolved,
    tests_dir: &str,
    case: &Case,
    files: &BTreeMap<String, SnapshotEntry>,
) -> Result<(), OpError> {
    let workdir = template.repo.workdir()?;
    let dir = workdir.join(snapshot_path(tests_dir, &case.name));

    let failed = |path: &Path, verb: &str, error: &std::io::Error| TestingError::SnapshotWrite {
        case: case.name.clone(),
        path: path.display().to_string(),
        reason: format!("could not {verb} it: {error}"),
    };

    super::clear_directory(&dir, &failed)?;

    super::materialise(
        &dir.join(SNAPSHOT_FILES),
        files
            .iter()
            .map(|(path, entry)| (path.as_str(), entry.content.as_slice(), entry.executable)),
        &failed,
    )?;

    let manifest = dir.join(SNAPSHOT_MANIFEST);
    std::fs::write(&manifest, render_manifest(&case.name, files))
        .map_err(|error| failed(&manifest, "write", &error))?;

    Ok(())
}

fn durations_path(tests_dir: &str) -> String {
    format!("{tests_dir}/{DURATIONS_DIR}/{DURATIONS_FILE}")
}

fn parse_durations(
    bytes: &[u8],
    unreadable: &impl Fn(String) -> TestingError,
) -> Result<BTreeMap<String, u64>, TestingError> {
    let text = std::str::from_utf8(bytes).map_err(|_| unreadable("it is not UTF-8".to_string()))?;

    let mut out = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // The name is first, unlike the manifest's path-last convention: a
        // case name cannot contain a space (it is a file stem), so there is
        // no quoting problem to avoid by ordering it differently.
        let mut parts = line.splitn(2, ' ');
        let (Some(name), Some(ms)) = (parts.next(), parts.next()) else {
            return Err(unreadable(format!("`{line}` is not a durations entry")));
        };
        let ms = ms
            .parse::<u64>()
            .map_err(|_| unreadable(format!("`{ms}` is not a millisecond count")))?;
        out.insert(name.to_string(), ms);
    }
    Ok(out)
}

fn render_durations(durations: &BTreeMap<String, u64>) -> String {
    let mut out = String::new();
    out.push_str(DURATIONS_VERSION);
    out.push('\n');
    out.push_str("# Written by `git tpl test --record-durations`. Do not edit by hand.\n");
    for (name, ms) in durations {
        out.push_str(&format!("{name} {ms}\n"));
    }
    out
}

/// Read the durations file recorded at a previous `--record-durations` run.
///
/// `Ok(None)` when it has never been recorded — never an error: `--shard`
/// without one falls back to an even split (ADR-036), so a template's first
/// `--shard` run, before anyone has run `--record-durations` once, must not
/// fail.
fn read_durations(
    template: &Resolved,
    tests_dir: &str,
) -> Result<Option<BTreeMap<String, u64>>, OpError> {
    let path = durations_path(tests_dir);
    let unreadable = |reason: String| TestingError::DurationsRead {
        path: path.clone(),
        reason,
    };

    // Straight off disk when dirty, for the same reason `read_snapshot` does
    // (issue #116): this file is data `--record-durations` writes directly,
    // bypassing `.gitignore` on purpose, and reading it back through the
    // dirty tree's own ignore-filtering could make a durations file whose
    // name happens to match an ordinary rule disappear.
    let bytes = if template.dirty {
        let workdir = template.repo.workdir()?;
        let full = workdir.join(&path);
        if !full.is_file() {
            return Ok(None);
        }
        std::fs::read(&full).map_err(|error| unreadable(error.to_string()))?
    } else {
        let Some(bytes) = template.repo.read_path(template.tree, &path)? else {
            return Ok(None);
        };
        bytes
    };

    Ok(Some(parse_durations(&bytes, &unreadable)?))
}

/// Record this run's measured durations, merged over whatever was already
/// recorded — never a full replace, so `git tpl test --record-durations
/// some-case` does not erase every other case's history. See ADR-036.
fn write_durations(
    template: &Resolved,
    tests_dir: &str,
    previous: Option<&BTreeMap<String, u64>>,
    measured: &BTreeMap<String, u64>,
) -> Result<(), OpError> {
    let mut merged = previous.cloned().unwrap_or_default();
    merged.extend(measured.iter().map(|(name, ms)| (name.clone(), *ms)));

    let workdir = template.repo.workdir()?;
    let dir = workdir.join(tests_dir).join(DURATIONS_DIR);
    let failed = |path: &Path, verb: &str, error: &std::io::Error| TestingError::DurationsWrite {
        path: path.display().to_string(),
        reason: format!("could not {verb} it: {error}"),
    };
    std::fs::create_dir_all(&dir).map_err(|error| failed(&dir, "create", &error))?;
    let path = dir.join(DURATIONS_FILE);
    std::fs::write(&path, render_durations(&merged))
        .map_err(|error| failed(&path, "write", &error))?;
    Ok(())
}

/// Parse `--shard`'s `INDEX/TOTAL`, both 1-based.
fn parse_shard(spec: &str) -> Result<(usize, usize), TestingError> {
    let malformed = |reason: &str| TestingError::MalformedShard {
        spec: spec.to_string(),
        reason: reason.to_string(),
    };
    let (index, total) = spec
        .split_once('/')
        .ok_or_else(|| malformed("must be written as `INDEX/TOTAL`, e.g. `--shard 2/4`"))?;
    let index: usize = index
        .parse()
        .map_err(|_| malformed("INDEX is not a number"))?;
    let total: usize = total
        .parse()
        .map_err(|_| malformed("TOTAL is not a number"))?;
    if index == 0 || total == 0 {
        return Err(malformed("INDEX and TOTAL both start at 1"));
    }
    if index > total {
        return Err(malformed(&format!(
            "INDEX ({index}) cannot exceed TOTAL ({total})"
        )));
    }
    Ok((index, total))
}

/// Select this shard's cases out of every case `discover` and the positional
/// filter already narrowed down to.
///
/// `durations` drives a balanced split when present and non-empty; `None` or
/// empty falls back to a deterministic split by count, in the same order
/// `discover` already sorted them into. See ADR-036.
///
/// Returns the selected cases, in `cases`' own original order (never
/// weight-sorted order — a shard's own run order must stay as legible as an
/// unsharded run's), and whether a balanced (as opposed to even) split was
/// used.
fn shard_cases(
    cases: Vec<Case>,
    index: usize,
    total: usize,
    durations: Option<&BTreeMap<String, u64>>,
) -> (Vec<Case>, bool) {
    if total == 1 {
        return (cases, false);
    }

    let balanced = durations.is_some_and(|durations| !durations.is_empty());

    let bucket_of: BTreeMap<String, usize> = if balanced {
        // pytest-split's `least_duration`: known durations descending,
        // unknown at the mean of known ones, one combined descending sort,
        // greedy-assigned to whichever bucket currently holds the least
        // accumulated weight. Ties (equal weight, or an equally-light
        // bucket) break on name/lowest-index — a `BTreeMap`/`Vec`, never a
        // hash map, so the split cannot depend on iteration order.
        let durations = durations.expect("balanced implies Some and non-empty");
        let known: Vec<u64> = cases
            .iter()
            .filter_map(|case| durations.get(&case.name).copied())
            .collect();
        let mean = if known.is_empty() {
            0
        } else {
            known.iter().sum::<u64>() / known.len() as u64
        };

        let mut weighted: Vec<(&Case, u64)> = cases
            .iter()
            .map(|case| (case, durations.get(&case.name).copied().unwrap_or(mean)))
            .collect();
        weighted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));

        let mut bucket_weight = vec![0u64; total];
        let mut bucket_of = BTreeMap::new();
        for (case, weight) in weighted {
            let lightest = bucket_weight
                .iter()
                .enumerate()
                .min_by_key(|(index, weight)| (**weight, *index))
                .map(|(index, _)| index)
                .expect("total > 0, checked above");
            bucket_weight[lightest] += weight;
            bucket_of.insert(case.name.clone(), lightest);
        }
        bucket_of
    } else {
        // Even split by count: shard i gets a contiguous run of
        // `len / total` cases, the first `len % total` shards getting one
        // extra — deterministic, and matches `discover`'s own name order.
        let per = cases.len() / total;
        let extra = cases.len() % total;
        let mut bucket_of = BTreeMap::new();
        let mut i = 0;
        for bucket in 0..total {
            let n = per + usize::from(bucket < extra);
            for _ in 0..n {
                if let Some(case) = cases.get(i) {
                    bucket_of.insert(case.name.clone(), bucket);
                }
                i += 1;
            }
        }
        bucket_of
    };

    let wanted = index - 1;
    let selected = cases
        .into_iter()
        .filter(|case| bucket_of.get(&case.name) == Some(&wanted))
        .collect();
    (selected, balanced)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn case(body: &str) -> Result<Case, TestingError> {
        Case::parse("c", "tests/c.toml", body.as_bytes())
    }

    fn shape_reason(result: Result<Case, TestingError>) -> String {
        match result {
            Err(TestingError::CaseShape { reason, .. }) => reason,
            other => panic!("expected a shape error, got {other:?}"),
        }
    }

    #[test]
    fn a_case_with_only_answers_expects_the_render_to_succeed() {
        let parsed = case("[answers]\nname = \"thing\"\n").unwrap();
        assert_eq!(parsed.name, "c");
        assert_eq!(
            parsed.answers.get("name"),
            Some(&Value::String("thing".into()))
        );
        assert_eq!(parsed.expect, Expect::default());
    }

    #[test]
    fn a_case_with_neither_section_is_still_a_case() {
        assert_eq!(case("").unwrap().expect, Expect::default());
    }

    #[test]
    fn an_unknown_top_level_key_is_refused_with_a_suggestion() {
        let reason = shape_reason(case("[expects]\nfiles = []\n"));
        assert!(reason.contains("Did you mean `expect`?"), "{reason}");
    }

    #[test]
    fn an_unknown_expectation_is_refused_with_a_suggestion() {
        let reason = shape_reason(case("[expect]\nfile = []\n"));
        assert!(reason.contains("Did you mean `files`?"), "{reason}");
    }

    #[test]
    fn a_near_miss_of_lacks_is_suggested() {
        let reason = shape_reason(case("[expect]\nlack = []\n"));
        assert!(reason.contains("Did you mean `lacks`?"), "{reason}");
    }

    #[test]
    fn contains_accepts_a_bare_string_as_well_as_an_array() {
        let parsed =
            case("[expect.contains]\n\"a.toml\" = \"x\"\n\"b.toml\" = [\"y\", \"z\"]\n").unwrap();
        assert_eq!(parsed.expect.contains["a.toml"], vec!["x"]);
        assert_eq!(parsed.expect.contains["b.toml"], vec!["y", "z"]);
    }

    #[test]
    fn lacks_accepts_a_bare_string_as_well_as_an_array() {
        let parsed =
            case("[expect.lacks]\n\"a.toml\" = \"x\"\n\"b.toml\" = [\"y\", \"z\"]\n").unwrap();
        assert_eq!(parsed.expect.lacks["a.toml"], vec!["x"]);
        assert_eq!(parsed.expect.lacks["b.toml"], vec!["y", "z"]);
    }

    #[test]
    fn an_error_that_is_not_a_diagnostic_code_is_refused() {
        let reason = shape_reason(case("[expect]\nerror = \"it broke\"\n"));
        assert!(reason.contains("tpl::<area>::<kind>"), "{reason}");
    }

    #[test]
    fn a_case_cannot_expect_an_error_and_a_file_at_once() {
        let reason = shape_reason(case(
            "[expect]\nerror = \"tpl::eval::wrong_type\"\nfiles = [\"a\"]\n",
        ));
        assert!(reason.contains("Split them into two cases"), "{reason}");
    }

    #[test]
    fn a_case_cannot_expect_an_error_and_lacks_at_once() {
        let reason = shape_reason(case(
            "[expect]\nerror = \"tpl::eval::wrong_type\"\n\n[expect.lacks]\n\"a\" = \"x\"\n",
        ));
        assert!(reason.contains("Split them into two cases"), "{reason}");
    }

    #[test]
    fn files_must_be_strings() {
        let reason = shape_reason(case("[expect]\nfiles = [1]\n"));
        assert!(reason.contains("must contain strings"), "{reason}");
    }

    #[test]
    fn a_case_that_is_not_a_table_is_refused() {
        let result = Case::parse("c", "tests/c.json", b"[1, 2]");
        std::assert_matches!(result, Err(TestingError::CaseShape { .. }));
    }

    /// Each of these is a section written with the wrong type. The message has
    /// to name what the section should be, because "invalid case" tells an
    /// author nothing they cannot already see.
    #[rstest]
    #[case("answers = 1\n", "`answers` must be a table")]
    #[case("trust = 1\n", "`trust` must be `true` or `false`")]
    #[case("expect = 1\n", "`expect` must be a table")]
    #[case("[expect]\ncontains = 1\n", "`expect.contains` must be a table")]
    #[case("[expect]\nerror = 1\n", "`expect.error` must be a diagnostic code")]
    #[case("[expect]\nfiles = 1\n", "`expect.files` must be an array of strings")]
    #[case(
        "[expect]\nabsent = 1\n",
        "`expect.absent` must be an array of strings"
    )]
    #[case(
        "[expect.contains]\n\"a\" = 1\n",
        "`expect.contains.\"a\"` must be an array of strings"
    )]
    #[case("[expect]\nlacks = 1\n", "`expect.lacks` must be a table")]
    #[case(
        "[expect.lacks]\n\"a\" = 1\n",
        "`expect.lacks.\"a\"` must be an array of strings"
    )]
    #[case("git = 1\n", "`git` must be `true`, `false`, or a table overriding")]
    fn a_section_of_the_wrong_type_names_the_type_it_should_be(
        #[case] body: &str,
        #[case] expected: &str,
    ) {
        let reason = shape_reason(case(body));
        assert!(reason.contains(expected), "{reason}");
    }

    #[test]
    fn git_true_gives_the_built_in_identity() {
        let parsed = case("git = true\n").unwrap();
        let git = parsed.git.expect("git sandbox");
        assert_eq!(git.user_name, "git-tpl test");
        assert_eq!(git.user_email, "test@git-tpl.invalid");
    }

    #[rstest]
    #[case("")]
    #[case("git = false\n")]
    fn git_false_and_git_absent_parse_identically(#[case] body: &str) {
        assert_eq!(case(body).unwrap().git, None);
    }

    #[test]
    fn a_git_table_may_override_the_identity() {
        let parsed =
            case("[git]\nuser.name = \"Someone\"\nuser.email = \"someone@example.com\"\n").unwrap();
        let git = parsed.git.expect("git sandbox");
        assert_eq!(git.user_name, "Someone");
        assert_eq!(git.user_email, "someone@example.com");
    }

    #[test]
    fn a_git_table_may_override_only_one_leaf() {
        // The other leaf still falls back to the built-in default: an
        // override is per-key, not all-or-nothing.
        let parsed = case("[git]\nuser.name = \"Someone\"\n").unwrap();
        let git = parsed.git.expect("git sandbox");
        assert_eq!(git.user_name, "Someone");
        assert_eq!(git.user_email, "test@git-tpl.invalid");
    }

    #[test]
    fn an_unknown_top_level_key_hints_git() {
        let reason = shape_reason(case("gi = true\n"));
        assert!(reason.contains("Did you mean `git`?"), "{reason}");
    }

    #[test]
    fn an_unknown_git_key_is_refused_with_a_suggestion() {
        let reason = shape_reason(case("[git]\nusr.name = \"x\"\n"));
        assert!(reason.contains("Did you mean `user`?"), "{reason}");
    }

    #[test]
    fn an_unknown_git_user_key_is_refused_with_a_suggestion() {
        let reason = shape_reason(case("[git.user]\nemai = \"x\"\n"));
        assert!(reason.contains("Did you mean `email`?"), "{reason}");
    }

    #[test]
    fn a_git_user_leaf_of_the_wrong_type_names_the_type_it_should_be() {
        let reason = shape_reason(case("[git.user]\nname = 1\n"));
        assert!(
            reason.contains("`git.user.name` must be a string"),
            "{reason}"
        );
    }

    #[test]
    fn git_must_be_a_table_when_not_a_bool() {
        let reason = shape_reason(case("git = \"yes\"\n"));
        assert!(
            reason.contains("`git` must be `true`, `false`, or a table overriding"),
            "{reason}"
        );
    }

    #[test]
    fn a_case_that_does_not_parse_names_the_format() {
        match Case::parse("c", "tests/c.json", b"{not json") {
            Err(TestingError::CaseParse { format, .. }) => assert_eq!(format, "json"),
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    fn read_manifest(body: &str) -> Result<BTreeMap<String, RecordedFile>, TestingError> {
        parse_manifest(body.as_bytes(), &|reason| TestingError::SnapshotRead {
            case: "c".into(),
            path: "p".into(),
            reason,
        })
    }

    fn snapshot_read_reason(
        result: Result<BTreeMap<String, RecordedFile>, TestingError>,
    ) -> String {
        match result {
            Err(TestingError::SnapshotRead { reason, .. }) => reason,
            Ok(_) => panic!("expected the manifest to be refused"),
            Err(other) => panic!("expected a snapshot read error, got {other:?}"),
        }
    }

    /// A manifest that has been hand-edited into nonsense is refused rather
    /// than half-read. Silently dropping a malformed line would quietly shrink
    /// the set of files the snapshot asserts on.
    #[rstest]
    #[case("100644 abc\n", "is not a manifest entry")]
    #[case("100600 abc 3 a.txt\n", "`100600` is not a file mode")]
    #[case("100644 abc three a.txt\n", "`three` is not a byte count")]
    fn a_malformed_manifest_line_is_refused(#[case] body: &str, #[case] expected: &str) {
        let reason = snapshot_read_reason(read_manifest(body));
        assert!(reason.contains(expected), "{reason}");
    }

    #[test]
    fn a_manifest_path_may_contain_spaces() {
        // The path is last precisely so it needs no quoting.
        let parsed = read_manifest("100644 abc 3 a file.txt\n").unwrap();
        assert_eq!(parsed.keys().collect::<Vec<_>>(), ["a file.txt"]);
    }

    #[test]
    fn manifest_comments_and_blank_lines_are_skipped() {
        let parsed = read_manifest("# a comment\n\n100644 abc 3 a.txt\n").unwrap();
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn the_manifest_round_trips() {
        let mut files = BTreeMap::new();
        files.insert(
            "a.txt".to_string(),
            SnapshotEntry {
                content: b"hello\n".to_vec(),
                executable: false,
            },
        );
        files.insert(
            "run.sh".to_string(),
            SnapshotEntry {
                content: b"#!/bin/sh\n".to_vec(),
                executable: true,
            },
        );
        files.insert(
            "logo.png".to_string(),
            SnapshotEntry {
                content: vec![0, 1, 2, 3],
                executable: false,
            },
        );

        let rendered = render_manifest("c", &files);
        let parsed = parse_manifest(rendered.as_bytes(), &|reason| TestingError::SnapshotRead {
            case: "c".into(),
            path: "p".into(),
            reason,
        })
        .unwrap();

        assert_eq!(parsed.len(), 3);
        assert!(!parsed["a.txt"].executable);
        assert!(parsed["run.sh"].executable);
        assert_eq!(parsed["logo.png"].digest, None, "binary records no digest");
        assert_eq!(parsed["a.txt"].size, 6);
    }

    fn entry(content: &[u8], executable: bool) -> SnapshotEntry {
        SnapshotEntry {
            content: content.to_vec(),
            executable,
        }
    }

    #[test]
    fn the_comparator_names_added_deleted_and_modified_paths() {
        let recorded = BTreeMap::from([
            ("kept".to_string(), entry(b"same", false)),
            ("gone".to_string(), entry(b"old", false)),
            ("changed".to_string(), entry(b"a\n", false)),
        ]);
        let current = BTreeMap::from([
            ("kept".to_string(), entry(b"same", false)),
            ("new".to_string(), entry(b"fresh", false)),
            ("changed".to_string(), entry(b"b\n", false)),
        ]);

        let changes = compare(&recorded, &current);
        let by_path: BTreeMap<&str, &SnapshotChange> = changes
            .iter()
            .map(|change| (change.path.as_str(), change))
            .collect();

        assert_eq!(by_path.len(), 3, "an unchanged file is not a change");
        assert_eq!(by_path["new"].kind, ChangeKind::Added);
        assert_eq!(by_path["gone"].kind, ChangeKind::Deleted);
        assert_eq!(by_path["changed"].kind, ChangeKind::Modified);
        assert!(by_path["changed"].patch.is_some());
    }

    #[test]
    fn a_mode_change_alone_is_a_change() {
        let changes = compare(
            &BTreeMap::from([("run.sh".to_string(), entry(b"#!/bin/sh\n", false))]),
            &BTreeMap::from([("run.sh".to_string(), entry(b"#!/bin/sh\n", true))]),
        );
        assert_eq!(changes.len(), 1);
        assert!(changes[0].mode_only);
        assert!(changes[0].patch.is_none());
    }

    /// Not every unreadable file has a NUL in it. A Latin-1 `README` sniffs as
    /// text and still cannot be decoded, and the diff has to decline rather
    /// than panic on it.
    #[test]
    fn a_change_that_is_not_utf8_carries_no_patch_either() {
        let before = [0xffu8, 0xfe, b'a'];
        let after = [0xffu8, 0xfe, b'b'];
        assert!(
            !is_binary(&before),
            "no NUL, so not binary by the heuristic"
        );
        assert!(patch(&before, &after).is_none());
    }

    #[test]
    fn a_binary_change_carries_no_patch() {
        let changes = compare(
            &BTreeMap::from([("logo.png".to_string(), entry(&[0, 1, 2], false))]),
            &BTreeMap::from([("logo.png".to_string(), entry(&[0, 9, 9], false))]),
        );
        assert_eq!(changes[0].kind, ChangeKind::Modified);
        assert!(changes[0].patch.is_none(), "no patch for binary");
    }

    /// `Silent` is what a caller with nothing to say uses — today, no caller
    /// in this crate; `commands::test` overrides every method instead. The
    /// point of this test is that none of the following panics: every
    /// default body really is empty, for a future caller (this crate as a
    /// library, say) that never implements `Progress` at all.
    #[test]
    fn silent_progress_does_nothing_for_every_event() {
        let mut progress = Silent;
        progress.case_started("c");
        progress.case_status("c", Status::Rendering);
        progress.case_status(
            "c",
            Status::Command {
                step: CommandStep::Before,
                command: "true",
            },
        );
        progress.case_status("c", Status::Snapshot);
        progress.command_finished("c", CommandStep::Before, "true", true);
        progress.command_output("c", Stream::Stdout, b"noise");
        progress.case_finished(&CaseOutcome {
            name: "c".to_string(),
            path: "tests/c.toml".to_string(),
            failures: Vec::new(),
            snapshot: SnapshotOutcome::None,
            files: 1,
            commands_run: 0,
            snapshot_changes: Vec::new(),
            duration_ms: 0,
        });
    }
}
