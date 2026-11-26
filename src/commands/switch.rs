//! `git tpl switch`: replace the source, retaining the attachment's merge base.

use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::{self, OpError};

use super::{Session, answering, report_ignored, report_ignored_paths, supplied, trust};
use crate::cli::{GlobalArgs, SwitchArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{change, command, field, headline};

pub fn run(args: SwitchArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    if args.merge {
        super::pull::preflight(&ctx)?;
    }
    let preferences = Preferences::load(&ctx.repo)?.with_overrides(Overrides {
        non_interactive: args.answers.defaults,
        ..Default::default()
    });
    let source = ctx.user.expand(&args.template).into_owned();
    let mut prompter = Interactive;
    let mut confirmer = Confirmer(ctx.out.theme.clone());
    let outcome = ops::switch(
        &ctx.repo,
        &ctx.root,
        &source,
        args.r#ref,
        supplied(&args.answers)?,
        args.dirty,
        args.answers.strict_answers,
        &ctx.user,
        &ctx.access,
        answering(&args.answers, preferences.interactive, &mut prompter),
        trust(
            &args.answers,
            args.trust,
            preferences.interactive,
            &mut confirmer,
        ),
    )?;
    report_ignored(&ctx.out, &outcome.ignored_answers);
    report_ignored_paths(&ctx.out, &outcome.ignored);
    ctx.out
        .say(field(&ctx.out.theme, "Template", &outcome.source));
    ctx.out.say(field(
        &ctx.out.theme,
        "Revision",
        &ops::describe_revision(&outcome.reference, outcome.revision),
    ));
    ctx.out
        .say(headline(&ctx.out.theme, "Switched", &outcome.id.ref_name()));
    for item in &outcome.changes {
        ctx.out.say(change(&ctx.out.theme, item.kind, &item.path));
    }
    if !outcome.reused_answers.is_empty() {
        ctx.out.say(format!(
            "Reused answers: {}.",
            outcome.reused_answers.join(", ")
        ));
    }
    if !outcome.skipped_answers.is_empty() {
        ctx.out.say(format!(
            "Recorded answers not reused: {}.",
            outcome.skipped_answers.join(", ")
        ));
    }
    ctx.out
        .say("Configuration changes were left unstaged in .config/git.tpl.toml.");
    let switched = serde_json::json!({
        "result": "switched",
        "previousTemplate": outcome.previous_source,
        "template": outcome.source,
        "inherited": outcome.inherited,
        "id": outcome.id.as_str(),
        "ref": outcome.id.ref_name(),
        "commit": outcome.commit.to_hex(),
        "changed": outcome.changed,
        "revision": crate::report::revision(Some(&outcome.reference), Some(outcome.revision), Some(outcome.dirty)),
        "changes": crate::report::changes(&outcome.changes),
        "reusedAnswers": outcome.reused_answers,
        "skippedAnswers": outcome.skipped_answers,
        "ignoredAnswers": outcome.ignored_answers,
    });
    let payload = if args.merge {
        let merged = super::pull::integrate(&ctx, None, true)?;
        serde_json::json!({ "switch": switched, "merge": merged })
    } else {
        ctx.out
            .say("Your rendered worktree files were not modified. Run:");
        ctx.out.say(command(&ctx.out.theme, "git tpl diff"));
        ctx.out.say(command(&ctx.out.theme, "git tpl merge"));
        switched
    };
    if global.json {
        println!("{}", crate::report::success(payload));
    }
    Ok(crate::exit::SUCCESS)
}
