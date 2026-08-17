//! `git tpl update`

use tpl::git::GitBackend;
use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::{self, OpError, UpdateOutcome};

use super::{Session, answering, report_ignored, report_ignored_paths, supplied, trust};
use crate::cli::{GlobalArgs, UpdateArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{change, command, field, headline, muted, transition};

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
        let payload = dry_run(&ctx, &config, &args, overrides, preferences.interactive)?;
        if global.json {
            println!("{}", crate::report::success(payload));
        }
        return Ok(crate::exit::SUCCESS);
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

    // Built alongside the prose rather than instead of it: `--json` suppresses
    // `say` but not the work, so the two branches cannot report different
    // outcomes. Issue #53 was the opposite arrangement — the `UpToDate` arm
    // said its piece to a silenced stderr and stdout stayed empty, which a
    // caller could not tell apart from the binary producing nothing at all.
    let payload = match outcome {
        UpdateOutcome::UpToDate {
            revision_description,
            ignored_answers,
            ignored,
        } => {
            ctx.out.say(format!(
                "Already up to date with {} at {revision_description}.",
                config.template.source
            ));
            report_ignored(&ctx.out, &ignored_answers);
            report_ignored_paths(&ctx.out, &ignored);

            // The id is not in the outcome, because nothing was created. It is
            // still what the caller asked about, so read it back off disk.
            let (id, ref_name) = ops::identify(&ctx.root)?;
            serde_json::json!({
                "result": "upToDate",
                "id": id.as_str(),
                "ref": ref_name,
                "template": config.template.source,
                "revision": revision_description,
                "ignoredAnswers": ignored_answers,
            })
        }

        UpdateOutcome::Updated {
            id,
            commit,
            changes,
            previous_revision_description,
            revision_description,
            answers_changed,
            ignored_answers,
            ignored,
        } => {
            report_ignored(&ctx.out, &ignored_answers);
            report_ignored_paths(&ctx.out, &ignored);
            ctx.out.blank();
            ctx.out.say(field(&ctx.out.theme, "Template", &config.template.source));
            ctx.out.say(field(
                &ctx.out.theme,
                "Revision",
                &match &previous_revision_description {
                    Some(previous) => {
                        transition(&ctx.out.theme, previous, &revision_description, None)
                    }
                    None => revision_description.clone(),
                },
            ));
            ctx.out.blank();
            ctx.out.say(headline(&ctx.out.theme, "Updated", &id.ref_name()));
            ctx.out.blank();
            for c in &changes {
                ctx.out.say(change(&ctx.out.theme, c.kind, &c.path));
            }
            ctx.out.blank();

            // Stated every time. It is the single most surprising property of
            // the tool, and a user who does not believe it will not use it.
            ctx.out.say("Your working tree was not modified.");

            if answers_changed {
                // The one exception, so it must be said rather than discovered
                // in `git status`.
                ctx.out.say(muted(
                    &ctx.out.theme,
                    "The template added a question, so .config/git.tpl.toml was updated.",
                ));
            }

            ctx.out.blank();
            ctx.out.say("Run:");
            ctx.out.say(command(&ctx.out.theme, "git tpl diff"));
            ctx.out.say(command(&ctx.out.theme, "git tpl merge"));

            // The push happens under `--json` too. Only its prose is silenced;
            // suppressing the push itself would make the flag change behaviour
            // rather than change output.
            let pushed = if preferences.auto_push {
                ctx.out.blank();
                let pushed = ops::push(&ctx.repo, &ctx.root, &preferences)?;
                ctx.out.say(format!("Pushed {pushed} to {}.", preferences.remote));
                Some(preferences.remote.clone())
            } else {
                None
            };

            serde_json::json!({
                "result": "updated",
                "id": id.as_str(),
                "ref": id.ref_name(),
                "template": config.template.source,
                "commit": commit.to_hex(),
                "previousRevision": previous_revision_description,
                "revision": revision_description,
                "changes": crate::report::changes(&changes),
                "answersChanged": answers_changed,
                "ignoredAnswers": ignored_answers,
                "pushed": pushed,
            })
        }
    };

    if global.json {
        println!("{}", crate::report::success(payload));
    }

    Ok(crate::exit::SUCCESS)
}

/// Report what would change, without writing anything.
///
/// Returns the machine-readable form so that the caller does the single
/// `println!`: a dry run that stayed silent under `--json` would reopen the
/// hole this command's payload exists to close.
fn dry_run(
    ctx: &Session,
    config: &tpl::config::Config,
    args: &UpdateArgs,
    overrides: std::collections::BTreeMap<String, tpl::template::Value>,
    interactive: bool,
) -> Result<serde_json::Value, OpError> {
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

    report_ignored(&ctx.out, &rendered.ignored_answers);

    let (id, ref_name) = ops::identify(&ctx.root)?;
    let tip = ctx.repo.resolve_ref(&ref_name)?;
    let previous_tree = match tip {
        Some(oid) => Some(ctx.repo.commit(oid)?.tree),
        None => None,
    };
    let revision_description =
        ops::describe_revision(&rendered.template.reference, rendered.template.revision);

    if previous_tree == Some(rendered.tree) {
        ctx.out.say("Already up to date. Nothing would change.");
        return Ok(serde_json::json!({
            "dryRun": true,
            "result": "upToDate",
            "id": id.as_str(),
            "ref": ref_name,
            "template": config.template.source,
            "revision": revision_description,
            "changes": [],
            "ignoredAnswers": rendered.ignored_answers,
        }));
    }

    let changes = ctx.repo.diff_trees(previous_tree, rendered.tree, &[])?;

    ctx.out.blank();
    ctx.out.say(field(&ctx.out.theme, "Template", &config.template.source));
    ctx.out.say(field(&ctx.out.theme, "Revision", &revision_description));
    ctx.out.blank();
    ctx.out.say(headline(&ctx.out.theme, "Would update", &id.ref_name()));
    ctx.out.blank();
    for c in &changes {
        ctx.out.say(change(&ctx.out.theme, c.kind, &c.path));
    }
    ctx.out.blank();
    ctx.out.say(muted(&ctx.out.theme, "Nothing was written."));

    Ok(serde_json::json!({
        "dryRun": true,
        "result": "wouldUpdate",
        "id": id.as_str(),
        "ref": ref_name,
        "template": config.template.source,
        "revision": revision_description,
        "changes": crate::report::changes(&changes),
        "ignoredAnswers": rendered.ignored_answers,
    }))
}
