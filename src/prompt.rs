//! The interactive prompter, built on `demand`.

use std::collections::BTreeMap;

use demand::{Confirm, DemandOption, Input, MultiSelect, Select};
use tpl::data::{DataError, Decision, RemoteRequest, TrustGate};
use tpl::eval::{EvalError, Prompter};
use tpl::template::{Choice, Question, QuestionKind, Value};

/// Asks questions on a terminal.
pub struct Interactive;

impl Prompter for Interactive {
    fn ask(
        &mut self,
        name: &str,
        question: &Question,
        default: Option<&Value>,
        seed: Option<&Value>,
        choices: Option<&[Choice]>,
    ) -> Result<Value, EvalError> {
        let title = question.prompt_for(name);
        let help = question.help.as_deref();

        match question.kind {
            QuestionKind::Boolean => {
                let mut confirm = Confirm::new(title)
                    .affirmative("yes")
                    .negative("no")
                    // Pre-select the default so Enter accepts it — which is the
                    // whole point of the template having supplied one.
                    .selected(matches!(default, Some(Value::Bool(true))));
                if let Some(help) = help {
                    confirm = confirm.description(help);
                }
                Ok(Value::Bool(confirm.run().map_err(cancelled)?))
            }

            QuestionKind::Choice => {
                let options = choices.unwrap_or_default();
                let mut select = Select::new(title);
                if let Some(help) = help {
                    select = select.description(help);
                }
                // The `Value` is carried as the option's item, so `run` hands
                // back the typed value rather than a label we would have to
                // match back — which would go wrong the moment two choices
                // rendered to the same text.
                for choice in options {
                    let value = Value::String(choice.value.clone());
                    let mut option = DemandOption::with_label(choice.label(), value.clone())
                        .selected(default == Some(&value));
                    if let Some(help) = &choice.help {
                        option = option.description(help);
                    }
                    select = select.option(option);
                }
                select.run().map_err(cancelled)
            }

            QuestionKind::MultiChoice => {
                let options = choices.unwrap_or_default();
                let preselected: &[Value] = match default {
                    Some(Value::Array(items)) => items,
                    _ => &[],
                };

                let mut select = MultiSelect::new(title);
                if let Some(help) = help {
                    select = select.description(help);
                }
                for choice in options {
                    let value = Value::String(choice.value.clone());
                    let mut option = DemandOption::with_label(choice.label(), value.clone())
                        .selected(preselected.contains(&value));
                    if let Some(help) = &choice.help {
                        option = option.description(help);
                    }
                    select = select.option(option);
                }

                Ok(Value::Array(select.run().map_err(cancelled)?))
            }

            QuestionKind::Integer => {
                let placeholder = default.map(ToString::to_string).unwrap_or_default();
                loop {
                    let text = text_input(title, help, &placeholder)?;
                    match Value::parse_as(&text, "an integer") {
                        Ok(value) => return Ok(value),
                        // Re-ask rather than abort. A typo in one field must
                        // not discard every answer given so far.
                        Err(error) => eprintln!("  {error}"),
                    }
                }
            }

            QuestionKind::String => {
                // The seed wins where there is one: a `default_from` exists
                // precisely so the pre-filled text is the one this user would
                // have typed. Only `string` questions can carry a seed — the
                // manifest refuses it anywhere else — so no other branch here
                // looks at it.
                let placeholder = seed
                    .or(default)
                    .map(ToString::to_string)
                    .unwrap_or_default();

                loop {
                    let value = Value::String(text_input(title, help, &placeholder)?);
                    // Not `demand`'s own validator hook: `text_input` maps an
                    // empty submission back to the placeholder, and a validator
                    // runs on the typed text — so pressing Enter on a
                    // pre-filled prompt would submit an unchecked default.
                    match tpl::eval::validate_pattern(name, question, &value) {
                        Ok(()) => return Ok(value),
                        // Re-ask rather than abort, as the integer branch does.
                        Err(error) => eprintln!("  {error}"),
                    }
                }
            }
        }
    }
}

/// A single-line input, returning the placeholder when nothing was typed.
fn text_input(title: &str, help: Option<&str>, placeholder: &str) -> Result<String, EvalError> {
    let mut input = Input::new(title);
    if let Some(help) = help {
        input = input.description(help);
    }
    if !placeholder.is_empty() {
        input = input.placeholder(placeholder);
    }

    let text = input.run().map_err(cancelled)?;
    Ok(if text.trim().is_empty() {
        // `demand` shows the placeholder but does not return it, so an empty
        // submission has to be mapped back to the default here. Without this,
        // pressing Enter on a pre-filled prompt would answer with "".
        placeholder.to_string()
    } else {
        text
    })
}

/// Treat an interrupted read as a clean cancellation.
///
/// `demand` reports Ctrl-C and Esc as `ErrorKind::Interrupted`. Surfacing that
/// as an I/O error would print a page of diagnostics for what is simply the
/// user changing their mind.
fn cancelled(_error: std::io::Error) -> EvalError {
    EvalError::Cancelled
}

/// Confirms a template's remote data sources on a terminal.
///
/// Rendering never requires trust: no template can execute anything, trusted or
/// not. This gates only what a template asks *git-tpl* to do on its behalf, and
/// today that is exactly one thing — fetch a URL.
pub struct Confirmer;

impl TrustGate for Confirmer {
    fn confirm(
        &mut self,
        requests: &[RemoteRequest],
        limit_bytes: u64,
    ) -> Result<BTreeMap<String, Decision>, DataError> {
        // Everything is listed before anything is asked, so the decision is
        // made against the whole picture rather than one URL at a time with no
        // idea how many follow.
        eprintln!();
        eprintln!(
            "This template wants to fetch {} remote data source{}:",
            requests.len(),
            if requests.len() == 1 { "" } else { "s" }
        );
        eprintln!();
        for request in requests {
            eprintln!("  {}  {}", request.name, request.source);
        }
        eprintln!();
        eprintln!(
            "Each response is limited to {} KiB and is parsed as data — never executed.",
            limit_bytes / 1024
        );
        eprintln!();

        let mut decisions = BTreeMap::new();
        for request in requests {
            let choice = Select::new(format!("Fetch `{}`?", request.name))
                .description(&request.source)
                .option(DemandOption::new("fetch").label("Fetch it"))
                .option(
                    DemandOption::new("skip")
                        .label("Skip it — the render will fail if it is needed"),
                )
                .option(DemandOption::new("abort").label("Abort"))
                .run()
                .map_err(|_| DataError::Cancelled)?;

            match choice {
                "fetch" => decisions.insert(request.name.clone(), Decision::Allow),
                "skip" => decisions.insert(request.name.clone(), Decision::Skip),
                _ => return Err(DataError::Cancelled),
            };
        }

        Ok(decisions)
    }
}
