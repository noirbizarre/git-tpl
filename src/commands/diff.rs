//! `git tpl diff`

use tpl::ops::{self, OpError};

use super::Session;
use crate::cli::{DiffArgs, GlobalArgs};
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
    ctx.say(warning(
        &ctx.theme,
        &format!(
            "{} {files} would conflict; shown with conflict markers",
            conflicts.len()
        ),
    ));
    for path in conflicts {
        ctx.say(muted(&ctx.theme, &format!("         {path}")));
    }
    ctx.blank();
}

pub fn run(args: DiffArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;

    // `--name-only` wins when both are given: it is the machine-readable mode,
    // and a caller piping it should never receive a summary.
    if args.name_only {
        let preview = ops::diff_changes(&ctx.repo, &ctx.root, &args.paths, args.reverse)?;
        if preview.changes.is_empty() {
            ctx.say(muted(&ctx.theme, "No differences."));
            return Ok(crate::exit::SUCCESS);
        }
        report_conflicts(&ctx, &preview.conflicts);
        for c in &preview.changes {
            // Paths go to stdout: `git tpl diff --name-only | xargs` is an
            // obvious thing to do, and it must not receive decoration.
            println!("{}", c.path);
        }
        return Ok(crate::exit::SUCCESS);
    }

    if args.stat {
        let preview = ops::diff_stat(&ctx.repo, &ctx.root, &args.paths, args.reverse)?;
        let stats = &preview.changes;
        if stats.is_empty() {
            ctx.say(muted(&ctx.theme, "No differences."));
            return Ok(crate::exit::SUCCESS);
        }

        report_conflicts(&ctx, &preview.conflicts);

        let width = stats.iter().map(|s| s.path.len()).max().unwrap_or(0);
        for s in stats {
            ctx.say(change_stat(&ctx.theme, s, width));
        }

        let insertions = stats.iter().map(|s| s.insertions).sum();
        let deletions = stats.iter().map(|s| s.deletions).sum();
        ctx.blank();
        ctx.say(muted(
            &ctx.theme,
            &diff_summary(stats.len(), insertions, deletions),
        ));
        return Ok(crate::exit::SUCCESS);
    }

    let preview = ops::diff(&ctx.repo, &ctx.root, &args.paths, args.reverse)?;
    if preview.changes.is_empty() {
        ctx.say(muted(&ctx.theme, "No differences."));
    } else {
        report_conflicts(&ctx, &preview.conflicts);
        // A patch is data. It goes to stdout so it can be piped into `git apply`.
        print!("{}", preview.changes);
    }
    Ok(crate::exit::SUCCESS)
}
