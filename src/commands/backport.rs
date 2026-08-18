//! `git tpl backport`

use std::io::IsTerminal;

use tpl::ops::{self, Backport, OpError};

use super::Session;
use crate::cli::{BackportArgs, GlobalArgs};
use crate::prompt::Confirmer;
use crate::theme::{command, diff_summary, field, headline, muted, warning};

pub fn run(args: BackportArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let preferences = tpl::gitconfig::Preferences::load(&ctx.repo)?;
    let mut confirmer = Confirmer;
    let mut reverser = Confirmer;

    // Never prompts in practice: the recorded answers cover every question the
    // recorded revision asks. `defaults()` is what happens if that stops being
    // true, and the tree check then refuses rather than silently backporting
    // against a rendering nobody has.
    //
    // It also settles backport's relationship with the seed context (ADR-018):
    // `DefaultsOnly` ignores a seed outright, so no part of this rendering can
    // vary with the directory name or the remote URL. The tree it produces is
    // the tree the ref records, on any machine — which is what the check
    // against the ref is entitled to assume.
    let answering = tpl::ops::Answering::defaults();
    let trust = if args.trust {
        tpl::ops::Trust::always()
    } else if preferences.interactive {
        tpl::ops::Trust::Ask(&mut confirmer)
    } else {
        tpl::ops::Trust::refuse()
    };

    // Reversing a substitution is the one thing `backport` does that a
    // round-trip cannot prove right for anyone but the person running it
    // (ADR-022), so by default it happens only when that person is there to
    // look at it. With nobody to ask — `--json`, a script, `tpl.interactive`
    // off — it is not attempted, and the line refuses exactly as it did before.
    // `--unsubstitute` is the decision taken in advance, and is how CI opts in.
    //
    // The tty check is here and nowhere else in the tree, on purpose.
    // Elsewhere `tpl.interactive` is enough, because a prompt that cannot run
    // fails the command and the user finds out. Here a failed prompt would read
    // as a refusal and silently shrink the patch, so "can I actually ask?" has
    // to be answered before anything is attempted rather than after.
    let unsubstitute = if args.unsubstitute {
        ops::Unsubstitute::Always
    } else if preferences.interactive && !global.json && std::io::stderr().is_terminal() {
        ops::Unsubstitute::Ask(&mut reverser)
    } else {
        ops::Unsubstitute::Never
    };

    let result = ops::backport(
        &ctx.repo,
        &ctx.root,
        &args.paths,
        &args.exclude,
        &ctx.user,
        answering,
        trust,
        unsubstitute,
    )?;

    // Written before anything is said, so a failure to write is not reported
    // after a line claiming success.
    if let Some(path) = &args.output
        && !result.patch.is_empty()
    {
        std::fs::write(path, &result.patch).map_err(|error| {
            OpError::Backport(ops::BackportError::OutputWrite {
                path: path.display().to_string(),
                reason: error.to_string(),
            })
        })?;
    }

    report(&ctx, &args, &result);

    if global.json {
        println!("{}", crate::report::success(payload(&args, &result)));
    } else if args.output.is_none() && !result.patch.is_empty() {
        // The patch is data, so it goes to stdout and nowhere else. Everything
        // above went to stderr precisely so that this stays pipeable straight
        // into `git am`.
        print!("{}", result.patch);
    }

    Ok(crate::exit::SUCCESS)
}

/// The prose, on stderr.
///
/// Built beside [`payload`] rather than from it: `--json` suppresses this text
/// but not the work behind it, and the two must not come to disagree about
/// what was backported (#53).
fn report(ctx: &Session, args: &BackportArgs, result: &Backport) {
    // Loud regardless of verbosity: a path that was considered and dropped is
    // something the user is getting wrong right now, and a silent omission
    // from a patch is discovered only by the template's maintainer.
    for skipped in &result.skipped {
        ctx.out.warn(warning(
            &ctx.out.theme,
            &format!("skipped {}: {}", skipped.path, skipped.reason),
        ));
    }

    if result.files.is_empty() {
        ctx.out.say(muted(
            &ctx.out.theme,
            "Nothing to backport: the project matches the template's rendering.",
        ));
        return;
    }

    ctx.out.say(headline(
        &ctx.out.theme,
        "backport",
        &result.revision_description,
    ));
    ctx.out.blank();
    for file in &result.files {
        ctx.out.say(muted(
            &ctx.out.theme,
            &format!("  {} <- {}", file.source, file.rendered),
        ));
    }
    ctx.out.blank();
    // Loud, and above the summary. A reversed substitution changes what the
    // template produces for every project, so it is the one part of a patch
    // that must not be skimmed past on the way to the `apply:` line.
    for reversal in &result.unsubstituted {
        ctx.out.warn(warning(
            &ctx.out.theme,
            &format!(
                "un-substituted {} line {}: {}",
                reversal.path, reversal.line, reversal.patched
            ),
        ));
    }
    ctx.out.say(muted(
        &ctx.out.theme,
        &diff_summary(
            result.files.len(),
            result.files.iter().map(|f| f.insertions).sum(),
            result.files.iter().map(|f| f.deletions).sum(),
        ),
    ));

    ctx.out.blank();
    // git-tpl will not apply the patch — ADR-002 and ADR-020 — but it knows
    // exactly what would, and a user reconstructing this from prose gets the
    // `-C` wrong the first time.
    match &args.output {
        Some(path) => {
            ctx.out.say(field(
                &ctx.out.theme,
                "written",
                &path.display().to_string(),
            ));
            ctx.out.say(field(
                &ctx.out.theme,
                "apply",
                &command(
                    &ctx.out.theme,
                    &result
                        .apply_command
                        .replace("git tpl backport | ", "")
                        .replace(" am", &format!(" am {}", path.display())),
                ),
            ));
        }
        None => ctx.out.say(field(
            &ctx.out.theme,
            "apply",
            &command(&ctx.out.theme, &result.apply_command),
        )),
    }
}

/// The `--json` payload.
fn payload(args: &BackportArgs, result: &Backport) -> serde_json::Value {
    serde_json::json!({
        "result": if result.files.is_empty() { "nothingToBackport" } else { "patched" },
        "template": result.source,
        "revision": result.revision_description,
        // The patch travels *in* the payload rather than on stdout beside it:
        // `--json` means stdout is one JSON object, and a command that
        // sometimes emits two things on stdout is not machine-readable.
        "patch": result.patch,
        "output": args.output.as_ref().map(|p| p.display().to_string()),
        "applyCommand": result.apply_command,
        "files": result.files.iter().map(|file| serde_json::json!({
            "rendered": file.rendered,
            "source": file.source,
            "insertions": file.insertions,
            "deletions": file.deletions,
            "added": file.added,
        })).collect::<Vec<_>>(),
        "skipped": result.skipped.iter().map(|skipped| serde_json::json!({
            "path": skipped.path,
            "reason": skipped.reason,
        })).collect::<Vec<_>>(),
        // Carried so a consumer can gate on it. A reversed substitution is the
        // one thing in a backport that the round trip does not prove right for
        // anyone but the user who ran it, and a reviewer that cannot see which
        // lines they were cannot review them.
        "unsubstituted": result.unsubstituted.iter().map(|reversal| serde_json::json!({
            "path": reversal.path,
            "source": reversal.template_path,
            "line": reversal.line,
            "rendered": reversal.rendered,
            "project": reversal.project,
            "patched": reversal.patched,
            "expressions": reversal.expressions,
        })).collect::<Vec<_>>(),
        "insertions": result.files.iter().map(|f| f.insertions).sum::<usize>(),
        "deletions": result.files.iter().map(|f| f.deletions).sum::<usize>(),
    })
}
