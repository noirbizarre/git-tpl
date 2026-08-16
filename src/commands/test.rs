//! `git tpl test`
//!
//! Runs the cases a template carries. No project, no ref, and — beyond
//! `--write` recording a snapshot — nothing written anywhere.

use tpl::ops::testing::{CaseOutcome, Failure, Report, SnapshotOutcome};
use tpl::ops::{self, OpError, Target};

use super::Standalone;
use crate::cli::{GlobalArgs, TestArgs};
use crate::prompt::Confirmer;
use crate::theme::{field, heading, muted, warning};

pub fn run(args: TestArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Standalone::new(global)?;
    let source = ctx.user.expand(&args.template).into_owned();

    let mut confirmer = Confirmer;
    let report = ops::testing::run(
        Target {
            source: &source,
            reference: args.r#ref.as_deref(),
            root: args.root.as_deref(),
            dirty: args.dirty,
        },
        args.tests.as_deref(),
        &args.cases,
        args.write,
        &ctx.user,
        // The runner confirms once for the whole run, so this gate is consulted
        // at most once however many cases there are.
        trust_for(args.trust, &mut confirmer),
    )?;

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

/// The trust gate.
///
/// Built here rather than by [`super::trust`], which reads `--defaults` to
/// decide whether there is anybody to ask. `test` has no `--defaults`: it is
/// *always* non-interactive about questions, because a prompt in a test runner
/// is a hang. That says nothing about trust — a person running `git tpl test`
/// at a terminal can still answer "may this template reach that host?", and it
/// is asked once for the whole run.
fn trust_for(trusted: bool, confirmer: &mut Confirmer) -> ops::Trust<'_> {
    if trusted {
        ops::Trust::always()
    } else {
        ops::Trust::Ask(confirmer)
    }
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
        },
        "cases": report.cases.iter().map(|case| serde_json::json!({
            "name": case.name,
            "path": case.path,
            "passed": case.passed(),
            "files": case.files,
            "snapshot": case.snapshot.as_str(),
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
    }
}
