//! `git tpl init`

use std::collections::BTreeMap;
use std::path::Path;

use tpl::git::libgit2::LibGit2;
use tpl::git::{GitBackend, MergeOutcome};
use tpl::gitconfig::{Overrides, Preferences};
use tpl::ops::{self, OpError};
use tpl::userconfig::UserConfig;

use super::{
    Reporter, Session, Standalone, answering, current_dir, enforce_strict_answers, report_ignored,
    report_ignored_paths, supplied, trust,
};
use crate::cli::{GlobalArgs, InitArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{change, command, field, heading, headline, muted, note_block, warning};

pub fn run(args: InitArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    // Where the project goes. Not a `chdir` — see `Session::discover_at`: a
    // relative template path and a relative `--answers-from` stay relative to
    // where the command was typed, and a `chdir` would reinterpret both
    // without saying so.
    let destination = match &args.directory {
        Some(dir) => dir.clone(),
        None => current_dir()?,
    };

    if args.dry_run {
        // A dry run creates nothing, so `--init` is deliberately not honoured
        // here, and a destination that is not yet a repository previews
        // without a project to seed from rather than failing.
        let base = Standalone::new(global)?;
        let repo = LibGit2::discover(&destination).ok();
        let root = repo.as_ref().and_then(|r| r.workdir().ok());
        let project = repo
            .as_ref()
            .zip(root.as_deref())
            .map(|(r, p)| (r as &dyn GitBackend, p));

        let answers = supplied(&args.answers)?;
        // Expanded here, at the edge, and not in `ops`. See the comment on the
        // non-dry-run expansion below — the same rule applies to a preview.
        let template = base.user.expand(&args.template).into_owned();
        let payload = dry_run(&base.out, &base.user, project, &args, &template, answers)?;
        if global.json {
            println!("{}", crate::report::success(payload));
        }
        return Ok(crate::exit::SUCCESS);
    }

    // `--init` has to happen before discovery, since there may be no
    // repository — and now no directory — to discover yet.
    if args.init {
        // `create_dir_all`, so `init <template> a/b --init` behaves like
        // `mkdir -p`, and an already-existing directory is a no-op rather
        // than an error.
        std::fs::create_dir_all(&destination).map_err(|e| write_failed(&destination, &e))?;
        if LibGit2::discover(&destination).is_err() {
            LibGit2::init(&destination)?;
        }
    } else if !destination.exists() {
        // The error #84 is about: an argument clap accepted but that names
        // nothing yet, with the fix — `--init` — said outright.
        return Err(OpError::NoSuchDirectory { path: destination });
    }

    let ctx = Session::discover_at(&destination, global)?;
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
        args.answers.strict_answers,
        &ctx.user,
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

    ctx.out.blank();
    // The expanded URL, not what was typed: this is the line a user copies
    // when reporting a problem, and it is what is now recorded in the project.
    ctx.out.say(field(&ctx.out.theme, "Template", &template));
    ctx.out.say(field(
        &ctx.out.theme,
        "Revision",
        &outcome.revision_description,
    ));
    ctx.out.blank();
    ctx.out
        .say(headline(&ctx.out.theme, "Created", &outcome.id.ref_name()));
    ctx.out.blank();
    for c in &outcome.changes {
        ctx.out.say(change(&ctx.out.theme, c.kind, &c.path));
    }
    ctx.out.blank();

    match &outcome.merge {
        Some(MergeOutcome::Conflicted { paths }) => {
            ctx.out.say(warning(
                &ctx.out.theme,
                "the merge left conflicts. Resolve them and commit:",
            ));
            ctx.out.blank();
            for path in paths {
                ctx.out.say(format!("  {path}"));
            }
            ctx.out.blank();
            ctx.out.say(command(&ctx.out.theme, "git status"));
            ctx.out.say(command(&ctx.out.theme, "git commit"));
        }
        Some(_) => {
            let branch = ctx
                .repo
                .head_branch()?
                .unwrap_or_else(|| "the branch".into());
            ctx.out.say(format!("Merged into {branch}."));
        }
        None => {
            ctx.out.say(muted(
                &ctx.out.theme,
                "The rendered ref was created but not merged.",
            ));
            ctx.out.blank();
            ctx.out.say("Run:");
            ctx.out.say(command(&ctx.out.theme, "git tpl diff"));
            ctx.out.say(command(&ctx.out.theme, "git tpl merge"));
        }
    }

    ctx.out.blank();
    let relative = outcome
        .config_path
        .strip_prefix(&ctx.root)
        .unwrap_or(&outcome.config_path);
    ctx.out.say(muted(
        &ctx.out.theme,
        &if outcome.config_committed {
            format!("Answers recorded in {} and committed.", relative.display())
        } else {
            format!("Answers recorded in {} and staged.", relative.display())
        },
    ));

    // The template's own additions come last, below everything git-tpl did
    // itself. A message is untrusted text, and putting it above the change list
    // would let it be read as a preamble to git-tpl's output rather than as an
    // appendix to it.
    if !outcome.remotes.is_empty() {
        ctx.out.blank();
        for remote in &outcome.remotes {
            match &remote.outcome {
                ops::RemoteOutcome::Added => {
                    ctx.out.say(format!("Remote {} added.", remote.name));
                }
                // Silent. Reporting "nothing happened" on every re-init is the
                // kind of line that trains people to stop reading them.
                ops::RemoteOutcome::Unchanged => {}
                ops::RemoteOutcome::Skipped { existing } => {
                    // `warn`, not `say`: the template asked for something and
                    // did not get it, and the user is the only one who can
                    // decide which URL is right. Both are shown, because that
                    // decision cannot be made without seeing them together.
                    ctx.out.warn(warning(
                        &ctx.out.theme,
                        &format!(
                            "remote {} was left alone.\n  it points at {existing}\n  \
                             the template asked for {}",
                            remote.name, remote.url
                        ),
                    ));
                }
            }
        }
    }

    if let Some(raw) = &outcome.note {
        // Sanitised at the last possible moment and only for the human stream.
        // `--json` gets the text as written, below, because a consumer is not a
        // terminal and has no escape sequences to be attacked through.
        let formatting = if ctx.out.theme.is_colored() {
            tpl::note::Formatting::Allowed
        } else {
            // Covers `--color never`, `NO_COLOR`, `TERM=dumb` and a piped
            // stderr, all of which `Theme::resolve` has already decided. One
            // decision, so the message cannot come to disagree with the rest of
            // the output about whether this is a terminal.
            tpl::note::Formatting::Stripped
        };
        let sanitised = tpl::note::sanitise(raw, formatting);
        if !sanitised.trim().is_empty() {
            ctx.out.blank();
            ctx.out.say(note_block(&ctx.out.theme, &sanitised));
        }
    }

    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "id": outcome.id.as_str(),
                "ref": outcome.id.ref_name(),
                // The expanded URL, for the same reason the prose prints it:
                // it is what was recorded, and a `mine:` shortcut means
                // nothing on anybody else's machine.
                "template": template,
                "revision": outcome.revision_description,
                "commit": outcome.commit.to_hex(),
                "changes": crate::report::changes(&outcome.changes),
                // `null` under `--no-merge`, which is a different thing from
                // a merge that happened and did nothing.
                "merge": outcome.merge.as_ref().map(crate::report::merge),
                "configPath": relative.display().to_string(),
                "configCommitted": outcome.config_committed,
                "ignoredAnswers": outcome.ignored_answers,
                // Raw, unsanitised: escape sequences are a terminal's problem
                // and this stream reaches no terminal. Its prose is not a
                // contract — ADR-016's "no message matching" applies here too,
                // so branch on its presence, never on its text.
                "note": outcome.note,
                "remotes": outcome.remotes.iter().map(declared_remote).collect::<Vec<_>>(),
            }))
        );
    }

    Ok(crate::exit::SUCCESS)
}

/// One declared remote, for `--json`.
fn declared_remote(remote: &ops::DeclaredRemote) -> serde_json::Value {
    let (status, existing) = match &remote.outcome {
        ops::RemoteOutcome::Added => ("added", None),
        ops::RemoteOutcome::Unchanged => ("unchanged", None),
        ops::RemoteOutcome::Skipped { existing } => ("skipped", Some(existing.clone())),
    };
    serde_json::json!({
        "name": remote.name,
        // What the template asked for, always — including when it was refused,
        // which is the case a caller most needs to see it in.
        "url": remote.url,
        "status": status,
        // `null` unless the two disagree, so its presence is the signal.
        "existing": existing,
    })
}

/// The failure to create the destination directory, under the code that
/// means it.
///
/// Mirrors `commands::render::io`: `tpl::git::backend` was used here once, and
/// no Git is involved — a caller branching on `error.code` could not tell a
/// full disk from a libgit2 fault, and `tpl::ops::write_failed` already exists
/// for exactly this.
fn write_failed(path: &Path, error: &std::io::Error) -> OpError {
    OpError::WriteFailed {
        path: path.display().to_string(),
        reason: format!("could not create it: {error}"),
    }
}

/// Report what would be asked and rendered, without creating anything.
///
/// The cheapest way to find a cycle or a typo in an expression, since both are
/// caught when the graph is built rather than when a question is reached.
///
/// `project` is `None` when the destination is not (yet) inside a repository —
/// a dry run creates nothing, so it previews on the template alone rather than
/// requiring `--init` to have already run.
///
/// Returns the machine-readable form so the caller does the single `println!`.
fn dry_run(
    out: &Reporter,
    user: &UserConfig,
    project: Option<(&dyn GitBackend, &Path)>,
    args: &InitArgs,
    source: &str,
    answers: BTreeMap<String, tpl::template::Value>,
) -> Result<serde_json::Value, OpError> {
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
    enforce_strict_answers(
        &args.answers,
        &ignored,
        template.manifest.questions.keys().cloned(),
    )?;
    report_ignored(out, &ignored);

    out.blank();
    out.say(field(&out.theme, "Template", source));
    let revision_description = ops::describe_revision(&template.reference, template.revision);
    out.say(field(&out.theme, "Revision", &revision_description));
    out.blank();
    out.say(heading(
        &out.theme,
        "Questions, in the order they would be asked",
    ));
    out.blank();

    let mut asked = 0;
    // Resolution order, the same order the text list prints: it is the order
    // the answers must be supplied in when a `when` or a `default` references
    // an earlier one, so a caller driving the questionnaire needs it too.
    let mut nodes = Vec::new();
    for node in graph.order() {
        match node.kind {
            tpl::graph::NodeKind::Question => {
                let supplied = answers.contains_key(&node.key);
                let supplied_note = if supplied {
                    muted(&out.theme, "  (supplied)")
                } else {
                    String::new()
                };
                out.say(format!("  {}{supplied_note}", node.key));
                nodes.push(
                    serde_json::json!({ "name": node.key, "kind": "question", "supplied": supplied }),
                );
                asked += 1;
            }
            tpl::graph::NodeKind::Computed => {
                out.say(muted(&out.theme, &format!("  {} (computed)", node.key)));
                nodes.push(
                    serde_json::json!({ "name": node.key, "kind": "computed", "supplied": false }),
                );
            }
            tpl::graph::NodeKind::Data => {
                out.say(muted(&out.theme, &format!("  {} (data source)", node.key)));
                nodes.push(
                    serde_json::json!({ "name": node.key, "kind": "data", "supplied": false }),
                );
            }
        }
    }

    if asked == 0 {
        out.say(muted(&out.theme, "  (none)"));
    }

    // The file list, when the answers are complete enough to render without
    // asking. `update --dry-run` has always shown what would change, and a
    // flag that meant "list the questions" on one command and "list the files"
    // on another is a flag with two meanings.
    //
    // Only under `--defaults`: otherwise producing the list would mean asking
    // the whole questionnaire, which is precisely what a dry run is avoiding.
    let mut files = None;
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
            project,
            answers.clone(),
            user,
            tpl::ops::Answering::Interactive(&mut prompter),
            trust(&args.answers, args.trust, false, &mut confirmer),
        ) {
            out.blank();
            out.say(heading(&out.theme, "Files it would render"));
            for file in &rendered.files {
                out.say(muted(&out.theme, &format!("  {}", file.path)));
            }
            files = Some(
                rendered
                    .files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>(),
            );
        }
    }

    out.blank();
    out.say(muted(&out.theme, "Nothing was created."));

    Ok(serde_json::json!({
        "dryRun": true,
        "template": source,
        "revision": revision_description,
        "questions": nodes,
        // `null`, not `[]`: without `--defaults` the list was never computed,
        // and an empty array would claim it renders nothing.
        "files": files,
        "ignoredAnswers": ignored,
    }))
}
