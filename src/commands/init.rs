//! `git tpl init`

use std::collections::BTreeMap;

use tpl::git::libgit2::LibGit2;
use tpl::git::{GitBackend, MergeOutcome};
use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::{self, OpError};

use super::{Session, answering, current_dir, report_ignored, supplied, trust};
use crate::cli::{GlobalArgs, InitArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{change, command, field, heading, headline, muted, warning};

pub fn run(args: InitArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    // `--init` has to happen before discovery, since there may be no
    // repository to discover yet.
    if args.init {
        let cwd = current_dir()?;
        if LibGit2::discover(&cwd).is_err() {
            LibGit2::init(&cwd)?;
        }
    }

    let ctx = Session::discover(global)?;
    let preferences = Preferences::load(&ctx.repo)?.with_overrides(Overrides {
        // `init` has no `--remote` and no `--push`; `--defaults` is the only
        // preference it can override, and it must, or `tpl.interactive true`
        // would keep `--defaults` from meaning what it says.
        non_interactive: args.answers.defaults,
        ..Default::default()
    });

    let answers = supplied(&args.answers)?;

    // Expanded here, at the edge, and not in `ops`. Everything below this line
    // sees the real URL, which makes the rule structural rather than a thing to
    // remember: a shortcut can only ever match what was typed on the command
    // line, never a value read out of a repository. The expanded form is what
    // `ops::init` records in `.config/git.tpl.toml` and what derives
    // `refs/tpl/<id>`, so a shortcut never leaves this machine — otherwise a
    // project created by someone with a `mine:` shortcut would be unusable by
    // everyone else.
    let template = ctx.user.expand(&args.template).into_owned();

    if args.dry_run {
        return dry_run(&ctx, &args, &template, answers).map(|()| crate::exit::SUCCESS);
    }

    let mut prompter = Interactive;
    let mut confirmer = Confirmer;
    let outcome = ops::init(
        &ctx.repo,
        &ctx.root,
        &template,
        args.r#ref.clone(),
        args.id.as_deref(),
        answers,
        args.dirty,
        !args.no_merge,
        args.force,
        &ctx.user,
        answering(&args.answers, preferences.interactive, &mut prompter),
        trust(
            &args.answers,
            args.trust,
            preferences.interactive,
            &mut confirmer,
        ),
    )?;

    report_ignored(&ctx, &outcome.ignored_answers);

    ctx.blank();
    // The expanded URL, not what was typed: this is the line a user copies
    // when reporting a problem, and it is what is now recorded in the project.
    ctx.say(field(&ctx.theme, "Template", &template));
    ctx.say(field(&ctx.theme, "Revision", &outcome.revision_description));
    ctx.blank();
    ctx.say(headline(&ctx.theme, "Created", &outcome.id.ref_name()));
    ctx.blank();
    for c in &outcome.changes {
        ctx.say(change(&ctx.theme, c.kind, &c.path));
    }
    ctx.blank();

    match &outcome.merge {
        Some(MergeOutcome::Conflicted { paths }) => {
            ctx.say(warning(
                &ctx.theme,
                "the merge left conflicts. Resolve them and commit:",
            ));
            ctx.blank();
            for path in paths {
                ctx.say(format!("  {path}"));
            }
            ctx.blank();
            ctx.say(command(&ctx.theme, "git status"));
            ctx.say(command(&ctx.theme, "git commit"));
        }
        Some(_) => {
            let branch = ctx
                .repo
                .head_branch()?
                .unwrap_or_else(|| "the branch".into());
            ctx.say(format!("Merged into {branch}."));
        }
        None => {
            ctx.say(muted(
                &ctx.theme,
                "The rendered ref was created but not merged.",
            ));
            ctx.blank();
            ctx.say("Run:");
            ctx.say(command(&ctx.theme, "git tpl diff"));
            ctx.say(command(&ctx.theme, "git tpl merge"));
        }
    }

    ctx.blank();
    let relative = outcome
        .config_path
        .strip_prefix(&ctx.root)
        .unwrap_or(&outcome.config_path);
    ctx.say(muted(
        &ctx.theme,
        &if outcome.config_committed {
            format!("Answers recorded in {} and committed.", relative.display())
        } else {
            format!("Answers recorded in {} and staged.", relative.display())
        },
    ));

    Ok(crate::exit::SUCCESS)
}

/// Report what would be asked and rendered, without creating anything.
///
/// The cheapest way to find a cycle or a typo in an expression, since both are
/// caught when the graph is built rather than when a question is reached.
fn dry_run(
    ctx: &Session,
    args: &InitArgs,
    source: &str,
    answers: BTreeMap<String, tpl::template::Value>,
) -> Result<(), OpError> {
    let template = ops::resolve::resolve(ops::Request {
        source,
        reference: args.r#ref.as_deref(),
        root: None,
        dirty: args.dirty,
    })?;
    let graph = tpl::graph::Graph::build(&template.manifest)?;

    // The same rule `ops::render` applies, and for the same reason — a dry run
    // exists to find exactly this sort of mistake before anything is written.
    let ignored: Vec<String> = answers
        .keys()
        .filter(|key| !template.manifest.questions.contains_key(*key))
        .cloned()
        .collect();
    report_ignored(ctx, &ignored);

    ctx.blank();
    ctx.say(field(&ctx.theme, "Template", source));
    ctx.say(field(
        &ctx.theme,
        "Revision",
        &ops::describe_revision(&template.reference, template.revision),
    ));
    ctx.blank();
    ctx.say(heading(
        &ctx.theme,
        "Questions, in the order they would be asked",
    ));
    ctx.blank();

    let mut asked = 0;
    for node in graph.order() {
        match node.kind {
            tpl::graph::NodeKind::Question => {
                let supplied_note = if answers.contains_key(&node.key) {
                    muted(&ctx.theme, "  (supplied)")
                } else {
                    String::new()
                };
                ctx.say(format!("  {}{supplied_note}", node.key));
                asked += 1;
            }
            tpl::graph::NodeKind::Computed => {
                ctx.say(muted(&ctx.theme, &format!("  {} (computed)", node.key)));
            }
            tpl::graph::NodeKind::Data => {
                ctx.say(muted(&ctx.theme, &format!("  {} (data source)", node.key)));
            }
        }
    }

    if asked == 0 {
        ctx.say(muted(&ctx.theme, "  (none)"));
    }

    // The file list, when the answers are complete enough to render without
    // asking. `update --dry-run` has always shown what would change, and a
    // flag that meant "list the questions" on one command and "list the files"
    // on another is a flag with two meanings.
    //
    // Only under `--defaults`: otherwise producing the list would mean asking
    // the whole questionnaire, which is precisely what a dry run is avoiding.
    if args.answers.defaults {
        let mut prompter = tpl::eval::DefaultsOnly;
        let mut confirmer = crate::prompt::Confirmer;
        if let Ok(rendered) = ops::render_files(
            ops::Target {
                source,
                reference: args.r#ref.as_deref(),
                root: None,
                dirty: args.dirty,
            },
            Some((&ctx.repo, &ctx.root)),
            answers.clone(),
            &ctx.user,
            tpl::ops::Answering::Interactive(&mut prompter),
            trust(&args.answers, args.trust, false, &mut confirmer),
        ) {
            ctx.blank();
            ctx.say(heading(&ctx.theme, "Files it would render"));
            for file in &rendered.files {
                ctx.say(muted(&ctx.theme, &format!("  {}", file.path)));
            }
        }
    }

    ctx.blank();
    ctx.say(muted(&ctx.theme, "Nothing was created."));
    Ok(())
}
