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
    let merged = integrate(&ctx, args.message.as_deref(), !args.no_commit)?;
    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({ "update": updated, "merge": merged }))
        );
    }
    Ok(crate::exit::SUCCESS)
}

/// Refuse before rendering: a combined operation needs a merge base and clean
/// starting state, before its own configuration edit makes the worktree dirty.
pub(super) fn preflight(ctx: &Session) -> Result<(), OpError> {
    let (_, ref_name) = ops::identify(&ctx.root)?;
    if ctx.repo.resolve_ref(&ref_name)?.is_none() {
        return Err(OpError::NoRenderedRef { ref_name });
    }
    if !ctx.repo.is_normal_state() {
        return Err(tpl::git::GitError::RepositoryBusy.into());
    }
    if !ctx.repo.is_index_clean()? {
        return Err(tpl::git::GitError::DirtyIndex.into());
    }
    if !ctx.repo.is_clean()? {
        return Err(tpl::git::GitError::DirtyWorktree.into());
    }
    Ok(())
}

/// Ordinary integration shared by pull and opt-in render-and-merge commands.
pub(super) fn integrate(
    ctx: &Session,
    message: Option<&str>,
    commit: bool,
) -> Result<serde_json::Value, OpError> {
    // Prompts can leave enough time for another terminal to stage unrelated
    // changes. Never let them ride in an automatic merge commit.
    if !ctx.repo.is_normal_state() {
        return Err(tpl::git::GitError::RepositoryBusy.into());
    }
    if !ctx.repo.is_index_clean()? {
        return Err(tpl::git::GitError::DirtyIndex.into());
    }
    let (id, outcome) = ops::merge(&ctx.repo, &ctx.root, message, commit)?;
    Ok(merge::present(ctx, &id, &outcome))
}
