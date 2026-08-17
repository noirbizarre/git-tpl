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
    for file in &rendered.files {
        write_file(&args.output, file)?;
    }

    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "template": {
                    "name": rendered.template.manifest.name,
                    "description": rendered.template.manifest.description,
                },
                "revision": {
                    "reference": rendered.template.reference,
                    "commit": rendered.template.revision.to_hex(),
                    "dirty": rendered.template.dirty,
                },
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
/// Cleared rather than merged into, because a template that stops producing a
/// file must be seen to stop: rendering over a previous run would leave the
/// old file behind, and the author would conclude the conditional works.
fn prepare_output(output: &Path, force: bool) -> Result<u8, OpError> {
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
                std::fs::remove_dir_all(output).map_err(|e| io(output, "clear", &e))?;
                std::fs::create_dir_all(output).map_err(|e| io(output, "create", &e))?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(output).map_err(|e| io(output, "create", &e))?;
        }
        Err(error) => return Err(io(output, "read", &error)),
    }
    Ok(crate::exit::SUCCESS)
}

fn write_file(output: &Path, file: &tpl::render::Rendered) -> Result<(), OpError> {
    // `Rendered::path` is already validated by the renderer: no absolute
    // segment, no `..`, no separator inside a segment. Joining is safe because
    // of that check, not in spite of it.
    let target = output.join(&file.path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(parent, "create", &e))?;
    }
    std::fs::write(&target, &file.content).map_err(|e| io(&target, "write", &e))?;

    #[cfg(unix)]
    if file.executable {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&target)
            .map_err(|e| io(&target, "read the permissions of", &e))?
            .permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        std::fs::set_permissions(&target, permissions)
            .map_err(|e| io(&target, "set the permissions of", &e))?;
    }

    Ok(())
}

fn io(path: &Path, verb: &str, error: &std::io::Error) -> OpError {
    OpError::Git(tpl::git::GitError::Backend {
        context: format!("{verb} `{}`", path.display()),
        reason: error.to_string(),
    })
}
