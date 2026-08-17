//! `git tpl backport`

use tpl::ops::{self, Backport, OpError};

use super::Session;
use crate::cli::{BackportArgs, GlobalArgs};
use crate::prompt::Confirmer;
use crate::theme::{command, diff_summary, field, headline, muted, warning};

pub fn run(args: BackportArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let preferences = tpl::gitconfig::Preferences::load(&ctx.repo)?;
    let mut confirmer = Confirmer;

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

    let result = ops::backport(
        &ctx.repo,
        &ctx.root,
        &args.paths,
        &args.exclude,
        &ctx.user,
        answering,
        trust,
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
        ctx.warn(warning(
            &ctx.theme,
            &format!("skipped {}: {}", skipped.path, skipped.reason),
        ));
    }

    if result.files.is_empty() {
        ctx.say(muted(
            &ctx.theme,
            "Nothing to backport: the project matches the template's rendering.",
        ));
        return;
    }

    ctx.say(headline(
        &ctx.theme,
        "backport",
        &result.revision_description,
    ));
    ctx.blank();
    for file in &result.files {
        ctx.say(muted(
            &ctx.theme,
            &format!("  {} <- {}", file.source, file.rendered),
        ));
    }
    ctx.blank();
    ctx.say(muted(
        &ctx.theme,
        &diff_summary(
            result.files.len(),
            result.files.iter().map(|f| f.insertions).sum(),
            result.files.iter().map(|f| f.deletions).sum(),
        ),
    ));

    ctx.blank();
    // git-tpl will not apply the patch — ADR-002 and ADR-020 — but it knows
    // exactly what would, and a user reconstructing this from prose gets the
    // `-C` wrong the first time.
    match &args.output {
        Some(path) => {
            ctx.say(field(&ctx.theme, "written", &path.display().to_string()));
            ctx.say(field(
                &ctx.theme,
                "apply",
                &command(
                    &ctx.theme,
                    &result
                        .apply_command
                        .replace("git tpl backport | ", "")
                        .replace(" am", &format!(" am {}", path.display())),
                ),
            ));
        }
        None => ctx.say(field(
            &ctx.theme,
            "apply",
            &command(&ctx.theme, &result.apply_command),
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
        "insertions": result.files.iter().map(|f| f.insertions).sum::<usize>(),
        "deletions": result.files.iter().map(|f| f.deletions).sum::<usize>(),
    })
}
