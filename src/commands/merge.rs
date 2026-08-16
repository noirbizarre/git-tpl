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
        ctx.say("Merge aborted.");
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
            ctx.say("Already up to date.");
        }
        MergeOutcome::FastForward { to } => {
            ctx.say(headline(
                &ctx.theme,
                "Fast-forwarded",
                &format!("{} into the current branch", id.ref_name()),
            ));
            ctx.say(muted(&ctx.theme, &format!("Now at {}.", to.short())));
        }
        MergeOutcome::Merged { commit } => {
            ctx.say(headline(
                &ctx.theme,
                "Merged",
                &format!("{} into the current branch", id.ref_name()),
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
            for path in paths {
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
