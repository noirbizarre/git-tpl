//! Project-independent source-object maintenance.

use crate::cli::{CacheArgs, CacheCommand, GlobalArgs};
use std::time::Duration;
use tpl::ops::OpError;

pub fn run(args: CacheArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = super::Standalone::new(global)?;
    let result = match args.command {
        CacheCommand::List => {
            ctx.out
                .say(format!("Cache: {}", ctx.access.root()?.display()));
            let entries = ctx.access.list()?;
            for entry in &entries {
                ctx.out.say(format!(
                    "{}  {} bytes  {}",
                    entry.id, entry.bytes, entry.source
                ));
            }
            serde_json::json!({ "root": ctx.access.root()?, "entries": entries })
        }
        CacheCommand::Fetch { source } => {
            let source = ctx.user.expand(&source);
            ctx.access.fetch(&source)?;
            ctx.out.say("Source cache refreshed");
            serde_json::json!({ "source": tpl::git::cache::redact(&source), "refreshed": true })
        }
        CacheCommand::Clear { source } => {
            let source = source.as_deref().map(|source| ctx.user.expand(source));
            let removed = ctx.access.clear(source.as_deref(), None)?;
            ctx.out.say(format!(
                "Removed {} repositories; skipped {} busy entries",
                removed.removed.len(),
                removed.skipped.len()
            ));
            serde_json::to_value(removed).expect("removal serializes")
        }
        CacheCommand::Prune { older_than } => {
            let seconds =
                older_than
                    .checked_mul(86400)
                    .ok_or_else(|| OpError::InvalidArgument {
                        message: "prune age is too large".into(),
                    })?;
            let removed = ctx.access.clear(None, Some(Duration::from_secs(seconds)))?;
            ctx.out.say(format!(
                "Removed {} repositories; skipped {} busy entries",
                removed.removed.len(),
                removed.skipped.len()
            ));
            serde_json::to_value(removed).expect("removal serializes")
        }
    };
    if global.json {
        println!("{}", crate::report::success(result));
    }
    Ok(crate::exit::SUCCESS)
}
