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
use tpl::ops::testing::{
    CaseOutcome, CommandStep, Failure, Progress, Report, SnapshotChange, SnapshotOutcome, Status,
    Stream,
};
use tpl::ops::{self, OpError, Target};

use super::{Standalone, report_ignored_paths};
use crate::cli::{GlobalArgs, TestArgs};
use crate::theme::{Theme, bold, field, heading, muted, patch_line};

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
    Spinner {
        bar: ProgressBar,
        /// Cloned rather than borrowed, for the same reason [`Line`](Self::Line)'s
        /// does: `Standalone` outlives this value, but borrowing it would tie
        /// `TestProgress` to a lifetime for no benefit.
        theme: Theme,
    },
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
        Self::choose(
            ctx.out.speaks(),
            verbose,
            console::user_attended_stderr(),
            ctx.out.theme.clone(),
        )
    }

    /// The dispatch itself, with every input a parameter rather than read
    /// from the environment — the same separation [`crate::theme::decide`]
    /// already uses for the same reason: a real terminal cannot be faked for
    /// a test, but a `bool` can, so the *decision* stays testable even though
    /// [`new`](Self::new) that supplies it is not.
    fn choose(speaks: bool, verbose: bool, is_terminal: bool, theme: Theme) -> Self {
        if !speaks {
            return Self::Silent;
        }
        if verbose {
            return Self::Line {
                theme,
                verbose: true,
            };
        }
        if is_terminal {
            let bar = ProgressBar::new_spinner();
            bar.set_draw_target(ProgressDrawTarget::stderr());
            bar.enable_steady_tick(Duration::from_millis(100));
            // A literal template, checked once here rather than at every
            // tick: `unwrap` is the right tool for a string that cannot come
            // from anywhere but this line. `.cyan.bold` so the glyph itself
            // reads as "in progress" even before a message is set — plain
            // `{spinner}` renders in whatever the terminal's default
            // foreground is, easy to miss beside a coloured report.
            bar.set_style(ProgressStyle::with_template("{spinner:.cyan.bold} {msg}").unwrap());
            return Self::Spinner { bar, theme };
        }
        Self::Line {
            theme,
            verbose: false,
        }
    }

    /// Clear the spinner, if any, so its last message does not linger above
    /// the final report. A no-op for every other variant, which never drew
    /// anything that needs clearing.
    fn finish(&self) {
        if let Self::Spinner { bar, .. } = self {
            bar.finish_and_clear();
        }
    }
}

/// `rendering`/`checking snapshot`, bracketed and dim: neither carries a
/// pass/fail meaning of its own the way a command's exit status does, so
/// unlike [`command_line`] there is nothing here to colour green or red.
fn phase_line(theme: &Theme, status: &Status<'_>) -> String {
    match status {
        Status::Rendering => muted(theme, "[rendering]"),
        Status::Snapshot => muted(theme, "[checking snapshot]"),
        Status::Command { step, command } => command_line(theme, *step, command, true),
    }
}

/// `[step] $ command`, coloured by whether it is known to have failed yet.
///
/// Green while only *about* to run — [`Progress::case_status`] always calls
/// this with `ok: true`, since nothing has failed yet — and while it in fact
/// succeeded; red once [`Progress::command_finished`] reports otherwise.
/// `$` is always the same bold white regardless of outcome: it marks "a
/// command follows", not a verdict.
fn command_line(theme: &Theme, step: CommandStep, command: &str, ok: bool) -> String {
    let step = format!("[{}]", step.as_str());
    let step = if ok {
        theme.added.apply_to(step).to_string()
    } else {
        theme.deleted.apply_to(step).to_string()
    };
    format!("{step} {} {command}", bold(theme, "$"))
}

/// `N label`, styled only when `count` has something to report.
///
/// A zero count styled the same as a positive one would read as an alarm
/// about nothing — the summary line's `0 failed` on an all-green run being
/// the case this exists for.
fn counted(style: &console::Style, count: usize, label: &str) -> String {
    let text = format!("{count} {label}");
    if count > 0 {
        style.apply_to(text).to_string()
    } else {
        text
    }
}

/// `✔ case` (green tick, bold white name) or `✘ case` (red cross, yellow
/// name) — printed once, permanently, above the still-running spinner (or as
/// a plain line, piped) rather than overwritten like [`phase_line`]'s.
fn case_summary(theme: &Theme, outcome: &CaseOutcome) -> String {
    if outcome.passed() {
        format!(
            "{} {}",
            theme.added.apply_to("✔"),
            bold(theme, &outcome.name)
        )
    } else {
        format!(
            "{} {}",
            theme.deleted.apply_to("✘"),
            theme.warning.apply_to(&outcome.name)
        )
    }
}

impl Progress for TestProgress {
    fn case_started(&mut self, name: &str) {
        match self {
            Self::Silent => {}
            Self::Spinner { bar, theme } => bar.set_message(format!("{} …", bold(theme, name))),
            Self::Line { theme, .. } => eprintln!("{} …", bold(theme, name)),
        }
    }

    fn case_status(&mut self, name: &str, status: Status<'_>) {
        match self {
            Self::Silent => {}
            Self::Spinner { bar, theme } => {
                bar.set_message(format!(
                    "{} {}",
                    bold(theme, name),
                    phase_line(theme, &status)
                ));
            }
            Self::Line { theme, verbose } => {
                // A command's "about to run" announcement only earns its
                // keep when something follows it that needs attributing to
                // a command — `-v`'s live raw output. Without `-v`,
                // `command_finished` reports the same command a moment
                // later with its verdict, so printing here too would say
                // the same thing twice for every command that succeeds
                // (`ok` is always `true` here, since nothing has failed
                // yet, so the two lines would be byte-for-byte identical)
                // — every CI log line doubled.
                if matches!(status, Status::Command { .. }) && !*verbose {
                    return;
                }
                eprintln!("  {} {}", bold(theme, name), phase_line(theme, &status));
            }
        }
    }

    fn command_finished(&mut self, name: &str, step: CommandStep, command: &str, ok: bool) {
        match self {
            Self::Silent => {}
            Self::Spinner { bar, theme } => bar.set_message(format!(
                "{} {}",
                bold(theme, name),
                command_line(theme, step, command, ok)
            )),
            Self::Line { theme, .. } => eprintln!(
                "  {} {}",
                bold(theme, name),
                command_line(theme, step, command, ok)
            ),
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

    fn case_finished(&mut self, outcome: &CaseOutcome) {
        match self {
            Self::Silent => {}
            // `println` rather than `set_message`: this is history, not the
            // current line, and must survive the next case overwriting the
            // spinner. indicatif prints it above the bar and redraws the bar
            // below it, so the spinner never stops ticking to make room.
            Self::Spinner { bar, theme } => bar.println(case_summary(theme, outcome)),
            Self::Line { theme, .. } => eprintln!("{}", case_summary(theme, outcome)),
        }
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
        Err(GitError::NotARepository { .. } | GitError::JujutsuNotColocated { .. }) => {
            Preferences::default()
        }
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
        print_case(ctx, case, width, write);
    }

    ctx.out.blank();
    ctx.out.say(format!(
        "{}, {}",
        counted(&theme.added, report.passed(), "passed"),
        counted(&theme.deleted, report.failed(), "failed"),
    ));
    if write {
        // Written vs updated vs unchanged, not the "recorded" total ADR-016
        // used to report as one number: a reviewer cares differently about a
        // brand-new snapshot than a changed one, and `-v` below needs the
        // three buckets kept apart to name which case is in which. See
        // ADR-032.
        let written = report
            .cases
            .iter()
            .filter(|case| case.snapshot == SnapshotOutcome::Written)
            .count();
        let updated = report
            .cases
            .iter()
            .filter(|case| case.snapshot == SnapshotOutcome::Updated)
            .count();
        let unchanged = report
            .cases
            .iter()
            .filter(|case| case.snapshot == SnapshotOutcome::Unchanged)
            .count();
        let skipped = report
            .cases
            .iter()
            .filter(|case| case.snapshot == SnapshotOutcome::Skipped)
            .count();

        // Coloured the same way the passed/failed line above it is: `added`
        // for a brand-new snapshot, `modified` for a changed one — the same
        // colour `ChangeKind::Modified` already gets everywhere else —
        // neither alarming for `unchanged`, so it stays `muted`.
        ctx.out.say(format!(
            "{}, {}, {}",
            counted(&theme.added, written, "written"),
            counted(&theme.modified, updated, "updated"),
            counted(&theme.muted, unchanged, "unchanged"),
        ));
        if skipped > 0 {
            ctx.out.say(muted(
                theme,
                &format!("{skipped} skipped (no `snapshot = true`)"),
            ));
        }

        if ctx.out.global.verbose > 0 {
            let names = |outcome: SnapshotOutcome| -> String {
                report
                    .cases
                    .iter()
                    .filter(|case| case.snapshot == outcome)
                    .map(|case| case.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            if updated > 0 {
                ctx.out.say(muted(
                    theme,
                    &format!("  updated: {}", names(SnapshotOutcome::Updated)),
                ));
            }
            if written > 0 {
                ctx.out.say(muted(
                    theme,
                    &format!("  written: {}", names(SnapshotOutcome::Written)),
                ));
            }
        }
    }
}

fn print_case(ctx: &Standalone, case: &CaseOutcome, width: usize, write: bool) {
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

    // `--write` never rendered this case at all (ADR-032): reported on its
    // own, before the `files`/`commands` detail below, which would otherwise
    // read as "0 files" — a real, if misleading, number for a case that was
    // never touched rather than one that rendered nothing.
    if case.snapshot == SnapshotOutcome::Skipped {
        ctx.out.say(muted(
            theme,
            &format!(
                "  {status}  {:width$}   skipped (no `snapshot = true`)",
                case.name
            ),
        ));
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
        SnapshotOutcome::None | SnapshotOutcome::Skipped => {}
        SnapshotOutcome::Compared => detail.push("snapshot ok".into()),
        SnapshotOutcome::Written => detail.push("snapshot written".into()),
        SnapshotOutcome::Updated => detail.push("snapshot updated".into()),
        SnapshotOutcome::Unchanged => detail.push("snapshot unchanged".into()),
    }
    ctx.out.say(muted(
        theme,
        &format!("  {status}  {:width$}   {}", case.name, detail.join(", ")),
    ));

    // In place of the live `[commands]` output `--write` no longer produces
    // (ADR-032): the same unified diff, coloured the same way, that a normal
    // run's `Failure::SnapshotDiff` already shows under `-v`.
    if write && ctx.out.global.verbose > 0 && case.snapshot == SnapshotOutcome::Updated {
        for change in &case.snapshot_changes {
            let note = if change.mode_only { " (mode)" } else { "" };
            ctx.out.say(muted(
                theme,
                &format!("      {} {}{note}", change.kind.label(), change.path),
            ));
            if let Some(patch) = &change.patch {
                for line in patch.lines() {
                    ctx.out.say(format!("        {}", patch_line(theme, line)));
                }
            }
        }
    }
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
                        ctx.out.say(format!("        {}", patch_line(theme, line)));
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
        "revision": crate::report::revision(
            Some(&report.template.reference),
            Some(report.template.revision),
            Some(report.template.dirty),
        ),
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
            // Non-empty only when `snapshot` is `"updated"` — the same shape
            // `Failure::SnapshotDiff.changes` already uses, so a `--json
            // --write` caller can read the diff without a terminal to
            // colourise it for. See ADR-032.
            "snapshotChanges": case.snapshot_changes.iter().map(snapshot_change_json).collect::<Vec<_>>(),
            "commandsRun": case.commands_run,
            "failures": case.failures.iter().map(failure_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// One snapshot change, as JSON — shared by `Failure::SnapshotDiff` (a
/// compare-mode run) and a case's own `snapshotChanges` (a `--write` run
/// that updated one). Both name the same fact about the same pair of trees;
/// duplicating the shape would only let the two drift.
fn snapshot_change_json(change: &SnapshotChange) -> serde_json::Value {
    serde_json::json!({
        "path": change.path,
        "kind": change.kind.as_str(),
        "modeOnly": change.mode_only,
        "patch": change.patch,
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
            "changes": changes.iter().map(snapshot_change_json).collect::<Vec<_>>(),
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tpl::ops::testing::SnapshotOutcome;

    use super::*;

    /// A minimal outcome, passed or failed — enough to drive
    /// [`case_summary`] and [`Progress::case_finished`], which look only at
    /// `name` and `passed()`.
    fn outcome(name: &str, passed: bool) -> CaseOutcome {
        CaseOutcome {
            name: name.to_string(),
            path: format!("tests/{name}.toml"),
            failures: if passed {
                Vec::new()
            } else {
                vec![Failure::UnexpectedError {
                    code: None,
                    message: "boom".to_string(),
                }]
            },
            snapshot: SnapshotOutcome::None,
            files: 1,
            commands_run: 0,
            snapshot_changes: Vec::new(),
        }
    }

    /// The spinner's own message, or a panic naming which variant it was —
    /// every test using this constructs the spinner itself, so a mismatch
    /// here is this module's own bug, not something to report gracefully.
    fn spinner_message(progress: &TestProgress) -> String {
        match progress {
            TestProgress::Spinner { bar, .. } => bar.message(),
            other => panic!("expected a spinner, got {other:?}"),
        }
    }

    impl std::fmt::Debug for TestProgress {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Self::Silent => "Silent",
                Self::Spinner { .. } => "Spinner",
                Self::Line { .. } => "Line",
            })
        }
    }

    #[test]
    fn phase_line_names_rendering_and_snapshot_with_no_colour_of_their_own() {
        let theme = Theme::plain();
        assert_eq!(phase_line(&theme, &Status::Rendering), "[rendering]");
        assert_eq!(phase_line(&theme, &Status::Snapshot), "[checking snapshot]");
    }

    #[test]
    fn phase_line_delegates_a_running_command_to_command_line() {
        let theme = Theme::plain();
        let status = Status::Command {
            step: CommandStep::Rendered,
            command: "echo hi",
        };
        assert_eq!(
            phase_line(&theme, &status),
            command_line(&theme, CommandStep::Rendered, "echo hi", true)
        );
    }

    #[test]
    fn command_line_shows_the_step_the_dollar_and_the_command() {
        let theme = Theme::plain();
        assert_eq!(
            command_line(&theme, CommandStep::Before, "mkdir -p src", true),
            "[before] $ mkdir -p src"
        );
    }

    #[test]
    fn command_line_colours_the_step_differently_once_it_has_failed() {
        let theme = Theme::colored();
        let running = command_line(&theme, CommandStep::After, "true", true);
        let failed = command_line(&theme, CommandStep::After, "true", false);
        assert_ne!(running, failed, "{running:?} / {failed:?}");
        assert!(running.contains('\x1b'), "{running:?}");
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn case_summary_names_the_case_either_way(#[case] passed: bool) {
        let theme = Theme::plain();
        let line = case_summary(&theme, &outcome("basic", passed));
        assert!(line.contains("basic"), "{line}");
        assert_eq!(line.starts_with('✔'), passed, "{line}");
        assert_eq!(line.starts_with('✘'), !passed, "{line}");
    }

    #[test]
    fn counted_is_styled_only_when_positive() {
        let theme = Theme::colored();
        assert_eq!(counted(&theme.added, 0, "passed"), "0 passed");
        let styled = counted(&theme.added, 1, "passed");
        assert_ne!(styled, "1 passed", "{styled:?}");
        assert!(styled.contains('\x1b'), "{styled:?}");
    }

    #[rstest]
    // `--quiet`/`--json`: silent regardless of `-v` or a terminal.
    #[case(false, false, false, "Silent")]
    #[case(false, true, true, "Silent")]
    // `-v`: a scrolling log either way.
    #[case(true, true, false, "Line")]
    #[case(true, true, true, "Line")]
    // The default: a spinner only on a real terminal.
    #[case(true, false, true, "Spinner")]
    #[case(true, false, false, "Line")]
    fn choose_dispatches_on_speaks_verbose_and_terminal(
        #[case] speaks: bool,
        #[case] verbose: bool,
        #[case] is_terminal: bool,
        #[case] expected: &str,
    ) {
        let progress = TestProgress::choose(speaks, verbose, is_terminal, Theme::plain());
        assert_eq!(format!("{progress:?}"), expected);
    }

    #[test]
    fn a_spinner_reflects_every_progress_event_in_its_own_message() {
        let mut progress = TestProgress::choose(true, false, true, Theme::plain());

        progress.case_started("basic");
        assert!(spinner_message(&progress).contains("basic"));

        progress.case_status("basic", Status::Rendering);
        assert!(spinner_message(&progress).contains("[rendering]"));

        progress.command_finished("basic", CommandStep::Rendered, "true", true);
        assert!(spinner_message(&progress).contains("[rendered]"));

        // A no-op for the spinner: it says which command is running, not
        // what that command prints — calling it here only proves it does not
        // panic or change the message.
        let before = spinner_message(&progress);
        progress.command_output("basic", Stream::Stdout, b"noise\n");
        assert_eq!(spinner_message(&progress), before);

        // `case_finished` prints history above the bar; it must not touch
        // the bar's own current message.
        progress.case_finished(&outcome("basic", true));
        assert_eq!(spinner_message(&progress), before);

        // Clears the bar; exercised for its own sake; no further message to
        // assert on once finished.
        progress.finish();
    }

    #[test]
    fn silent_does_nothing_for_every_event() {
        // The point of this test is that none of the following panics —
        // `Silent` has no state for an assertion to inspect.
        let mut progress = TestProgress::Silent;
        progress.case_started("basic");
        progress.case_status("basic", Status::Rendering);
        progress.command_finished("basic", CommandStep::Before, "true", true);
        progress.command_output("basic", Stream::Stdout, b"noise\n");
        progress.case_finished(&outcome("basic", false));
        progress.finish();
    }

    #[test]
    fn a_verbose_line_accepts_every_event_including_raw_command_output() {
        // As with `Silent`: there is no in-process way to capture what a
        // child writes to the real stderr, so this proves the `-v` path
        // runs end to end without panicking rather than asserting on bytes
        // already covered by the integration suite's own `-v` runs.
        let mut progress = TestProgress::choose(true, true, false, Theme::plain());
        progress.case_started("basic");
        progress.case_status("basic", Status::Rendering);
        progress.command_finished("basic", CommandStep::Rendered, "true", true);
        progress.command_output("basic", Stream::Stdout, b"hello\n");
        progress.case_finished(&outcome("basic", true));
        progress.finish();
    }
}
