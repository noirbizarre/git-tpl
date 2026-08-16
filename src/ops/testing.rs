//! `git tpl test` — running a template's own test cases.
//!
//! A case is *data*: an answer set and a set of expectations, written in the
//! template repository and read by the same parsers `--answers-from` uses.
//! Nothing here executes anything the template asks for — that is invariant 5,
//! and it is why the assertion vocabulary is deliberately closed to `files`,
//! `absent`, `contains`, `error` and a snapshot. Checking a rendering with the
//! tools that understand it is the author's own CI's job.
//!
//! The area of every diagnostic here is `testing` rather than `test`, because
//! `tests/diagnostics.rs` deliberately ignores codes whose area is `test` —
//! `src/report.rs` uses `tpl::test::*` as fixtures, and a runner claiming that
//! area would drop itself out of the coverage guard.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use miette::Diagnostic;
use thiserror::Error;

use super::{Answering, OpError, RenderedOnce, Resolved, Target, Trust, render_resolved, resolve};
use crate::data::format::{Format, parse_value};
use crate::data::{Decision, REMOTE_LIMIT_BYTES, declared_remotes};
use crate::git::{ChangeKind, FileMode, Oid};
use crate::render::Rendered;
use crate::suggest::closest;
use crate::template::Value;
use crate::userconfig::UserConfig;

/// The directory cases are read from when `--tests` does not say otherwise.
pub const DEFAULT_TESTS_DIR: &str = "tests";

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

/// How many bytes are sniffed for a NUL before a file is called binary.
///
/// Git's own heuristic. A binary file is stored verbatim like any other, but no
/// patch is attempted for it.
const BINARY_SNIFF_LEN: usize = 8000;

/// Errors that stop a test run.
///
/// Only these. An unmet expectation is a [`Failure`] carried in the report, for
/// the same reason a lint finding is not an error: twelve failing cases must
/// all be reported, and an error that aborts at the first would report one.
#[derive(Debug, Error, Diagnostic)]
pub enum TestError {
    /// The tests directory does not exist at the resolved revision.
    #[error("`{dir}` does not exist in the template at {revision}")]
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
        /// The revision it was looked for at.
        revision: String,
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

    /// `--write` was used on a template with no working tree.
    #[error("`--write` needs a local template checkout")]
    #[diagnostic(
        code(tpl::testing::write_needs_local),
        help(
            "a snapshot is written to the working tree, and `{origin}` has none.\n\
             Clone it and run `git tpl test --write` there."
        )
    )]
    WriteNeedsLocal {
        /// The source that has no working tree.
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
    /// A diagnostic code the render must fail with.
    ///
    /// Present makes this a *failure* case: a successful render fails it. A
    /// code and never a message, because messages are expected to improve and
    /// a suite that pins prose makes every improvement a breaking change.
    pub error: Option<String>,
}

impl Case {
    /// Parse a case file.
    pub fn parse(name: &str, path: &str, bytes: &[u8]) -> Result<Self, TestError> {
        let format = Format::infer(path);
        let value = parse_value(format, bytes).map_err(|reason| TestError::CaseParse {
            path: path.to_string(),
            format: format!("{format:?}").to_lowercase(),
            reason,
        })?;

        let shape = |reason: String| TestError::CaseShape {
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
            if key != "answers" && key != "expect" {
                let hint = closest(key, ["answers", "expect"])
                    .map(|near| format!(" Did you mean `{near}`?"))
                    .unwrap_or_default();
                return Err(shape(format!(
                    "`{key}` is not a test case key.{hint} A case has `answers` and `expect`."
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

        Ok(Case {
            name: name.to_string(),
            path: path.to_string(),
            answers,
            expect,
        })
    }
}

impl Expect {
    fn parse(
        table: &BTreeMap<String, Value>,
        shape: &impl Fn(String) -> TestError,
    ) -> Result<Self, TestError> {
        for key in table.keys() {
            if !matches!(key.as_str(), "files" | "absent" | "contains" | "error") {
                let hint = closest(key, ["files", "absent", "contains", "error"])
                    .map(|near| format!(" Did you mean `{near}`?"))
                    .unwrap_or_default();
                return Err(shape(format!(
                    "`expect.{key}` is not an expectation.{hint} \
                     A case may expect `files`, `absent`, `contains` or `error`."
                )));
            }
        }

        let files = string_array(table.get("files"), "expect.files", shape)?;
        let absent = string_array(table.get("absent"), "expect.absent", shape)?;

        let contains = match table.get("contains") {
            None => BTreeMap::new(),
            Some(Value::Table(entries)) => {
                let mut out = BTreeMap::new();
                for (path, value) in entries {
                    // A bare string as well as an array, because
                    // `"a.toml" = 'name = "x"'` is what people write and
                    // refusing it would teach nothing.
                    let needles = match value {
                        Value::String(needle) => vec![needle.clone()],
                        other => string_array(
                            Some(other),
                            &format!("expect.contains.\"{path}\""),
                            shape,
                        )?,
                    };
                    out.insert(path.clone(), needles);
                }
                out
            }
            Some(other) => {
                return Err(shape(format!(
                    "`expect.contains` must be a table of path to expected text, not {}.",
                    other.type_name()
                )));
            }
        };

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
        if error.is_some() && (!files.is_empty() || !absent.is_empty() || !contains.is_empty()) {
            return Err(shape(
                "`expect.error` says the render fails, so there is no rendering for \
                 `files`, `absent` or `contains` to describe. Split them into two cases."
                    .to_string(),
            ));
        }

        Ok(Expect {
            files,
            absent,
            contains,
            error,
        })
    }
}

fn string_array(
    value: Option<&Value>,
    what: &str,
    shape: &impl Fn(String) -> TestError,
) -> Result<Vec<String>, TestError> {
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
    /// No snapshot is recorded, and `--write` was not given.
    ///
    /// Not a failure: snapshots are opt-in per case, so a template with three
    /// cases and one snapshot is a normal state.
    None,
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
}

impl CaseOutcome {
    /// Whether every expectation was met.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The result of a whole run.
pub struct Report {
    /// The template, resolved once for every case.
    pub template: Resolved,
    /// The tests directory that was read, relative to the template root.
    pub tests_dir: String,
    /// One per case, in name order.
    pub cases: Vec<CaseOutcome>,
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
}

/// Run a template's test cases.
///
/// The template is resolved **once**, so a report saying "12 cases at abc1234"
/// is telling the truth even if the branch moved mid-run — and a remote
/// template is cloned once rather than once per case.
pub fn run(
    target: Target<'_>,
    tests_dir: Option<&str>,
    filter: &[String],
    write: bool,
    user: &UserConfig,
    mut trust: Trust<'_>,
) -> Result<Report, OpError> {
    // Before anything is resolved or rendered: `--write` on a source with no
    // working tree cannot succeed, and finding that out after twelve renders
    // wastes the user's time. The same locality rule `--dirty` uses, so the two
    // flags cannot disagree about what "local" means.
    if write && resolve::local_path(target.source).is_none() {
        return Err(TestError::WriteNeedsLocal {
            origin: target.source.to_string(),
        }
        .into());
    }

    let template = resolve::resolve(super::Request {
        source: target.source,
        reference: target.reference,
        root: target.root,
        dirty: target.dirty,
    })?;

    let tests_dir = tests_dir.unwrap_or(DEFAULT_TESTS_DIR).trim_end_matches('/');
    let cases = discover(&template, tests_dir, filter)?;

    // Confirmed once for the whole run rather than once per case. The consent
    // being asked for is "may this template reach these hosts?", and the answer
    // does not change between two answer sets — asking twelve times would train
    // the user to say yes without reading.
    //
    // The decisions are replayed rather than collapsed to "allowed": a source
    // the user *refused* has to stay refused for every case, and a gate that
    // replayed a yes would turn one refusal into an allowance.
    let requests = declared_remotes(&template.manifest.data);
    let decisions: BTreeMap<String, Decision> = if requests.is_empty() {
        BTreeMap::new()
    } else {
        trust.gate().confirm(&requests, REMOTE_LIMIT_BYTES)?
    };

    let mut outcomes = Vec::with_capacity(cases.len());
    for case in &cases {
        outcomes.push(run_case(
            &template,
            target.source,
            tests_dir,
            case,
            write,
            user,
            &decisions,
        )?);
    }

    Ok(Report {
        template,
        tests_dir: tests_dir.to_string(),
        cases: outcomes,
    })
}

/// Read the cases out of the resolved tree.
///
/// From the tree and not the filesystem, so `--ref v1` runs *that tag's* cases
/// and `--dirty` runs the uncommitted ones — the same meaning those flags have
/// everywhere else. `--dirty` needs no special case here because
/// `tree_from_workdir` has already built a synthetic tree of the working
/// directory.
fn discover(template: &Resolved, tests_dir: &str, filter: &[String]) -> Result<Vec<Case>, OpError> {
    // `Resolved.tree` is the repository root, not `root_tree`: the tests
    // directory is outside the render root, exactly like the manifest and the
    // partials.
    let Some(dir) = template.repo.subtree(template.tree, tests_dir)? else {
        return Err(TestError::NoTests {
            dir: tests_dir.to_string(),
            revision: super::describe_revision(&template.reference, template.revision),
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
            return Err(TestError::CaseShape {
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
        return Err(TestError::NoTests {
            dir: tests_dir.to_string(),
            revision: super::describe_revision(&template.reference, template.revision),
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
            return Err(TestError::NoSuchCase {
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

#[allow(clippy::too_many_arguments)]
fn run_case(
    template: &Resolved,
    source: &str,
    tests_dir: &str,
    case: &Case,
    write: bool,
    user: &UserConfig,
    decisions: &BTreeMap<String, Decision>,
) -> Result<CaseOutcome, OpError> {
    // Always defaults, never a prompt: there is nobody to ask in CI, and a
    // prompt in a test runner is a hang. A question the case leaves unanswered
    // with no default fails as `tpl::eval::unanswered`, which is a true
    // statement about the template.
    let rendered = render_resolved(
        template,
        source,
        None,
        case.answers.clone(),
        user,
        Answering::defaults(),
        // Replaying what was confirmed once in `run`, before the first case.
        Trust::decided(decisions.clone()),
    );

    let mut failures = Vec::new();
    let mut snapshot = SnapshotOutcome::None;
    let mut files = 0;

    match rendered {
        Err(error) => {
            let codes = codes(&error);
            match &case.expect.error {
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
        Ok(RenderedOnce {
            files: rendered, ..
        }) => {
            files = rendered.len();

            if let Some(expected) = &case.expect.error {
                failures.push(Failure::ExpectedError {
                    code: expected.clone(),
                });
            } else {
                check(&case.expect, &rendered, &mut failures);
            }

            snapshot = snapshot_step(template, tests_dir, case, &rendered, write, &mut failures)?;
        }
    }

    Ok(CaseOutcome {
        name: case.name.clone(),
        path: case.path.clone(),
        failures,
        snapshot,
        files,
    })
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
fn snapshot_step(
    template: &Resolved,
    tests_dir: &str,
    case: &Case,
    rendered: &[Rendered],
    write: bool,
    failures: &mut Vec<Failure>,
) -> Result<SnapshotOutcome, OpError> {
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
            // Snapshots are opt-in per case. Failing a case that never asked
            // for one would force `--write` on people who only wanted
            // `expect.files`.
            return Ok(SnapshotOutcome::None);
        };
        let changes = compare(&recorded, &current);
        if !changes.is_empty() {
            failures.push(Failure::SnapshotDiff { changes });
        }
        return Ok(SnapshotOutcome::Compared);
    }

    // Compared *before* writing, so `--write` on a green suite says "unchanged"
    // rather than claiming to have rewritten twelve files that a reviewer would
    // then have to check.
    let outcome = match &recorded {
        None => SnapshotOutcome::Written,
        Some(recorded) if compare(recorded, &current).is_empty() => SnapshotOutcome::Unchanged,
        Some(_) => SnapshotOutcome::Updated,
    };

    if outcome != SnapshotOutcome::Unchanged {
        write_snapshot(template, tests_dir, case, &current)?;
    }

    Ok(outcome)
}

/// Read a recorded snapshot out of the resolved tree.
///
/// From the tree, like the cases themselves, so `--ref v1.2.0` compares against
/// that tag's snapshots. Reading them off the filesystem would make `--ref` a
/// lie.
fn read_snapshot(
    template: &Resolved,
    tests_dir: &str,
    case: &Case,
) -> Result<Option<BTreeMap<String, SnapshotEntry>>, OpError> {
    let dir = snapshot_path(tests_dir, &case.name);
    let Some(tree) = template.repo.subtree(template.tree, &dir)? else {
        return Ok(None);
    };

    let unreadable = |reason: String| TestError::SnapshotRead {
        case: case.name.clone(),
        path: dir.clone(),
        reason,
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
    unreadable: &impl Fn(String) -> TestError,
) -> Result<BTreeMap<String, RecordedFile>, TestError> {
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

    let failed = |path: &Path, error: &std::io::Error| TestError::SnapshotWrite {
        case: case.name.clone(),
        path: path.display().to_string(),
        reason: error.to_string(),
    };

    // Cleared, not merged into. A template that stops producing a file has to
    // be seen to stop; leaving the old one behind would let an author conclude
    // their conditional works.
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failed(&dir, &error).into()),
    }
    std::fs::create_dir_all(&dir).map_err(|error| failed(&dir, &error))?;

    for (path, entry) in files {
        // Safe to join: every rendered path was validated by `render_path`,
        // which rejects `.`, `..` and absolute segments.
        let target = dir.join(SNAPSHOT_FILES).join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| failed(parent, &error))?;
        }
        std::fs::write(&target, &entry.content).map_err(|error| failed(&target, &error))?;
        set_executable(&target, entry.executable).map_err(|error| failed(&target, &error))?;
    }

    let manifest = dir.join(SNAPSHOT_MANIFEST);
    std::fs::write(&manifest, render_manifest(&case.name, files))
        .map_err(|error| failed(&manifest, &error))?;

    Ok(())
}

/// Apply the executable bit, on the platforms that have one.
///
/// Git records nothing else about permissions, so this is the whole of what a
/// snapshot has to reproduce. On Windows there is no bit to set, which is
/// exactly why the manifest records the mode separately.
fn set_executable(path: &Path, executable: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        let mode = permissions.mode();
        permissions.set_mode(if executable {
            mode | 0o111
        } else {
            mode & !0o111
        });
        std::fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, executable);
    }
    Ok(())
}

/// The working-tree directory a case's snapshot is written to.
///
/// Exposed so the command can name it in output without rebuilding the path and
/// coming to disagree with what was written.
pub fn snapshot_dir(workdir: &Path, tests_dir: &str, case: &str) -> PathBuf {
    workdir.join(snapshot_path(tests_dir, case))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(body: &str) -> Result<Case, TestError> {
        Case::parse("c", "tests/c.toml", body.as_bytes())
    }

    fn shape_reason(result: Result<Case, TestError>) -> String {
        match result {
            Err(TestError::CaseShape { reason, .. }) => reason,
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
    fn contains_accepts_a_bare_string_as_well_as_an_array() {
        let parsed =
            case("[expect.contains]\n\"a.toml\" = \"x\"\n\"b.toml\" = [\"y\", \"z\"]\n").unwrap();
        assert_eq!(parsed.expect.contains["a.toml"], vec!["x"]);
        assert_eq!(parsed.expect.contains["b.toml"], vec!["y", "z"]);
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
    fn files_must_be_strings() {
        let reason = shape_reason(case("[expect]\nfiles = [1]\n"));
        assert!(reason.contains("must contain strings"), "{reason}");
    }

    #[test]
    fn a_case_that_is_not_a_table_is_refused() {
        let result = Case::parse("c", "tests/c.json", b"[1, 2]");
        assert!(matches!(result, Err(TestError::CaseShape { .. })));
    }

    #[test]
    fn a_case_that_does_not_parse_names_the_format() {
        match Case::parse("c", "tests/c.json", b"{not json") {
            Err(TestError::CaseParse { format, .. }) => assert_eq!(format, "json"),
            other => panic!("expected a parse error, got {other:?}"),
        }
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
        let parsed = parse_manifest(rendered.as_bytes(), &|reason| TestError::SnapshotRead {
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

    #[test]
    fn a_binary_change_carries_no_patch() {
        let changes = compare(
            &BTreeMap::from([("logo.png".to_string(), entry(&[0, 1, 2], false))]),
            &BTreeMap::from([("logo.png".to_string(), entry(&[0, 9, 9], false))]),
        );
        assert_eq!(changes[0].kind, ChangeKind::Modified);
        assert!(changes[0].patch.is_none(), "no patch for binary");
    }
}
