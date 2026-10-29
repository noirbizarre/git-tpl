//! `git tpl questions`
//!
//! The answer schema, without asking anything.
//!
//! `init --dry-run` lists question *names*, on stderr, and needs a repository
//! and a network fetch to do it. That is enough to reassure a human and not
//! enough to write an answers file, which is what a caller that cannot answer
//! a prompt actually needs.

use tpl::ops::{self, OpError};
use tpl::template::{Question, is_expression};

use super::Standalone;
use crate::cli::{GlobalArgs, QuestionsArgs};
use crate::theme::{field, heading, muted};

pub fn run(args: QuestionsArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Standalone::new(global)?;
    let source = ctx.user.expand(&args.template).into_owned();

    let ops::Questionnaire {
        template,
        order,
        resolved_choices,
    } = ops::questions(
        ops::Request {
            source: &source,
            reference: args.r#ref.as_deref(),
            root: args.root.as_deref(),
            dirty: args.dirty,
        },
        &ctx.user,
    )?;

    let manifest = &template.manifest;
    let ordered: Vec<(usize, &String, &Question)> = order
        .iter()
        .enumerate()
        .filter_map(|(index, key)| {
            manifest
                .questions
                .get_key_value(key)
                .map(|(name, question)| (index, name, question))
        })
        .collect();

    if global.json {
        println!(
            "{}",
            crate::report::success(serde_json::json!({
                "template": {
                    "name": manifest.name,
                    "description": manifest.description,
                    "root": manifest.root,
                },
                "questions": ordered
                    .iter()
                    .map(|(order, name, question)| describe(order, name, question, resolved_choices.get(*name).map(Vec::as_slice)))
                    .collect::<Vec<_>>(),
                "computed": manifest.computed.keys().collect::<Vec<_>>(),
                "data": manifest.data.iter().map(|(name, decl)| serde_json::json!({
                    "name": name,
                    "source": decl.source,
                    "kind": decl.kind,
                    "format": decl.format,
                    "sha256": decl.sha256,
                })).collect::<Vec<_>>(),
            }))
        );
        return Ok(crate::exit::SUCCESS);
    }

    ctx.out.blank();
    ctx.out
        .say(field(&ctx.out.theme, "Template", &manifest.name));
    if let Some(description) = &manifest.description {
        ctx.out.say(field(&ctx.out.theme, "About", description));
    }
    ctx.out.blank();
    ctx.out.say(heading(
        &ctx.out.theme,
        "Questions, in the order they are asked",
    ));
    if ordered.is_empty() {
        ctx.out.say(muted(&ctx.out.theme, "  (none)"));
    }
    for (_, name, question) in &ordered {
        let mut line = format!("  {name} ({})", question.kind.declared_name());
        if let Some(when) = &question.when {
            line.push_str(&format!(" when {when}"));
        }
        ctx.out.say(line);
    }

    Ok(crate::exit::SUCCESS)
}

fn describe(
    order: &usize,
    name: &str,
    question: &Question,
    resolved: Option<&[tpl::template::Choice]>,
) -> serde_json::Value {
    let default_expression = question.default_expression().map(str::to_string);

    let mut out = serde_json::json!({
        "name": name,
        "order": order,
        "type": question.kind,
        "prompt": question.prompt_for(name),
        "help": question.help,
        "default": question.default,
        // A default may be an expression: `bin_name` defaulting to
        // `"{{ crate }}"` is derived, not literal, and a caller that treated
        // the string as the value would write the wrong answers file.
        "defaultIsExpression": default_expression
            .as_deref()
            .is_some_and(is_expression),
        "when": question.when,
        "defaultWhenSkipped": question.default_when_skipped,
        "pattern": question.pattern,
        "message": question.pattern_message(),
        "defaultFrom": question.default_from,
    });
    let map = out.as_object_mut().expect("object");

    if let Some(choices) = &question.choices {
        map.insert(
            "choices".into(),
            serde_json::Value::Array(
                choices
                    .iter()
                    .map(|choice| {
                        serde_json::json!({
                            "value": choice.value,
                            "label": choice.label,
                            "help": choice.help,
                        })
                    })
                    .collect(),
            ),
        );
    }

    if let Some(reference) = &question.choices_from {
        map.insert(
            "choicesFrom".into(),
            serde_json::Value::String(reference.clone()),
        );
        // Resolved where it can be, so a caller does not have to fetch and
        // parse the data file itself. Only for a source that lives in the
        // template repository: a remote one would mean a network fetch, and a
        // command that reads a manifest should not silently acquire one.
        if let Some(choices) = resolved {
            // Preserve the string-array contract; presentation is additive.
            map.insert(
                "choicesResolved".into(),
                serde_json::json!(
                    choices
                        .iter()
                        .map(|choice| &choice.value)
                        .collect::<Vec<_>>()
                ),
            );
            map.insert(
                "choicesResolvedDetails".into(),
                serde_json::json!(
                    choices
                        .iter()
                        .map(|choice| serde_json::json!({
                            "value": choice.value, "label": choice.label(), "help": choice.help,
                        }))
                        .collect::<Vec<_>>()
                ),
            );
        }
    }

    out
}
