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

    if args.dry_run {
        return dry_run(&ctx, &args, answers).map(|()| crate::exit::SUCCESS);
    }

    let mut prompter = Interactive;
    let mut confirmer = Confirmer;
    let outcome = ops::init(
        &ctx.repo,
        &ctx.root,
        &args.template,
        args.r#ref.clone(),
        args.id.as_deref(),
        answers,
        args.dirty,
        !args.no_merge,
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
    ctx.say(field(&ctx.theme, "Template", &args.template));
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
    answers: BTreeMap<String, tpl::template::Value>,
) -> Result<(), OpError> {
    let template = ops::resolve::resolve(ops::Request {
        source: &args.template,
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
    ctx.say(field(&ctx.theme, "Template", &args.template));
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

    ctx.blank();
    ctx.say(muted(&ctx.theme, "Nothing was created."));
    Ok(())
}
