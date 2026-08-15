//! `git tpl update`

use tpl::git::GitBackend;
use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::{self, OpError, UpdateOutcome};

use super::{Session, answering, report_ignored, supplied, trust};
use crate::cli::{GlobalArgs, UpdateArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{change, command, field, headline, muted};

pub fn run(args: UpdateArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;
    let preferences = Preferences::load(&ctx.repo)?.with_overrides(Overrides {
        remote: args.remote.as_deref(),
        push: args.push,
        non_interactive: args.answers.defaults,
    });

    let mut config = tpl::config::Config::load(&ctx.root)?;
    // `--ref` renders a different revision for this run only. It deliberately
    // does not rewrite the configuration: "show me what v2 would look like" is
    // a question, not a decision.
    if let Some(reference) = &args.r#ref {
        config.template.r#ref = Some(reference.clone());
    }

    let overrides = supplied(&args.answers)?;

    if args.dry_run {
        return dry_run(&ctx, &config, &args, overrides, preferences.interactive)
            .map(|()| crate::exit::SUCCESS);
    }

    let mut prompter = Interactive;
    let mut confirmer = Confirmer;
    let outcome = ops::update(
        &ctx.repo,
        &ctx.root,
        overrides,
        args.dirty,
        &ctx.user,
        answering(&args.answers, preferences.interactive, &mut prompter),
        trust(
            &args.answers,
            args.trust,
            preferences.interactive,
            &mut confirmer,
        ),
    )?;

    match outcome {
        UpdateOutcome::UpToDate {
            revision_description,
            ignored_answers,
        } => {
            ctx.say(format!(
                "Already up to date with {} at {revision_description}.",
                config.template.source
            ));
            report_ignored(&ctx, &ignored_answers);
        }

        UpdateOutcome::Updated {
            id,
            changes,
            previous_revision_description,
            revision_description,
            answers_changed,
            ignored_answers,
            ..
        } => {
            report_ignored(&ctx, &ignored_answers);
            ctx.blank();
            ctx.say(field(&ctx.theme, "Template", &config.template.source));
            ctx.say(field(
                &ctx.theme,
                "Revision",
                &match previous_revision_description {
                    Some(previous) => format!("{previous} → {revision_description}"),
                    None => revision_description.clone(),
                },
            ));
            ctx.blank();
            ctx.say(headline(&ctx.theme, "Updated", &id.ref_name()));
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

    Ok(crate::exit::SUCCESS)
}

fn dry_run(
    ctx: &Session,
    config: &tpl::config::Config,
    args: &UpdateArgs,
    overrides: std::collections::BTreeMap<String, tpl::template::Value>,
    interactive: bool,
) -> Result<(), OpError> {
    let mut answers = config.answers.clone();
    answers.extend(overrides);

    // The same two helpers the real run uses. Reimplementing them here dropped
    // the `--defaults` term, so `update --dry-run --defaults` prompted and
    // asked to confirm remote sources — which is exactly what `--defaults`
    // exists to prevent.
    //
    // `--dry-run` still fetches: it reports what *would* change, and a render
    // that skipped its data would report a different tree than the real one.
    let mut prompter = Interactive;
    let mut confirmer = Confirmer;
    let rendered = ops::render(
        &ctx.repo,
        &ctx.root,
        config,
        answers,
        args.dirty,
        &ctx.user,
        answering(&args.answers, interactive, &mut prompter),
        trust(&args.answers, args.trust, interactive, &mut confirmer),
    )?;

    report_ignored(ctx, &rendered.ignored_answers);

    let (id, ref_name) = ops::identify(&ctx.root)?;
    let tip = ctx.repo.resolve_ref(&ref_name)?;
    let previous_tree = match tip {
        Some(oid) => Some(ctx.repo.commit(oid)?.tree),
        None => None,
    };

    if previous_tree == Some(rendered.tree) {
        ctx.say("Already up to date. Nothing would change.");
        return Ok(());
    }

    let changes = ctx.repo.diff_trees(previous_tree, rendered.tree)?;

    ctx.blank();
    ctx.say(field(&ctx.theme, "Template", &config.template.source));
    ctx.say(field(
        &ctx.theme,
        "Revision",
        &ops::describe_revision(&rendered.template.reference, rendered.template.revision),
    ));
    ctx.blank();
    ctx.say(headline(&ctx.theme, "Would update", &id.ref_name()));
    ctx.blank();
    for c in &changes {
        ctx.say(change(&ctx.theme, c.kind, &c.path));
    }
    ctx.blank();
    ctx.say(muted(&ctx.theme, "Nothing was written."));
    Ok(())
}
