//! `git tpl push`

use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::{self, OpError};

use super::Session;
use crate::cli::{GlobalArgs, RemoteArgs};

pub fn run(args: RemoteArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let preferences = Preferences::load(&ctx.repo)?.with_overrides(Overrides {
        remote: args.remote.as_deref(),
        ..Default::default()
    });

    if args.dry_run {
        let (_, ref_name) = ops::identify(&ctx.root)?;
        ctx.say(format!("Would push {ref_name} to {}", preferences.remote));
        if global.json {
            println!(
                "{}",
                crate::report::success(serde_json::json!({
                    "dryRun": true,
                    "remote": preferences.remote,
                    "ref": ref_name,
                }))
            );
        }
        return Ok(crate::exit::SUCCESS);
    }

    let ref_name = ops::push(&ctx.repo, &ctx.root, &preferences)?;
    ctx.say(format!("Pushed {ref_name} to {}.", preferences.remote));
    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "remote": preferences.remote,
                "ref": ref_name,
            }))
        );
    }
    Ok(crate::exit::SUCCESS)
}
