//! `git tpl fetch`

use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::{self, OpError};

use super::Session;
use crate::cli::{GlobalArgs, RemoteArgs};
use crate::theme::{command, muted};

pub fn run(args: RemoteArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let preferences = Preferences::load(&ctx.repo)?.with_overrides(Overrides {
        remote: args.remote.as_deref(),
        ..Default::default()
    });

    if args.dry_run {
        ctx.say(format!(
            "Would fetch {} from {}",
            preferences.fetch_refspec(),
            preferences.remote
        ));
        return Ok(crate::exit::SUCCESS);
    }

    let (ref_name, relation) = ops::fetch(&ctx.repo, &ctx.root, &preferences)?;

    // The arms are ordered, not independent: `is_diverged` is `ahead > 0 &&
    // behind > 0`, so it has to be tested before the plain `behind > 0` arm
    // below, which would otherwise swallow it and tell a diverged user to
    // simply merge.
    match relation {
        None => {
            ctx.say(muted(
                &ctx.theme,
                &format!("No shared copy of {ref_name} on {}.", preferences.remote),
            ));
        }
        Some(relation) if relation.is_synced() => {
            ctx.say(format!(
                "{ref_name} is in sync with {}.",
                preferences.remote
            ));
        }
        Some(relation) if relation.is_diverged() => {
            ctx.blank();
            ctx.say(format!("{ref_name} has {}.", relation.describe()));
            ctx.blank();
            ctx.say("Both were rendered independently. Reconcile them:");
            ctx.say(command(
                &ctx.theme,
                &format!("git merge refs/remotes/{}/tpl/...", preferences.remote),
            ));
        }
        Some(relation) if relation.behind > 0 => {
            ctx.blank();
            ctx.say(format!(
                "The remote copy is {} commit(s) ahead of your local ref.",
                relation.behind
            ));
            ctx.blank();
            // Fetching never moves the local ref. What to do about a newer
            // remote copy is a decision, and adopting someone else's rendering
            // silently would be a surprising thing for a fetch to do.
            ctx.say("Adopt it, or render your own:");
            ctx.say(command(
                &ctx.theme,
                &format!(
                    "git merge refs/remotes/{}/tpl/{}",
                    preferences.remote,
                    ref_name.trim_start_matches("refs/tpl/")
                ),
            ));
            ctx.say(command(&ctx.theme, "git tpl update"));
        }
        Some(relation) => {
            ctx.say(format!(
                "You have {} rendering(s) the remote does not.",
                relation.ahead
            ));
            ctx.say(command(&ctx.theme, "git tpl push"));
        }
    }

    Ok(crate::exit::SUCCESS)
}
