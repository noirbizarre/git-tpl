//! `git tpl diff`

use tpl::ops::{self, OpError};

use super::Session;
use crate::cli::{DiffArgs, GlobalArgs};
use crate::theme::{change_stat, diff_summary, muted};

pub fn run(args: DiffArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;

    // `--name-only` wins when both are given: it is the machine-readable mode,
    // and a caller piping it should never receive a summary.
    if args.name_only {
        let changes = ops::diff_changes(&ctx.repo, &ctx.root, &args.paths, args.reverse)?;
        if changes.is_empty() {
            ctx.say(muted(&ctx.theme, "No differences."));
            return Ok(crate::exit::SUCCESS);
        }
        for c in &changes {
            // Paths go to stdout: `git tpl diff --name-only | xargs` is an
            // obvious thing to do, and it must not receive decoration.
            println!("{}", c.path);
        }
        return Ok(crate::exit::SUCCESS);
    }

    if args.stat {
        let stats = ops::diff_stat(&ctx.repo, &ctx.root, &args.paths, args.reverse)?;
        if stats.is_empty() {
            ctx.say(muted(&ctx.theme, "No differences."));
            return Ok(crate::exit::SUCCESS);
        }

        let width = stats.iter().map(|s| s.path.len()).max().unwrap_or(0);
        for s in &stats {
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

    let patch = ops::diff(&ctx.repo, &ctx.root, &args.paths, args.reverse)?;
    if patch.is_empty() {
        ctx.say(muted(&ctx.theme, "No differences."));
    } else {
        // A patch is data. It goes to stdout so it can be piped into `git apply`.
        print!("{patch}");
    }
    Ok(crate::exit::SUCCESS)
}
