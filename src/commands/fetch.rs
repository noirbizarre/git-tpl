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
        ctx.out.say(format!(
            "Would fetch {} from {}",
            preferences.fetch_refspec(),
            preferences.remote
        ));
        if global.json {
            println!(
                "{}",
                crate::report::success(serde_json::json!({
                    "dryRun": true,
                    "remote": preferences.remote,
                    "refspec": preferences.fetch_refspec(),
                }))
            );
        }
        return Ok(crate::exit::SUCCESS);
    }

    let report = ops::fetch_report(&ctx.repo, &ctx.root, &preferences)?;
    let ref_name = &report.ref_name;
    let relation = &report.relation;

    // The arms are ordered, not independent: `is_diverged` is `ahead > 0 &&
    // behind > 0`, so it has to be tested before the plain `behind > 0` arm
    // below, which would otherwise swallow it and tell a diverged user to
    // simply merge.
    //
    // `state` comes out of the *same* match for that reason. Deriving it a
    // second time somewhere else is how the JSON and the prose come to
    // disagree about a diverged ref.
    let state = match relation {
        None => {
            let message = match (report.local_tip.is_some(), report.remote_tip.is_some()) {
                (false, true) => format!(
                    "Local {ref_name} is absent; a shared copy is available on {}. Fetch did not restore the local ref.",
                    preferences.remote
                ),
                (true, false) => format!(
                    "No shared copy of {ref_name} on {}. The local ref is present.",
                    preferences.remote
                ),
                (false, false) => format!(
                    "Neither local {ref_name} nor a shared copy on {} is present.",
                    preferences.remote
                ),
                (true, true) => unreachable!("two tips always have a relation"),
            };
            ctx.out.say(muted(&ctx.out.theme, &message));
            "absent"
        }
        Some(relation) if relation.is_synced() => {
            ctx.out.say(format!(
                "{ref_name} is in sync with {}.",
                preferences.remote
            ));
            "synced"
        }
        Some(relation) if relation.is_diverged() => {
            ctx.out.blank();
            ctx.out
                .say(format!("{ref_name} has {}.", relation.describe()));
            ctx.out.blank();
            ctx.out
                .say("Both were rendered independently. Reconcile them:");
            ctx.out.say(command(
                &ctx.out.theme,
                &format!("git merge refs/remotes/{}/tpl/...", preferences.remote),
            ));
            "diverged"
        }
        Some(relation) if relation.behind > 0 => {
            ctx.out.blank();
            ctx.out.say(format!(
                "The remote copy is {} commit(s) ahead of your local ref.",
                relation.behind
            ));
            ctx.out.blank();
            // Fetching never moves the local ref. What to do about a newer
            // remote copy is a decision, and adopting someone else's rendering
            // silently would be a surprising thing for a fetch to do.
            ctx.out.say("Adopt it, or render your own:");
            ctx.out.say(command(
                &ctx.out.theme,
                &format!(
                    "git merge refs/remotes/{}/tpl/{}",
                    preferences.remote,
                    ref_name.trim_start_matches("refs/tpl/")
                ),
            ));
            ctx.out.say(command(&ctx.out.theme, "git tpl update"));
            "behind"
        }
        Some(relation) => {
            ctx.out.say(format!(
                "You have {} rendering(s) the remote does not.",
                relation.ahead
            ));
            ctx.out.say(command(&ctx.out.theme, "git tpl push"));
            "ahead"
        }
    };

    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "remote": preferences.remote,
                "ref": ref_name,
                "state": state,
                "localPresent": report.local_tip.is_some(),
                "remotePresent": report.remote_tip.is_some(),
                // Absence of either tip prevents an ancestry comparison.
                "relation": relation.as_ref().map(|relation| serde_json::json!({
                    "ahead": relation.ahead,
                    "behind": relation.behind,
                    "synced": relation.is_synced(),
                    "diverged": relation.is_diverged(),
                })),
            }))
        );
    }

    Ok(crate::exit::SUCCESS)
}
