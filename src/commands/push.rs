//! `git tpl push`

use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::{self, OpError};

use super::Context;
use crate::cli::{GlobalArgs, RemoteArgs};

pub fn run(args: RemoteArgs, global: &GlobalArgs) -> Result<(), OpError> {
    let ctx = Context::discover(global)?;
    let preferences =
        Preferences::load(&ctx.repo)?.with_overrides(Overrides {
            remote: args.remote.as_deref(),
            ..Default::default()
        });

    if args.dry_run {
        let (_, ref_name) = ops::identify(&ctx.root)?;
        ctx.say(format!("Would push {ref_name} to {}", preferences.remote));
        return Ok(());
    }

    let ref_name = ops::push(&ctx.repo, &ctx.root, &preferences)?;
    ctx.say(format!("Pushed {ref_name} to {}.", preferences.remote));
    Ok(())
}
