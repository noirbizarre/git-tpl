//! `git tpl diff`

use tpl::ops::{self, OpError};

use super::Session;
use crate::cli::{DiffArgs, GlobalArgs};
use crate::theme::{change, muted};

pub fn run(args: DiffArgs, global: &GlobalArgs) -> Result<(), OpError> {
    let ctx = Session::discover(global)?;

    if args.name_only || args.stat {
        let changes = ops::diff_changes(&ctx.repo, &ctx.root)?;

        if changes.is_empty() {
            ctx.say(muted(&ctx.theme, "No differences."));
            return Ok(());
        }

        for c in &changes {
            if args.name_only {
                // Paths go to stdout: `git tpl diff --name-only | xargs` is an
                // obvious thing to do, and it must not receive decoration.
                println!("{}", c.path);
            } else {
                ctx.say(change(&ctx.theme, c.kind, &c.path));
            }
        }

        if args.stat {
            ctx.blank();
            ctx.say(muted(
                &ctx.theme,
                &format!("{} file(s) differ", changes.len()),
            ));
        }
        return Ok(());
    }

    let patch = ops::diff(&ctx.repo, &ctx.root, &args.paths, args.reverse)?;
    if patch.is_empty() {
        ctx.say(muted(&ctx.theme, "No differences."));
    } else {
        // A patch is data. It goes to stdout so it can be piped into `git apply`.
        print!("{patch}");
    }
    Ok(())
}
