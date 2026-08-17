//! `git tpl diff`

use tpl::ops::{self, OpError};

use super::{Session, answering, report_ignored_paths, supplied, trust};
use crate::cli::{DiffArgs, GlobalArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{change_stat, diff_summary, muted, warning};

/// Announce the paths a merge could not resolve on its own.
///
/// Chrome, not data: it goes where `say` goes, so a piped patch stays a patch.
/// The exit code stays zero — a conflicting preview is a correct answer to the
/// question asked, and the whole point of looking before merging.
fn report_conflicts(ctx: &Session, conflicts: &[String]) {
    if conflicts.is_empty() {
        return;
    }
    let files = if conflicts.len() == 1 {
        "file"
    } else {
        "files"
    };
    ctx.out.say(warning(
        &ctx.out.theme,
        &format!(
            "{} {files} would conflict; shown with conflict markers",
            conflicts.len()
        ),
    ));
    for path in conflicts {
        ctx.out.say(muted(&ctx.out.theme, &format!("         {path}")));
    }
    ctx.out.blank();
}

pub fn run(args: DiffArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let against = preview(&ctx, args.dirty, &args.answers)?;

    // `--exit-code` reports *difference*, in Git's own convention, so that CI
    // can assert "the template output has not drifted" without parsing
    // anything. Deliberately not conflict: a conflicting preview is a correct
    // answer to the question asked, and turning it into a failure would make
    // the flag useless on exactly the repositories that need it.
    let code = |changed: bool| {
        if changed && args.exit_code {
            crate::exit::FAILURE
        } else {
            crate::exit::SUCCESS
        }
    };

    // JSON first: it subsumes every other mode, and it is the one that carries
    // the conflicts as *data*. In text output they are chrome on stderr, which
    // is right for a human reading a patch and useless to a caller — and
    // "which files would conflict" is the single most valuable thing to know
    // before merging.
    if global.json {
        let preview = ops::diff_stat(&ctx.repo, &ctx.root, &args.paths, args.reverse, against)?;
        let stats = &preview.changes;
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "conflicts": preview.conflicts,
                "changes": stats.iter().map(|s| serde_json::json!({
                    "path": s.path,
                    "kind": s.kind.as_str(),
                    "insertions": s.insertions,
                    "deletions": s.deletions,
                    "binary": s.binary,
                })).collect::<Vec<_>>(),
                "insertions": stats.iter().map(|s| s.insertions).sum::<usize>(),
                "deletions": stats.iter().map(|s| s.deletions).sum::<usize>(),
            }))
        );
        return Ok(code(!stats.is_empty()));
    }

    // `--name-only` wins when both are given: it is the machine-readable mode,
    // and a caller piping it should never receive a summary.
    if args.name_only {
        let preview = ops::diff_changes(&ctx.repo, &ctx.root, &args.paths, args.reverse, against)?;
        if preview.changes.is_empty() {
            ctx.out.say(muted(&ctx.out.theme, "No differences."));
            return Ok(code(false));
        }
        report_conflicts(&ctx, &preview.conflicts);
        for c in &preview.changes {
            // Paths go to stdout: `git tpl diff --name-only | xargs` is an
            // obvious thing to do, and it must not receive decoration.
            println!("{}", c.path);
        }
        return Ok(code(true));
    }

    if args.stat {
        let preview = ops::diff_stat(&ctx.repo, &ctx.root, &args.paths, args.reverse, against)?;
        let stats = &preview.changes;
        if stats.is_empty() {
            ctx.out.say(muted(&ctx.out.theme, "No differences."));
            return Ok(code(false));
        }

        report_conflicts(&ctx, &preview.conflicts);

        let width = stats.iter().map(|s| s.path.len()).max().unwrap_or(0);
        for s in stats {
            ctx.out.say(change_stat(&ctx.out.theme, s, width));
        }

        let insertions = stats.iter().map(|s| s.insertions).sum();
        let deletions = stats.iter().map(|s| s.deletions).sum();
        ctx.out.blank();
        ctx.out.say(muted(
            &ctx.out.theme,
            &diff_summary(stats.len(), insertions, deletions),
        ));
        return Ok(code(true));
    }

    let preview = ops::diff(&ctx.repo, &ctx.root, &args.paths, args.reverse, against)?;
    let changed = !preview.changes.is_empty();
    if changed {
        report_conflicts(&ctx, &preview.conflicts);
        // A patch is data. It goes to stdout so it can be piped into `git apply`.
        print!("{}", preview.changes);
    } else {
        ctx.out.say(muted(&ctx.out.theme, "No differences."));
    }
    Ok(code(changed))
}

/// Render the template now, when `--dirty` asks for a preview of it.
///
/// The result is a commit no ref points at, so nothing is written that a later
/// `update` would have to reconcile with.
fn preview(
    ctx: &Session,
    dirty: bool,
    answers: &crate::cli::AnswerArgs,
) -> Result<Option<tpl::git::Oid>, OpError> {
    if !dirty {
        return Ok(None);
    }
    let preferences = tpl::gitconfig::Preferences::load(&ctx.repo)?;
    let mut prompter = Interactive;
    let mut confirmer = Confirmer;
    ops::render_preview(
        &ctx.repo,
        &ctx.root,
        supplied(answers)?,
        true,
        &ctx.user,
        answering(answers, preferences.interactive, &mut prompter),
        trust(answers, false, preferences.interactive, &mut confirmer),
    )
    .map(|preview| {
        // Warned here rather than swallowed: a `--dirty` diff that quietly
        // omitted the files a `.gitignore` removed would understate the change.
        report_ignored_paths(&ctx.out, &preview.ignored);
        Some(preview.commit)
    })
}
