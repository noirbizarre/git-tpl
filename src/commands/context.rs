//! `git tpl context`
//!
//! What a template actually sees, and a way to try one expression against it.
//!
//! Checking a filter chain — does argument-less `select` drop falsy values?
//! does `map('trim')` work on this? — otherwise costs a whole render each
//! time, and the answer is buried in the output rather than stated.

use tpl::ops::{self, OpError, Target};

use super::{
    Standalone, answering, enforce_strict_answers, report_ignored, report_ignored_paths, supplied,
    trust,
};
use crate::cli::{ContextArgs, GlobalArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::{heading, muted};

pub fn run(args: ContextArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Standalone::new(global)?;
    let source = ctx.user.expand(&args.template).into_owned();

    let mut prompter = Interactive;
    let mut confirmer = Confirmer;

    let rendered = ops::render_files(
        Target {
            source: &source,
            reference: args.r#ref.as_deref(),
            root: args.root.as_deref(),
            dirty: args.dirty,
        },
        None,
        supplied(&args.answers)?,
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

    // One expression, evaluated against the resolved context. This is the REPL
    // a templating language otherwise makes you do without, and it is the
    // whole reason to run this command interactively.
    if let Some(expression) = &args.eval {
        let partials = rendered.template.partials()?;
        let value = tpl::eval::evaluate(expression, &rendered.context, "--eval", &partials)?;

        if global.json {
            println!(
                "{}",
                crate::report::success(serde_json::json!({
                    "expression": expression,
                    "type": value.type_name(),
                    "value": value,
                }))
            );
        } else {
            // The type as well as the value: `"1"` and `1` print identically
            // and behave differently, which is the bug being debugged about
            // half the time.
            ctx.out
                .say(muted(&ctx.out.theme, &format!("({})", value.type_name())));
            println!("{}", serde_json::to_string(&value).unwrap_or_default());
        }
        return Ok(crate::exit::SUCCESS);
    }

    if global.json {
        let mut payload = rendered.context.to_json();
        if let serde_json::Value::Object(map) = &mut payload {
            // Always present, `chain` empty for a template with no
            // `[extends]`: a script should be able to index this without
            // first checking whether the key exists (the same rule
            // `status --json` follows for its own optional fields).
            map.insert("extends".to_string(), extends_json(&rendered.template));
        }
        println!("{}", crate::report::success(payload));
        return Ok(crate::exit::SUCCESS);
    }

    let section = |title: &str, map: &std::collections::BTreeMap<String, tpl::template::Value>| {
        ctx.out.blank();
        ctx.out.say(heading(&ctx.out.theme, title));
        if map.is_empty() {
            ctx.out.say(muted(&ctx.out.theme, "  (none)"));
        }
        for (key, value) in map {
            ctx.out.say(format!(
                "  {key} = {}",
                serde_json::to_string(value).unwrap_or_default()
            ));
        }
    };

    section("Answers", rendered.context.answers());
    section("Computed", rendered.context.computed());
    section("Gated defaults", rendered.context.gated_defaults());
    section("Template", rendered.context.template());
    section("Data", rendered.context.data());

    Ok(crate::exit::SUCCESS)
}

/// The `[extends]` chain, and which layer of it declared each question or
/// data source currently in effect.
///
/// `Context` itself carries no such metadata — it is a flat `name -> value`
/// map, on purpose (ADR-006: no runtime context, and no reason to widen it
/// for this) — so this is built separately, straight from `Resolved`, the
/// one place a chain several layers deep is still fully in hand. `question`
/// and `data` map a name to an index into `chain`, so a caller debugging an
/// override does not have to reason about anything but this one payload.
fn extends_json(template: &ops::Resolved) -> serde_json::Value {
    let chain: Vec<serde_json::Value> = template
        .extends_provenance()
        .iter()
        .map(|ancestor| {
            serde_json::json!({
                "source": ancestor.source,
                "revision": ancestor.revision.to_hex(),
            })
        })
        .collect();

    serde_json::json!({
        "chain": chain,
        "questions": template.question_origin(),
        "data": template.data_origin(),
    })
}
