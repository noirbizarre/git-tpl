//! `git tpl merge`

use tpl::git::{GitBackend, MergeOutcome};
use tpl::ops::{self, OpError};

use super::Session;
use crate::cli::{GlobalArgs, MergeArgs};
use crate::theme::{command, heading, muted, warning};

pub fn run(args: MergeArgs, global: &GlobalArgs) -> Result<(), OpError> {
    let ctx = Session::discover(global)?;

    if args.abort {
        ctx.repo.abort_merge()?;
        ctx.say("Merge aborted.");
        return Ok(());
    }

    let (id, outcome) = ops::merge(
        &ctx.repo,
        &ctx.root,
        args.message.as_deref(),
        !args.no_commit,
    )?;

    match outcome {
        MergeOutcome::UpToDate => {
            ctx.say("Already up to date.");
        }
        MergeOutcome::FastForward { to } => {
            ctx.say(format!(
                "{} {} into the current branch",
                heading(&ctx.theme, "Fast-forwarded"),
                id.ref_name()
            ));
            ctx.say(muted(&ctx.theme, &format!("Now at {}.", to.short())));
        }
        MergeOutcome::Merged { commit } => {
            ctx.say(format!(
                "{} {} into the current branch",
                heading(&ctx.theme, "Merged"),
                id.ref_name()
            ));
            ctx.say(muted(
                &ctx.theme,
                &format!("Merge commit {}.", commit.short()),
            ));
        }
        MergeOutcome::Staged => {
            ctx.say("Merged and staged, not committed.");
            ctx.blank();
            ctx.say(command(&ctx.theme, "git commit"));
        }
        MergeOutcome::Conflicted { paths } => {
            ctx.blank();
            for path in &paths {
                ctx.say(format!("CONFLICT (content): Merge conflict in {path}"));
            }
            ctx.blank();
            ctx.say(warning(
                &ctx.theme,
                "automatic merge failed; fix conflicts and then commit the result.",
            ));
            ctx.blank();
            // The index is left exactly as Git leaves it, so every tool the
            // user already has applies. Say so, rather than inventing a
            // git-tpl-specific resolution flow.
            ctx.say(command(
                &ctx.theme,
                "git status              see what conflicted",
            ));
            ctx.say(command(
                &ctx.theme,
                "git mergetool           resolve interactively",
            ));
            ctx.say(command(&ctx.theme, "git commit              finish"));
            ctx.say(command(&ctx.theme, "git merge --abort       start over"));
        }
    }

    Ok(())
}
