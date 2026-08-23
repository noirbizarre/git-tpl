//! `git tpl backport`

use std::io::IsTerminal;

use tpl::ops::{self, Backport, OpError};

use super::Session;
use crate::cli::{BackportArgs, GlobalArgs};
use crate::prompt::{Chooser, Confirmer, Reverser};
use crate::theme::{command, diff_summary, field, headline, muted, warning};

pub fn run(args: BackportArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let preferences = tpl::gitconfig::Preferences::load(&ctx.repo)?;
    let mut confirmer = Confirmer;
    let mut reverser = Reverser(ctx.out.theme.clone());

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
    let unsubstitute = match reversing(
        args.unsubstitute,
        preferences.interactive,
        global.json,
        std::io::stderr().is_terminal(),
    ) {
        Reversing::Always => ops::Unsubstitute::Always,
        Reversing::Ask => ops::Unsubstitute::Ask(&mut reverser),
        Reversing::Never => ops::Unsubstitute::Never,
    };

    // The opposite rule to the one above, and deliberately so. `--unsubstitute`
    // is absent by default, so silence when nobody can be asked is correct.
    // `-p` was typed: it *is* the request for a prompt, so a prompt that cannot
    // run is a refusal, not a downgrade to sending everything.
    let mut chooser = Chooser(ctx.out.theme.clone());
    let picking = if picking(
        args.patch,
        preferences.interactive,
        global.json,
        std::io::stderr().is_terminal(),
    )? {
        ops::Picking::Ask(&mut chooser)
    } else {
        ops::Picking::All
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
        picking,
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

    print_text(&ctx, &args, &result);

    if global.json {
        println!("{}", crate::report::success(json(&args, &result)));
    } else if args.output.is_none() && !result.patch.is_empty() {
        // The patch is data, so it goes to stdout and nowhere else. Everything
        // above went to stderr precisely so that this stays pipeable straight
        // into `git am`.
        print!("{}", result.patch);
    }

    Ok(crate::exit::SUCCESS)
}

/// Whether substitutions may be reversed, and whether to ask first.
///
/// Split out from [`run`] because it is the whole of the policy and none of the
/// plumbing: `Unsubstitute` borrows the gate mutably, so the decision could not
/// otherwise be examined without a terminal to answer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reversing {
    /// Take every reversal without asking. `--unsubstitute`.
    Always,
    /// Offer each one.
    Ask,
    /// Do not attempt it. What `backport` did before ADR-022.
    Never,
}

/// The rule, in one place.
///
/// `tty` is consulted here and nowhere else in the tree. Elsewhere
/// `tpl.interactive` is enough, because a prompt that cannot run fails the
/// command and the user finds out. Here a failed prompt would read as a refusal
/// and silently shrink the patch, so "can I actually ask?" has to be answered
/// before anything is attempted rather than after.
fn reversing(flag: bool, interactive: bool, json: bool, tty: bool) -> Reversing {
    if flag {
        Reversing::Always
    } else if interactive && !json && tty {
        Reversing::Ask
    } else {
        Reversing::Never
    }
}

/// Whether to ask which hunks to send.
///
/// Returns a `Result`, where [`reversing`] returns a variant, and that is the
/// whole difference between the two flags. An absent `--unsubstitute` on a CI
/// runner means "do not attempt it", which is a decision git-tpl can take on
/// the user's behalf. A `-p` on a CI runner means "show me the hunks", which it
/// cannot do and must not pretend to have done — sending everything would be
/// the one answer the user demonstrably did not ask for.
fn picking(flag: bool, interactive: bool, json: bool, tty: bool) -> Result<bool, OpError> {
    if !flag {
        return Ok(false);
    }
    if json || !interactive || !tty {
        return Err(OpError::Backport(ops::BackportError::NotInteractive));
    }
    Ok(true)
}

/// The prose, on stderr.
///
/// Built beside [`json`] rather than from it: `--json` suppresses this text
/// but not the work behind it, and the two must not come to disagree about
/// what was backported (#53).
fn print_text(ctx: &Session, args: &BackportArgs, result: &Backport) {
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
fn json(args: &BackportArgs, result: &Backport) -> serde_json::Value {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // The flag is a decision taken in advance, and holds everywhere — it is
    // how a script or a CI job opts in at all.
    #[case(true, false, true, false, Reversing::Always)]
    #[case(true, true, false, true, Reversing::Always)]
    // Otherwise: only with a person at a terminal to answer.
    #[case(false, true, false, true, Reversing::Ask)]
    // `--json` means a machine is reading, whatever the terminal says.
    #[case(false, true, true, true, Reversing::Never)]
    // No terminal: the prompt would fail, and a failed prompt reads as a
    // refusal — which would silently shrink the patch.
    #[case(false, true, false, false, Reversing::Never)]
    // `tpl.interactive = false` is the user saying not to ask.
    #[case(false, false, false, true, Reversing::Never)]
    fn a_reversal_is_only_offered_when_someone_can_answer(
        #[case] flag: bool,
        #[case] interactive: bool,
        #[case] json: bool,
        #[case] tty: bool,
        #[case] expected: Reversing,
    ) {
        assert_eq!(reversing(flag, interactive, json, tty), expected);
    }

    #[rstest]
    // Without the flag, nothing is asked and nothing is refused.
    #[case(false, true, false, true)]
    #[case(false, false, true, false)]
    // With it, and someone to ask.
    #[case(true, true, false, true)]
    fn hunk_selection_is_offered_or_omitted_without_complaint(
        #[case] flag: bool,
        #[case] interactive: bool,
        #[case] json: bool,
        #[case] tty: bool,
    ) {
        assert_eq!(picking(flag, interactive, json, tty).unwrap(), flag);
    }

    #[rstest]
    // `--json`: a machine is reading, and there is nothing to show hunks on.
    #[case(true, true, true, true)]
    // No terminal: the prompt cannot run.
    #[case(true, true, false, false)]
    // `tpl.interactive = false` is the user saying not to ask.
    #[case(true, false, false, true)]
    fn patch_selection_needs_somebody_to_ask(
        #[case] flag: bool,
        #[case] interactive: bool,
        #[case] json: bool,
        #[case] tty: bool,
    ) {
        // Refused, not silently downgraded to sending everything: `-p` was
        // typed, so the one thing it cannot mean is "send it all".
        let error = picking(flag, interactive, json, tty).expect_err("refused");
        std::assert_matches!(
            error,
            OpError::Backport(ops::BackportError::NotInteractive),
            "{error:?}"
        );
    }
}
