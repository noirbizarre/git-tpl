//! `git tpl pull`: explicit update followed by ordinary Git integration.

use tpl::git::GitBackend;
use tpl::ops::{self, OpError};

use super::{Session, merge, update};
use crate::cli::{GlobalArgs, PullArgs};

pub fn run(args: PullArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let (_, ref_name) = ops::identify(&ctx.root)?;
    // Update alone permits a new orphan; immediately merging it would lose the
    // existing merge base, so pull requires the original history before rendering.
    if ctx.repo.resolve_ref(&ref_name)?.is_none() {
        return Err(OpError::NoRenderedRef { ref_name });
    }
    let updated = update::execute(&ctx, &args.update, false, true)?;
    // An unchanged rendering can still be pending integration from an earlier update.
    let (id, outcome) = ops::merge(
        &ctx.repo,
        &ctx.root,
        args.message.as_deref(),
        !args.no_commit,
    )?;
    let merged = merge::present(&ctx, &id, &outcome);
    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({ "update": updated, "merge": merged }))
        );
    }
    Ok(crate::exit::SUCCESS)
}
