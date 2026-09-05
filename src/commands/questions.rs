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

    let ops::Questionnaire { template, order } = ops::questions(
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
                    .map(|(order, name, question)| describe(&template, order, name, question))
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
    template: &ops::Resolved,
    order: &usize,
    name: &str,
    question: &Question,
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
        if let Some(values) = resolve_template_choices(template, reference) {
            map.insert("choicesResolved".into(), serde_json::Value::Array(values));
        }
    }

    out
}

/// The values behind a `choices_from`, when they are in the template itself.
///
/// Returns `None` for anything remote, anything project-local, or a reference
/// that does not point at a plain array of strings — every one of which is a
/// case where the honest answer is "ask the renderer", not a guess.
fn resolve_template_choices(
    template: &ops::Resolved,
    reference: &str,
) -> Option<Vec<serde_json::Value>> {
    let rest = reference.strip_prefix("data.")?;
    let (name, path) = rest.split_once('.')?;
    let decl = template.manifest.data.get(name)?;

    // Only a path inside the template repository. `kind` is inferred when it is
    // absent, and the inference that matters here is: no scheme, no leading
    // `./`, so it is a template file.
    if decl.kind.as_deref().is_some_and(|kind| kind != "template") {
        return None;
    }
    // A `ref` or a `path` makes it a git source whatever `source` looks like,
    // and resolving it would mean a clone. A command that only reads a manifest
    // must not acquire a network call.
    if decl.reference.is_some() || decl.path.is_some() {
        return None;
    }
    if decl.source.contains("://") || decl.source.starts_with('.') {
        return None;
    }
    // An interpolated source depends on answers, which is exactly what has not
    // been collected yet.
    if is_expression(&decl.source) {
        return None;
    }

    // Read through the flattened tree rather than a path lookup, because that
    // is the only accessor the backend exposes and the trees involved are a
    // template's, not a monorepo's.
    let entries = template.repo.list_tree(template.tree).ok()?;
    let entry = entries.iter().find(|entry| entry.path == decl.source)?;
    let bytes = template.repo.read_blob(entry.oid).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let parsed: toml::Value = toml::from_str(&text).ok()?;

    let mut cursor = &parsed;
    for segment in path.split('.') {
        cursor = cursor.get(segment)?;
    }

    Some(
        cursor
            .as_array()?
            .iter()
            .filter_map(|value| value.as_str().map(|s| serde_json::Value::String(s.into())))
            .collect(),
    )
}
