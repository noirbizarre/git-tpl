//! `git tpl render`
//!
//! One template, one answer set, a directory. No project, no ref, no merge.
//!
//! This is the primitive a template author needs and the ref model does not
//! provide: everything else renders *into* a repository, so asking "what does
//! this template produce?" meant creating one first. Writing to a directory is
//! not a hole in invariant 1 — that invariant is about `update` never touching
//! `HEAD`, the index or the worktree, and this is a different command whose
//! entire purpose is stated in a required flag.

use std::path::Path;

use tpl::ops::{self, OpError, Target};

use super::{
    Standalone, answering, enforce_strict_answers, report_ignored, report_ignored_paths, supplied,
    trust,
};
use crate::cli::{GlobalArgs, RenderArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{field, headline, muted};

pub fn run(args: RenderArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Standalone::new(global)?;
    let source = ctx.user.expand(&args.template).into_owned();

    let supplied = supplied(&args.answers)?;

    // Nobody is prompted when the answers are complete, but a template with a
    // question this answer set does not cover still has to be answerable — so
    // the prompter is available unless `--defaults` says otherwise, exactly as
    // it is for `init`.
    let mut prompter = Interactive;
    let mut confirmer = Confirmer;

    let rendered = ops::render_files(
        Target {
            source: &source,
            reference: args.r#ref.as_deref(),
            root: args.root.as_deref(),
            dirty: args.dirty,
        },
        // No project. A `local` data source is refused rather than resolved
        // against the working directory, and no prompt is seeded from git
        // config — see `ops::render_files`.
        None,
        supplied,
        &ctx.user,
        answering(&args.answers, true, &mut prompter),
        trust(&args.answers, args.trust, true, &mut confirmer),
    )?;

    enforce_strict_answers(
        &args.answers,
        &rendered.ignored_answers,
        rendered.template.manifest.questions.keys().cloned(),
    )?;
    report_ignored(&ctx.out, &rendered.ignored_answers);
    report_ignored_paths(&ctx.out, &rendered.template.ignored);

    prepare_output(&args.output, args.force)?;
    ops::materialise(
        &args.output,
        rendered
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.content.as_slice(), file.executable)),
        &io,
    )?;

    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "template": {
                    "name": rendered.template.manifest.name,
                    "description": rendered.template.manifest.description,
                },
                "revision": crate::report::revision(
                    Some(&rendered.template.reference),
                    Some(rendered.template.revision),
                    Some(rendered.template.dirty),
                ),
                "output": args.output,
                "files": rendered.files.iter().map(|f| serde_json::json!({
                    "path": f.path,
                    "bytes": f.content.len(),
                    "executable": f.executable,
                    // Whether the file went through MiniJinja at all. This is
                    // how an author confirms that a workflow full of `${{ }}`
                    // really was copied byte-for-byte rather than rendered.
                    "templated": f.templated,
                })).collect::<Vec<_>>(),
                "ignoredAnswers": rendered.ignored_answers,
                "skippedByGitignore": rendered.template.ignored,
            }))
        );
        return Ok(crate::exit::SUCCESS);
    }

    ctx.out.blank();
    ctx.out.say(field(
        &ctx.out.theme,
        "Template",
        &rendered.template.manifest.name,
    ));
    ctx.out.say(field(
        &ctx.out.theme,
        "Revision",
        &ops::describe_revision(&rendered.template.reference, rendered.template.revision),
    ));
    ctx.out.blank();
    ctx.out.say(headline(
        &ctx.out.theme,
        "Rendered",
        &format!(
            "{} file{} into {}",
            rendered.files.len(),
            if rendered.files.len() == 1 { "" } else { "s" },
            args.output.display()
        ),
    ));
    for file in &rendered.files {
        ctx.out
            .say(muted(&ctx.out.theme, &format!("  {}", file.path)));
    }

    Ok(crate::exit::SUCCESS)
}

/// Make `output` exist and be empty.
///
/// The `--force` guard is here rather than in `ops::clear_directory` because
/// it is a policy of this command: refusing to replace a directory somebody
/// did not say could be replaced. Clearing it, once permitted, is the shared
/// operation.
fn prepare_output(output: &Path, force: bool) -> Result<(), OpError> {
    match std::fs::read_dir(output) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                if !force {
                    return Err(OpError::InvalidArgument {
                        message: format!(
                            "`{}` is not empty. Pass --force to replace its contents.",
                            output.display()
                        ),
                    });
                }
                ops::clear_directory(output, &io)?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(output).map_err(|e| io(output, "create", &e))?;
        }
        Err(error) => return Err(io(output, "read", &error)),
    }
    Ok(())
}

/// The failure to write the output directory, under the code that means it.
///
/// `tpl::git::backend` was used here, and no Git is involved: a caller
/// branching on `error.code` — the contract `docs/reference/json.md` states —
/// could not tell a full disk from a libgit2 fault, and the catalogue
/// describes that code in Git terms. `tpl::ops::write_failed` already exists
/// for exactly this, and its help names the directory rather than a remote.
fn io(path: &Path, verb: &str, error: &std::io::Error) -> OpError {
    OpError::WriteFailed {
        path: path.display().to_string(),
        // The verb rides in the reason: "write" and "set the permissions of"
        // fail for different causes, and the path alone does not say which was
        // attempted.
        reason: format!("could not {verb} it: {error}"),
    }
}
