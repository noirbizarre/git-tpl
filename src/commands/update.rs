//! `git tpl update`

use tpl::gitconfig::Preferences;
use tpl::ops::{self, OpError, UpdateOutcome};

use super::{Context, answering, trust};
use crate::cli::{GlobalArgs, UpdateArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{change, command, field, heading, muted};

pub fn run(args: UpdateArgs, global: &GlobalArgs) -> Result<(), OpError> {
    let ctx = Context::discover(global)?;
    let preferences = Preferences::load(&ctx.repo)?.with_overrides(
        args.remote.as_deref(),
        args.push,
        args.answers.defaults,
    );

    let mut config = tpl::config::Config::load(&ctx.root)?;
    // `--ref` renders a different revision for this run only. It deliberately
    // does not rewrite the configuration: "show me what v2 would look like" is
    // a question, not a decision.
    if let Some(reference) = &args.r#ref {
        config.template.r#ref = Some(reference.clone());
    }

    let overrides = args
        .answers
        .parsed()
        .map_err(|message| OpError::InvalidArgument { message })?;

    if args.dry_run {
        return dry_run(
            &ctx,
            &config,
            overrides,
            args.dirty,
            preferences.interactive,
            args.trust,
        );
    }

    let mut prompter = Interactive;
    let mut confirmer = Confirmer;
    let outcome = ops::update(
        &ctx.repo,
        &ctx.root,
        overrides,
        args.dirty,
        answering(&args.answers, preferences.interactive, &mut prompter),
        trust(
            &args.answers,
            args.trust,
            preferences.interactive,
            &mut confirmer,
        ),
    )?;

    match outcome {
        UpdateOutcome::UpToDate { revision } => {
            ctx.say(format!(
                "Already up to date with {} at {revision}.",
                config.template.source
            ));
        }

        UpdateOutcome::Updated {
            id,
            changes,
            previous_revision,
            revision,
            answers_changed,
            ..
        } => {
            ctx.blank();
            ctx.say(field(&ctx.theme, "Template", &config.template.source));
            ctx.say(field(
                &ctx.theme,
                "Revision",
                &match previous_revision {
                    Some(previous) => format!("{previous} → {revision}"),
                    None => revision.clone(),
                },
            ));
            ctx.blank();
            ctx.say(format!(
                "{} {}",
                heading(&ctx.theme, "Updated"),
                id.ref_name()
            ));
            ctx.blank();
            for c in &changes {
                ctx.say(change(&ctx.theme, c.kind, &c.path));
            }
            ctx.blank();

            // Stated every time. It is the single most surprising property of
            // the tool, and a user who does not believe it will not use it.
            ctx.say("Your working tree was not modified.");

            if answers_changed {
                // The one exception, so it must be said rather than discovered
                // in `git status`.
                ctx.say(muted(
                    &ctx.theme,
                    "The template added a question, so .config/git.tpl.toml was updated.",
                ));
            }

            ctx.blank();
            ctx.say("Run:");
            ctx.say(command(&ctx.theme, "git tpl diff"));
            ctx.say(command(&ctx.theme, "git tpl merge"));

            if preferences.auto_push {
                ctx.blank();
                let pushed = ops::push(&ctx.repo, &ctx.root, &preferences)?;
                ctx.say(format!("Pushed {pushed} to {}.", preferences.remote));
            }
        }
    }

    Ok(())
}

fn dry_run(
    ctx: &Context,
    config: &tpl::config::Config,
    overrides: std::collections::BTreeMap<String, tpl::template::Value>,
    dirty: bool,
    interactive: bool,
    trusted: bool,
) -> Result<(), OpError> {
    let mut supplied = config.answers.clone();
    supplied.extend(overrides);

    let mut prompter = Interactive;
    let answering = if interactive {
        ops::Answering::Interactive(&mut prompter)
    } else {
        ops::Answering::defaults()
    };

    // `--dry-run` still fetches: it reports what *would* change, and a render
    // that skipped its data would report a different tree than the real one.
    let mut confirmer = Confirmer;
    let trust = if trusted {
        ops::Trust::always()
    } else if interactive {
        ops::Trust::Ask(&mut confirmer)
    } else {
        ops::Trust::refuse()
    };

    let rendered = ops::render(
        &ctx.repo, &ctx.root, config, supplied, dirty, answering, trust,
    )?;

    let (id, ref_name) = ops::identify(&ctx.root)?;
    let tip = tpl::git::GitBackend::resolve_ref(&ctx.repo, &ref_name)?;
    let previous_tree = match tip {
        Some(oid) => Some(tpl::git::GitBackend::commit(&ctx.repo, oid)?.tree),
        None => None,
    };

    if previous_tree == Some(rendered.tree) {
        ctx.say("Already up to date. Nothing would change.");
        return Ok(());
    }

    let changes = tpl::git::GitBackend::diff_trees(&ctx.repo, previous_tree, rendered.tree)?;

    ctx.blank();
    ctx.say(field(&ctx.theme, "Template", &config.template.source));
    ctx.say(field(&ctx.theme, "Revision", &rendered.template.reference));
    ctx.blank();
    ctx.say(format!(
        "{} {}",
        heading(&ctx.theme, "Would update"),
        id.ref_name()
    ));
    ctx.blank();
    for c in &changes {
        ctx.say(change(&ctx.theme, c.kind, &c.path));
    }
    ctx.blank();
    ctx.say(muted(&ctx.theme, "Nothing was written."));
    Ok(())
}
