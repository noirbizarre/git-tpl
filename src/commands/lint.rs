//! `git tpl lint`
//!
//! Static analysis of a template, with no project, no network and no render.
//!
//! It exists because the renderer can only report on the answer set it was
//! given, and the failures that hurt most are the ones a particular answer set
//! never reaches: a syntax error in an untaken branch, a conditional path
//! segment that renders to `.yaml`, a `${{ }}` MiniJinja quietly ate.

use tpl::lint::{Finding, Severity};
use tpl::ops::{self, OpError};

use super::Standalone;
use crate::cli::{GlobalArgs, LintArgs};
use crate::theme::{heading, muted, warning};

pub fn run(args: LintArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Standalone::new(global)?;
    let source = ctx.user.expand(&args.template).into_owned();

    let template = ops::resolve::resolve(ops::Request {
        source: &source,
        reference: args.r#ref.as_deref(),
        root: args.root.as_deref(),
        dirty: args.dirty,
    })?;

    let entries = template.entries()?;
    let partials = template.partials()?;
    let findings = tpl::lint::lint(
        template.repo.as_ref(),
        &template.manifest,
        &entries,
        &partials,
    )?;

    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();

    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "template": template.manifest.name,
                "diagnostics": findings.iter().map(|f| serde_json::json!({
                    "severity": f.severity.as_str(),
                    "code": f.code,
                    "message": f.message,
                    "help": f.help,
                    "path": f.path,
                })).collect::<Vec<_>>(),
                "errors": errors,
                "warnings": findings.len() - errors,
            }))
        );
    } else {
        report(&ctx, &findings, errors);
    }

    // Warnings alone are not a failure: they are things a template may
    // legitimately mean, and a lint that fails on them is a lint people stop
    // running.
    Ok(if errors > 0 {
        crate::exit::FAILURE
    } else {
        crate::exit::SUCCESS
    })
}

fn report(ctx: &Standalone, findings: &[Finding], errors: usize) {
    ctx.out.blank();
    if findings.is_empty() {
        ctx.out.say(muted(&ctx.out.theme, "No problems found."));
        return;
    }

    for finding in findings {
        let label = match finding.severity {
            Severity::Error => format!("error[{}]", finding.code),
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

    let warnings = findings.len() - errors;
    ctx.out.say(warning(
        &ctx.out.theme,
        &format!("{errors} error(s), {warnings} warning(s)"),
    ));
}
