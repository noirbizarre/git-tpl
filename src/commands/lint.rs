//! `git tpl lint`
//!
//! Static analysis of a template, with no project, no network and no render.
//!
//! It exists because the renderer can only report on the answer set it was
//! given, and the failures that hurt most are the ones a particular answer set
//! never reaches: a syntax error in an untaken branch, a conditional path
//! segment that renders to `.yaml`, a `${{ }}` MiniJinja quietly ate.

use tpl::lint::{Levels, Severity, Verdict};
use tpl::ops::{self, OpError};

use super::Standalone;
use crate::cli::{GlobalArgs, LintArgs};
use crate::theme::{heading, muted, warning};

pub fn run(args: LintArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Standalone::new(global)?;
    let source = ctx.user.expand(&args.template).into_owned();

    // Before anything is resolved or walked: a misspelled `-D` should cost a
    // message, not a clone of the template repository.
    let levels = Levels::parse(&args.deny, &args.allow).map_err(OpError::from)?;

    let template = ops::resolve::resolve(ops::Request {
        source: &source,
        reference: args.r#ref.as_deref(),
        root: args.root.as_deref(),
        dirty: args.dirty,
    })?;

    let entries = template.entries()?;
    // The whole repository, not just the render root: a `note_file` names a
    // path beside the manifest, in the same namespace a partial lives in.
    let repo_entries = template.repo.list_tree(template.tree)?;
    let partials = template.partials()?;
    let findings = tpl::lint::lint(
        template.repo.as_ref(),
        &template.manifest,
        &entries,
        &repo_entries,
        &partials,
    )?;

    let verdicts = levels.apply(findings);

    let errors = verdicts
        .iter()
        .filter(|v| v.finding.severity == Severity::Error)
        .count();
    let denied = verdicts.iter().filter(|v| v.denied).count();

    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "template": template.manifest.name,
                "diagnostics": verdicts.iter().map(|v| serde_json::json!({
                    "severity": v.finding.severity.as_str(),
                    "code": v.finding.code,
                    "message": v.finding.message,
                    "help": v.finding.help,
                    "path": v.finding.path,
                    // The severity is the template's, `denied` is this run's
                    // policy. Rewriting the first would lose the difference.
                    "denied": v.denied,
                })).collect::<Vec<_>>(),
                "errors": errors,
                "warnings": verdicts.len() - errors,
                "denied": denied,
            }))
        );
    } else {
        report(&ctx, &verdicts, errors, denied);
    }

    // Warnings alone are not a failure by default: they are things a template
    // may legitimately mean, and a lint that fails on them is a lint people
    // stop running. `--deny` is how a template that has decided otherwise says
    // so, per repository rather than for everyone.
    Ok(if errors > 0 || denied > 0 {
        crate::exit::FAILURE
    } else {
        crate::exit::SUCCESS
    })
}

fn report(ctx: &Standalone, verdicts: &[Verdict], errors: usize, denied: usize) {
    ctx.out.blank();
    if verdicts.is_empty() {
        ctx.out.say(muted(&ctx.out.theme, "No problems found."));
        return;
    }

    for Verdict { finding, denied } in verdicts {
        let label = match finding.severity {
            Severity::Error => format!("error[{}]", finding.code),
            // Still labelled a warning, with the promotion stated: the code is
            // what a reader needs to look up, and the marker says why the
            // command is about to fail on it.
            Severity::Warning if *denied => format!("warning[{}] (denied)", finding.code),
            Severity::Warning => format!("warning[{}]", finding.code),
        };
        ctx.out.say(heading(&ctx.out.theme, &label));
        if let Some(path) = &finding.path {
            ctx.out.say(muted(&ctx.out.theme, &format!("  {path}")));
        }
        ctx.out.say(format!("  {}", finding.message));
        ctx.out
            .say(muted(&ctx.out.theme, &format!("  help: {}", finding.help)));
        ctx.out.blank();
    }

    let warnings = verdicts.len() - errors;
    ctx.out.say(warning(
        &ctx.out.theme,
        &format!("{errors} error(s), {warnings} warning(s)"),
    ));
    if denied > 0 {
        ctx.out.say(warning(
            &ctx.out.theme,
            &format!("{denied} warning(s) denied, which fails the lint"),
        ));
    }
}
