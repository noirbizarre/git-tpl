//! `git tpl merge`

use tpl::git::{GitBackend, MergeOutcome};
use tpl::ops::{self, OpError};

use super::Session;
use crate::cli::{GlobalArgs, MergeArgs};
use crate::theme::{command, headline, muted, warning};

pub fn run(args: MergeArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;

    if args.abort {
        ctx.repo.abort_merge()?;
        ctx.out.say("Merge aborted.");
        if global.json {
            // Its own `result`, not one of `MergeOutcome`'s: aborting is the
            // undoing of a merge, and reporting it as `upToDate` would tell a
            // caller the opposite of what happened.
            println!(
                "{}",
                crate::report::success(serde_json::json!({ "result": "aborted" }))
            );
        }
        return Ok(crate::exit::SUCCESS);
    }

    let (id, outcome) = ops::merge(
        &ctx.repo,
        &ctx.root,
        args.message.as_deref(),
        !args.no_commit,
    )?;

    match &outcome {
        MergeOutcome::UpToDate => {
            ctx.out.say("Already up to date.");
        }
        MergeOutcome::FastForward { to } => {
            ctx.out.say(headline(
                &ctx.out.theme,
                "Fast-forwarded",
                &format!("{} into the current branch", id.ref_name()),
            ));
            ctx.out
                .say(muted(&ctx.out.theme, &format!("Now at {}.", to.short())));
        }
        MergeOutcome::Merged { commit } => {
            ctx.out.say(headline(
                &ctx.out.theme,
                "Merged",
                &format!("{} into the current branch", id.ref_name()),
            ));
            ctx.out.say(muted(
                &ctx.out.theme,
                &format!("Merge commit {}.", commit.short()),
            ));
        }
        MergeOutcome::Staged => {
            ctx.out.say("Merged and staged, not committed.");
            ctx.out.blank();
            ctx.out.say(command(&ctx.out.theme, "git commit"));
        }
        MergeOutcome::Conflicted { paths } => {
            ctx.out.blank();
            for path in paths {
                ctx.out
                    .say(format!("CONFLICT (content): Merge conflict in {path}"));
            }
            ctx.out.blank();
            ctx.out.say(warning(
                &ctx.out.theme,
                "automatic merge failed; fix conflicts and then commit the result.",
            ));
            ctx.out.blank();
            // The index is left exactly as Git leaves it, so every tool the
            // user already has applies. Say so, rather than inventing a
            // git-tpl-specific resolution flow.
            ctx.out.say(command(
                &ctx.out.theme,
                "git status              see what conflicted",
            ));
            ctx.out.say(command(
                &ctx.out.theme,
                "git mergetool           resolve interactively",
            ));
            ctx.out
                .say(command(&ctx.out.theme, "git commit              finish"));
            ctx.out.say(command(
                &ctx.out.theme,
                "git merge --abort       start over",
            ));
        }
    }

    if global.json {
        let mut payload = crate::report::merge(&outcome);
        if let Some(map) = payload.as_object_mut() {
            map.insert("id".into(), id.as_str().into());
            map.insert("ref".into(), id.ref_name().into());
        }
        println!("{}", crate::report::success(payload));
    }

    Ok(crate::exit::SUCCESS)
}
