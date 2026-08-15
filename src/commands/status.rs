//! `git tpl status`

use tpl::gitconfig::Preferences;
use tpl::ops::{self, OpError};

use super::Session;
use crate::cli::{Format, GlobalArgs, StatusArgs};
use crate::theme::{command, field, muted, warning};

pub fn run(args: StatusArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    // No overrides: `status` reports against the configured remote on purpose.
    // It prompts for nothing and pushes nothing, and a `--remote` that changed
    // only which remote a report compared against would be a flag whose effect
    // is invisible in the output it produces.
    let preferences = Preferences::load(&ctx.repo)?;
    let status = ops::status(&ctx.repo, &ctx.root, &preferences)?;

    let deprecated_json = args.format == Some(Format::Json);
    if args.format.is_some() {
        // stderr, so it cannot corrupt the JSON the caller came for.
        ctx.warn(warning(
            &ctx.theme,
            "`--format` is deprecated and will be removed; use `--json`",
        ));
    }

    if global.json || deprecated_json {
        println!("{}", crate::report::success(json(&status)));
    } else {
        print_text(&ctx, &status);
    }

    Ok(if status.is_pending() {
        crate::exit::PENDING
    } else {
        crate::exit::SUCCESS
    })
}

fn print_text(ctx: &Session, status: &ops::Status) {
    ctx.blank();
    ctx.say(field(&ctx.theme, "Template", &status.source));
    ctx.say(field(&ctx.theme, "Ref", &status.ref_name));
    ctx.blank();

    let rendered = status
        .recorded
        .as_ref()
        .map(|r| r.describe_revision())
        .unwrap_or_else(|| "never rendered".into());

    let revision_line = match (
        &status.available_revision_description,
        status.template_moved,
    ) {
        (Some(available), true) => format!(
            "{rendered} → {available}   {}",
            muted(&ctx.theme, "template has moved")
        ),
        (Some(available), false) if status.tip.is_none() => available.clone(),
        (_, _) => rendered.clone(),
    };
    ctx.say(field(&ctx.theme, "Revision", &revision_line));

    ctx.say(field(
        &ctx.theme,
        "Rendered",
        &match status.rendering_count {
            0 => "nothing yet".to_string(),
            1 => "1 rendering".to_string(),
            n => format!("{n} renderings"),
        },
    ));

    ctx.say(field(
        &ctx.theme,
        "Merged",
        if status.tip.is_none() {
            "n/a"
        } else if status.merged {
            "yes"
        } else {
            "no — there is a rendering you have not taken"
        },
    ));

    if let Some((remote_ref, relation)) = &status.remote {
        ctx.say(field(
            &ctx.theme,
            "Remote",
            &format!("{remote_ref} — {}", relation.describe()),
        ));
    }

    ctx.say(field(
        &ctx.theme,
        "Worktree",
        if status.worktree_clean {
            "clean"
        } else {
            "dirty"
        },
    ));

    // Say what to do next, rather than leaving the reader to infer it.
    if status.template_moved {
        ctx.blank();
        ctx.say("The template has moved. Run:");
        ctx.say(command(&ctx.theme, "git tpl update"));
    } else if status.tip.is_some() && !status.merged {
        ctx.blank();
        ctx.say("There is a rendering you have not merged. Run:");
        ctx.say(command(&ctx.theme, "git tpl diff"));
        ctx.say(command(&ctx.theme, "git tpl merge"));
    }
}

/// The machine-readable form.
///
/// camelCase keys, and `ref` rather than `refName`: this is consumed by
/// scripts and CI, where JSON is conventionally camelCase, and the names
/// follow the vocabulary of the text output rather than the field names of
/// `Status`. Renaming a key here is a breaking change.
fn json(status: &ops::Status) -> serde_json::Value {
    let recorded = status.recorded.as_ref();
    serde_json::json!({
        "source": status.source,
        "id": status.id.as_str(),
        "ref": status.ref_name,
        "tip": status.tip.map(|o| o.to_hex()),
        "renderedRevision": recorded.and_then(|r| r.reference.clone()),
        "renderedCommit": recorded.and_then(|r| r.commit.map(|c| c.to_hex())),
        "dirty": recorded.map(|r| r.dirty).unwrap_or(false),
        "availableRevision": status.available_revision_description,
        "templateMoved": status.template_moved,
        "merged": status.merged,
        "renderingCount": status.rendering_count,
        "remote": status.remote.as_ref().map(|(name, relation)| serde_json::json!({
            "ref": name,
            "ahead": relation.ahead,
            "behind": relation.behind,
        })),
        "worktreeClean": status.worktree_clean,
        "pending": status.is_pending(),
    })
}
