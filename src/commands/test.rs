//! `git tpl test`
//!
//! Runs the cases a template carries. No project, no ref, and — beyond
//! `--write` recording a snapshot — nothing written anywhere.

use std::io::Write as _;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use tpl::git::GitError;
use tpl::git::libgit2::LibGit2;
use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::testing::{CaseOutcome, Failure, Progress, Report, SnapshotOutcome, Status, Stream};
use tpl::ops::{self, OpError, Target};

use super::{Standalone, report_ignored_paths};
use crate::cli::{GlobalArgs, TestArgs};
use crate::theme::{Theme, field, heading, muted, warning};

pub fn run(args: TestArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Standalone::new(global)?;
    let source = ctx.user.expand(&args.template).into_owned();
    let run_commands = test_commands_enabled(args.skip_commands)?;

    // Chosen once, up front: `--quiet`/`--json` get nothing, `-v` gets a
    // scrolling log with live command output, a real terminal otherwise gets
    // a spinner, and anything else (piped, in a CI log) gets plain lines.
    let mut progress = TestProgress::new(&ctx, global.verbose > 0);
    let report = ops::testing::run(
        Target {
            source: &source,
            reference: args.r#ref.as_deref(),
            root: None,
            // Dirty unless `--ref` names a committed revision to check
            // instead: testing is for what is in front of you right now, not
            // what was last committed.
            dirty: args.r#ref.is_none(),
        },
        args.tests.as_deref(),
        &args.cases,
        args.write,
        run_commands,
        &ctx.user,
        // Told to `[commands]` children so a colour-aware tool does not
        // silently mute itself just because its stdout/stderr are pipes —
        // never on `--color=never`/`NO_COLOR`, since that already decided
        // `is_colored()` to be false.
        ctx.out.theme.is_colored(),
        &mut progress,
    )?;
    // Clears any spinner before the final report prints, so its last
    // message does not linger above it.
    progress.finish();

    report_ignored_paths(&ctx.out, &report.template.ignored);

    if global.json {
        println!("{}", crate::report::success(json(&report)));
    } else {
        print_text(&ctx, &report, args.write);
    }

    // A failing test is a failure, not something outstanding: `exit::PENDING`
    // means "nothing failed, but something is unmerged" and belongs to `status`.
    Ok(if report.is_failure() {
        crate::exit::FAILURE
    } else {
        crate::exit::SUCCESS
    })
}

/// How `git tpl test` reports what is happening while it happens.
///
/// [`tpl::ops::testing::Progress`] knows nothing about a terminal — that is
/// the whole point of the trait, see its own doc — so this is the one
/// implementation, chosen once per invocation in [`TestProgress::new`]
/// depending on `--quiet`/`--json`, `-v`, and whether stderr is a real
/// terminal.
enum TestProgress {
    /// `--quiet` or `--json`: nothing to show.
    Silent,
    /// The default, on a real terminal: one line, updated in place.
    Spinner(ProgressBar),
    /// Piped, or `-v`: one printed line per event.
    Line {
        /// Cloned rather than borrowed: `Standalone` outlives this value, but
        /// borrowing it would tie `TestProgress` to a lifetime for no benefit
        /// — a `Theme` is cheap to clone and never changes mid-run.
        theme: Theme,
        /// Whether a running command's own stdout/stderr is also forwarded,
        /// live, as it is produced. `false` for the plain-lines fallback: a
        /// non-tty, non-verbose run shows *which* command runs, not what it
        /// prints — that is what `-v` adds.
        verbose: bool,
    },
}

impl TestProgress {
    fn new(ctx: &Standalone, verbose: bool) -> Self {
        if !ctx.out.speaks() {
            return Self::Silent;
        }
        if verbose {
            return Self::Line {
                theme: ctx.out.theme.clone(),
                verbose: true,
            };
        }
        if console::user_attended_stderr() {
            let bar = ProgressBar::new_spinner();
            bar.set_draw_target(ProgressDrawTarget::stderr());
            bar.enable_steady_tick(Duration::from_millis(100));
            // A literal template, checked once here rather than at every
            // tick: `unwrap` is the right tool for a string that cannot come
            // from anywhere but this line.
            bar.set_style(ProgressStyle::with_template("{spinner} {msg}").unwrap());
            return Self::Spinner(bar);
        }
        Self::Line {
            theme: ctx.out.theme.clone(),
            verbose: false,
        }
    }

    /// Clear the spinner, if any, so its last message does not linger above
    /// the final report. A no-op for every other variant, which never drew
    /// anything that needs clearing.
    fn finish(&self) {
        if let Self::Spinner(bar) = self {
            bar.finish_and_clear();
        }
    }
}

/// What a phase looks like on one line, shared by the spinner and the plain
/// renderer so the two describe a case identically.
fn status_text(status: &Status<'_>) -> String {
    match status {
        Status::Rendering => "rendering".to_string(),
        Status::Command { step, command } => format!("[{}] $ {command}", step.as_str()),
        Status::Snapshot => "checking snapshot".to_string(),
    }
}

impl Progress for TestProgress {
    fn case_started(&mut self, name: &str) {
        match self {
            Self::Silent => {}
            Self::Spinner(bar) => bar.set_message(format!("{name} …")),
            Self::Line { theme, .. } => eprintln!("{}", muted(theme, &format!("{name} …"))),
        }
    }

    fn case_status(&mut self, name: &str, status: Status<'_>) {
        let text = status_text(&status);
        match self {
            Self::Silent => {}
            Self::Spinner(bar) => bar.set_message(format!("{name} — {text}")),
            Self::Line { theme, .. } => {
                eprintln!("{}", muted(theme, &format!("  {name} — {text}")));
            }
        }
    }

    fn command_output(&mut self, _name: &str, _stream: Stream, chunk: &[u8]) {
        // Raw, not reformatted: a chunk may cut a line or a multi-byte
        // sequence mid-way, and writing it straight through is what keeps an
        // embedded ANSI escape intact. Only under `-v` — the spinner and the
        // plain fallback both already say *which* command is running; this
        // is what it printed while doing so.
        if let Self::Line { verbose: true, .. } = self {
            let _ = std::io::stderr().write_all(chunk);
        }
    }

    fn case_finished(&mut self, _outcome: &CaseOutcome) {
        // Nothing to do: the next `case_started`/`case_status` overwrites the
        // spinner's message, [`TestProgress::finish`] clears it once the
        // whole run ends, and the `Line` variants already printed everything
        // they will for this case.
    }
}

/// Whether this run's `[commands]` execute at all. See ADR-027.
///
/// Reads `tpl.testCommands` from whatever repository contains the current
/// directory — never `Resolved.repo`, the template under test, which may be
/// a different local checkout entirely (`TEMPLATE` need not be `.`). A
/// personal opt-out has to live somewhere the person running the command
/// controls: their own machine's Git configuration, not a repository they
/// are merely pointing a test run at.
///
/// A missing repository here falls back to the built-in default silently:
/// running `test` from outside any repository at all is fine as long as
/// `TEMPLATE` names one — `resolve::resolve` is what actually enforces that,
/// and this function must not fail first over configuration nobody asked
/// about.
fn test_commands_enabled(skip: bool) -> Result<bool, OpError> {
    let preferences = match LibGit2::discover(&super::current_dir()?) {
        Ok(repo) => Preferences::load(&repo)?,
        Err(GitError::NotARepository { .. }) => Preferences::default(),
        Err(other) => return Err(other.into()),
    };
    Ok(preferences
        .with_overrides(Overrides {
            skip_commands: skip,
            ..Default::default()
        })
        .test_commands)
}

fn print_text(ctx: &Standalone, report: &Report, write: bool) {
    let theme = &ctx.out.theme;

    ctx.out.blank();
    ctx.out
        .say(field(theme, "Template", &report.template.manifest.name));
    ctx.out.say(field(
        theme,
        "Revision",
        &ops::describe_revision(&report.template.reference, report.template.revision),
    ));
    ctx.out.say(field(
        theme,
        "Tests",
        &format!(
            "{}/ — {} case{}",
            report.tests_dir,
            report.cases.len(),
            if report.cases.len() == 1 { "" } else { "s" }
        ),
    ));
    ctx.out.blank();

    for case in &report.cases {
        // The name column is as wide as the longest name, so the detail beside
        // it lines up. Computed over the run rather than fixed, because a
        // constant would either truncate or waste half the terminal.
        let width = report
            .cases
            .iter()
            .map(|case| case.name.len())
            .max()
            .unwrap_or(0);
        print_case(ctx, case, width);
    }

    ctx.out.blank();
    let summary = format!("{} passed, {} failed", report.passed(), report.failed());
    if report.is_failure() {
        ctx.out.say(warning(theme, &summary));
    } else {
        ctx.out.say(muted(theme, &summary));
    }
    if write {
        ctx.out.say(muted(
            theme,
            &format!(
                "{} snapshot(s) recorded, {} unchanged",
                report.snapshots_written(),
                report
                    .cases
                    .iter()
                    .filter(|case| case.snapshot == SnapshotOutcome::Unchanged)
                    .count()
            ),
        ));
    }
}

fn print_case(ctx: &Standalone, case: &CaseOutcome, width: usize) {
    let theme = &ctx.out.theme;

    // Padded to a common width so the case names line up: a column a reader
    // scans is worth more than the two characters it costs.
    let status = if case.passed() { "ok    " } else { "FAILED" };

    if !case.passed() {
        ctx.out
            .say(heading(theme, &format!("  {status}  {}", case.name)));
        for failure in &case.failures {
            print_failure(ctx, failure);
        }
        ctx.out.blank();
        return;
    }

    let mut detail = vec![format!(
        "{} file{}",
        case.files,
        if case.files == 1 { "" } else { "s" }
    )];
    if case.commands_run > 0 {
        detail.push(format!(
            "{} command{}",
            case.commands_run,
            if case.commands_run == 1 { "" } else { "s" }
        ));
    }
    match case.snapshot {
        SnapshotOutcome::None => {}
        SnapshotOutcome::Compared => detail.push("snapshot ok".into()),
        SnapshotOutcome::Written => detail.push("snapshot written".into()),
        SnapshotOutcome::Updated => detail.push("snapshot updated".into()),
        SnapshotOutcome::Unchanged => detail.push("snapshot unchanged".into()),
    }
    ctx.out.say(muted(
        theme,
        &format!("  {status}  {:width$}   {}", case.name, detail.join(", ")),
    ));
}

fn print_failure(ctx: &Standalone, failure: &Failure) {
    let theme = &ctx.out.theme;
    let say = |text: String| ctx.out.say(format!("    {text}"));

    match failure {
        Failure::MissingFile { path, closest } => {
            say(format!("missing file      {path}"));
            if let Some(near) = closest {
                ctx.out.say(muted(
                    theme,
                    &format!("      the template rendered `{near}`"),
                ));
            }
        }
        Failure::UnexpectedFile { path } => say(format!("unexpected file   {path}")),
        Failure::ContainsMissingFile { path } => {
            say(format!("missing file      {path} (named by `contains`)"));
        }
        Failure::ContainsMissing { path, needle } => {
            say(format!("`{path}` does not contain: {needle}"));
        }
        Failure::ContainsNotUtf8 { path } => {
            say(format!(
                "`{path}` is not text, so `contains` cannot look in it"
            ));
        }
        Failure::LacksMissingFile { path } => {
            say(format!("missing file      {path} (named by `lacks`)"));
        }
        Failure::LacksPresent { path, needle } => {
            say(format!("`{path}` contains: {needle}"));
        }
        Failure::LacksNotUtf8 { path } => {
            say(format!(
                "`{path}` is not text, so `lacks` cannot look in it"
            ));
        }
        Failure::ExpectedError { code } => {
            say(format!(
                "expected the render to fail with {code}, but it succeeded"
            ));
        }
        Failure::UnexpectedError { code, message } => {
            say(format!(
                "the render failed: {message}{}",
                code.as_deref()
                    .map(|code| format!(" [{code}]"))
                    .unwrap_or_default()
            ));
            if let Some(code) = code {
                ctx.out.say(muted(
                    theme,
                    &format!("      add `error = \"{code}\"` if that is the point of the case"),
                ));
            }
        }
        Failure::WrongError {
            expected,
            actual,
            message,
        } => {
            say(format!("expected {expected}, got {}", actual.join(" → ")));
            ctx.out.say(muted(theme, &format!("      {message}")));
        }
        Failure::SnapshotDiff { changes } => {
            say(format!(
                "snapshot differs ({} file{})",
                changes.len(),
                if changes.len() == 1 { "" } else { "s" }
            ));
            for change in changes {
                let note = if change.mode_only { " (mode)" } else { "" };
                ctx.out.say(muted(
                    theme,
                    &format!("      {} {}{note}", change.kind.label(), change.path),
                ));
                // Hunks only when asked for. A suite with a large rendering
                // would otherwise bury the list of what changed under the
                // change itself.
                if ctx.out.global.verbose > 0
                    && let Some(patch) = &change.patch
                {
                    for line in patch.lines() {
                        ctx.out.say(muted(theme, &format!("        {line}")));
                    }
                }
            }
            ctx.out.say(muted(
                theme,
                "      re-record with `git tpl test --write` once the change is intended",
            ));
        }
        Failure::SnapshotMissing => {
            say("snapshot requested but never recorded".to_string());
            ctx.out
                .say(muted(theme, "      record one with `git tpl test --write`"));
        }
        Failure::CommandFailed {
            step,
            command,
            code,
            stdout,
            stderr,
        } => {
            match code {
                Some(code) => say(format!("[{}] `{command}` exited {code}", step.as_str())),
                None => say(format!("[{}] `{command}` could not be run", step.as_str())),
            }
            // Under `-v` this was already shown live, byte for byte, as the
            // command produced it — repeating the captured (lossily
            // converted, tail-capped) copy here would only be a worse
            // version of what the user already watched happen.
            if ctx.out.global.verbose == 0 {
                // stderr first: it is where a failing command explains
                // itself. stdout only when there is nothing on stderr to
                // show instead.
                let output = if stderr.is_empty() { stdout } else { stderr };
                for line in output.lines() {
                    ctx.out.say(muted(theme, &format!("      {line}")));
                }
            }
        }
    }
}

/// The machine-readable form.
///
/// `ok` is `true` whenever the *command* ran; whether the suite passed is
/// `summary.failed` and the exit code. The same split `lint` uses — findings in
/// the payload, verdict in the status — and it must stay that way, or a caller
/// cannot tell "three cases failed" from "the template could not be resolved".
fn json(report: &Report) -> serde_json::Value {
    serde_json::json!({
        "template": {
            "name": report.template.manifest.name,
            "description": report.template.manifest.description,
        },
        "revision": {
            "reference": report.template.reference,
            "commit": report.template.revision.to_hex(),
            "dirty": report.template.dirty,
        },
        "tests": report.tests_dir,
        "summary": {
            "total": report.cases.len(),
            "passed": report.passed(),
            "failed": report.failed(),
            "snapshotsWritten": report.snapshots_written(),
            "snapshotsCompared": report.snapshots_compared(),
            "commandsEnabled": report.commands_enabled,
            "commandsRun": report.commands_run(),
        },
        "cases": report.cases.iter().map(|case| serde_json::json!({
            "name": case.name,
            "path": case.path,
            "passed": case.passed(),
            "files": case.files,
            "snapshot": case.snapshot.as_str(),
            "commandsRun": case.commands_run,
            "failures": case.failures.iter().map(failure_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// One failure, as JSON.
///
/// `kind` is the stable discriminator, in camelCase like every other key we
/// emit. Renaming one is a breaking change.
fn failure_json(failure: &Failure) -> serde_json::Value {
    match failure {
        Failure::MissingFile { path, closest } => serde_json::json!({
            "kind": "missingFile", "path": path, "closest": closest,
        }),
        Failure::UnexpectedFile { path } => serde_json::json!({
            "kind": "unexpectedFile", "path": path,
        }),
        Failure::ContainsMissingFile { path } => serde_json::json!({
            "kind": "containsMissingFile", "path": path,
        }),
        Failure::ContainsMissing { path, needle } => serde_json::json!({
            "kind": "containsMissing", "path": path, "needle": needle,
        }),
        Failure::ContainsNotUtf8 { path } => serde_json::json!({
            "kind": "containsNotUtf8", "path": path,
        }),
        Failure::LacksMissingFile { path } => serde_json::json!({
            "kind": "lacksMissingFile", "path": path,
        }),
        Failure::LacksPresent { path, needle } => serde_json::json!({
            "kind": "lacksPresent", "path": path, "needle": needle,
        }),
        Failure::LacksNotUtf8 { path } => serde_json::json!({
            "kind": "lacksNotUtf8", "path": path,
        }),
        Failure::ExpectedError { code } => serde_json::json!({
            "kind": "expectedError", "expected": code,
        }),
        Failure::UnexpectedError { code, message } => serde_json::json!({
            "kind": "unexpectedError", "code": code, "message": message,
        }),
        Failure::WrongError {
            expected,
            actual,
            message,
        } => serde_json::json!({
            "kind": "wrongError", "expected": expected,
            "actual": actual, "message": message,
        }),
        Failure::SnapshotDiff { changes } => serde_json::json!({
            "kind": "snapshotDiff",
            "changes": changes.iter().map(|change| serde_json::json!({
                "path": change.path,
                "kind": change.kind.as_str(),
                "modeOnly": change.mode_only,
                "patch": change.patch,
            })).collect::<Vec<_>>(),
        }),
        Failure::SnapshotMissing => serde_json::json!({
            "kind": "snapshotMissing",
        }),
        Failure::CommandFailed {
            step,
            command,
            code,
            stdout,
            stderr,
        } => serde_json::json!({
            "kind": "commandFailed",
            "step": step.as_str(),
            "command": command,
            "code": code,
            "stdout": stdout,
            "stderr": stderr,
        }),
    }
}
