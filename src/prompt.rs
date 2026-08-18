//! The interactive prompter, built on `demand`.

use std::collections::BTreeMap;

use demand::{Confirm, DemandOption, Input, MultiSelect, Select};
use tpl::data::{DataError, Decision, RemoteRequest, SourceKind, TrustGate};
use tpl::eval::{EvalError, Prompter};
use tpl::ops::{Proposal, Unsubstituter, Verdict};
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

        // The seed wins where there is one. Both of its sources — a question's
        // `default_from`, and the user's own `[defaults]` — exist precisely so
        // the pre-filled answer is the one this person would have given. It is
        // a separate parameter rather than folded into `default` so that a
        // prompter which never asks, `DefaultsOnly`, cannot reach it: a value
        // from this machine must never render without a human accepting it.
        let default = seed.or(default);

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
                let placeholder = default.map(ToString::to_string).unwrap_or_default();

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

/// Confirms a template's network data sources on a terminal.
///
/// Rendering never requires trust: no template can execute anything, trusted or
/// not. This gates only what a template asks *git-tpl* to do on its behalf, and
/// today that is exactly two things — fetch a URL, and clone a repository.
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
            "This template wants to reach the network for {} data source{}:",
            requests.len(),
            if requests.len() == 1 { "" } else { "s" }
        );
        eprintln!();
        for request in requests {
            // The verb is per source. A list that said "fetch" throughout would
            // describe a clone as something it is not, and consent to a
            // misdescription is not consent.
            eprintln!("  {}  {}  {}", verb(request), request.name, request.source);
        }
        eprintln!();
        // Stated only when it is true. The size bound is enforced while reading
        // an HTTP body; nothing bounds a clone, and claiming otherwise would be
        // a promise git-tpl cannot keep.
        if requests.iter().any(|r| r.kind == SourceKind::Remote) {
            eprintln!(
                "Each response is limited to {} KiB and is parsed as data — never executed.",
                limit_bytes / 1024
            );
        }
        if requests.iter().any(|r| r.kind == SourceKind::Git) {
            eprintln!(
                "A repository is cloned with your Git credentials, read, and discarded. \
                 Nothing in it is executed."
            );
        }
        eprintln!();

        let mut decisions = BTreeMap::new();
        for request in requests {
            let (question, label) = match request.kind {
                SourceKind::Git => ("Clone", "Clone it"),
                _ => ("Fetch", "Fetch it"),
            };
            let choice = Select::new(format!("{question} `{}`?", request.name))
                .description(&request.source)
                .option(DemandOption::new("fetch").label(label))
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

/// What git-tpl would do with this source, in one word.
fn verb(request: &RemoteRequest) -> &'static str {
    match request.kind {
        SourceKind::Git => "clone",
        _ => "fetch",
    }
}

impl Unsubstituter for Confirmer {
    fn confirm(&mut self, proposal: &Proposal<'_>) -> Verdict {
        // Everything goes to stderr. Stdout carries the mailbox, and a prompt
        // mixed into it would be piped straight into `git am`.
        eprintln!();
        eprintln!(
            "`{}` line {} was changed around a value the template substitutes.",
            proposal.path, proposal.line
        );
        eprintln!();
        eprintln!("  rendered  {}", proposal.rendered);
        eprintln!("  yours     {}", proposal.project);
        eprintln!("  upstream  {}", proposal.patched);
        eprintln!();
        // Named because it is the thing being kept, and the reason the user is
        // being asked rather than told: if they meant to change what is *in*
        // the placeholder, they meant to change their answer, not the template.
        eprintln!(
            "It keeps {} and sends the rest of the line to `{}`.",
            proposal
                .expressions
                .iter()
                .map(|expression| format!("`{expression}`"))
                .collect::<Vec<_>>()
                .join(" and "),
            proposal.template_path
        );
        eprintln!();

        let taken = Confirm::new("Send this line upstream?")
            .description(
                "It renders back to your file exactly. Whether it is right for every other \
                 project is what only you can say.",
            )
            .affirmative("yes")
            .negative("no")
            .run()
            // Ctrl-C at this prompt is a refusal, not an error. The command
            // refuses the file and says why, which is where the user was
            // heading anyway.
            .unwrap_or(false);

        if taken {
            Verdict::Accept
        } else {
            Verdict::Decline
        }
    }
}
