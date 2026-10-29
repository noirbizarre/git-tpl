//! `git tpl log`

use tpl::gitconfig::Preferences;
use tpl::note::plain_line;
use tpl::ops::OpError;
use tpl::ops::log::{self, Log};

use super::Session;
use crate::cli::{GlobalArgs, LogArgs};
use crate::theme::{field, log_entry};

pub fn run(args: LogArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let preferences = Preferences::load(&ctx.repo)?;
    let max_count = if args.all {
        None
    } else {
        Some(args.max_count.unwrap_or(20))
    };
    let log = log::log(&ctx.repo, &ctx.root, args.reference.as_deref(), max_count)?;
    if global.json {
        println!("{}", crate::report::success(json(&log)));
    } else {
        ctx.out.blank();
        ctx.out
            .say(field(&ctx.out.theme, "Template", &plain_line(&log.source)));
        let applied = tpl::ops::describe_revision(
            log.applied_reference.as_deref().unwrap_or("<unknown>"),
            log.applied_revision,
        );
        ctx.out
            .say(field(&ctx.out.theme, "Applied", &plain_line(&applied)));
        if let Some(reference) = &log.target_reference {
            let target = tpl::ops::describe_revision(reference, log.target_revision);
            ctx.out
                .say(field(&ctx.out.theme, "Target", &plain_line(&target)));
            if !log.target_descends_from_applied {
                ctx.out.say("Target does not descend from the applied version; showing target-only commits.");
            }
        }
        ctx.out.blank();
        // One sample for the whole list keeps ages consistent across minute boundaries.
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let relative = preferences.log_relative_date && !args.no_relative_date;
        for entry in &log.entries {
            ctx.out.say(log_entry(&ctx.out.theme, entry, now, relative));
        }
        if log.entries.is_empty() {
            ctx.out.say("No commits to show.");
        }
    }
    Ok(crate::exit::SUCCESS)
}

fn json(log: &Log) -> serde_json::Value {
    serde_json::json!({
        "source": log.source,
        "id": log.id.as_str(),
        "ref": log.ref_name,
        "appliedRendering": log.applied_rendering.to_hex(),
        "applied": crate::report::revision(log.applied_reference.as_deref(), Some(log.applied_revision), Some(false)),
        "target": crate::report::revision(log.target_reference.as_deref().or(log.applied_reference.as_deref()), Some(log.target_revision), Some(false)),
        "mode": if log.target_reference.is_some() { "range" } else { "applied" },
        "targetDescendsFromApplied": log.target_descends_from_applied,
        "maxCount": log.max_count,
        "entries": log.entries.iter().map(|entry| {
            let parsed = log::conventional(&entry.subject);
            serde_json::json!({
                "commit": entry.oid.to_hex(),
                "subject": entry.subject,
                "committedAt": entry.committed_at,
                "conventional": parsed.map(|prefix| serde_json::json!({
                    "type": prefix.kind, "scope": prefix.scope, "breaking": prefix.breaking,
                })),
            })
        }).collect::<Vec<_>>(),
    })
}
