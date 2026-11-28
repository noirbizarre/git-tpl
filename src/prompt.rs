//! Choisir selections/confirmations and dialoguer input with one shared theme.

use std::collections::{BTreeMap, BTreeSet};

use choisir::{Confirm, MultiSelect, SearchMode, Select, theme::MinimalTheme};
use console::Term;
use dialoguer::Input;
use tpl::data::{DataError, Decision, RemoteRequest, SourceKind, TrustGate};
use tpl::eval::{EvalError, Prompter};
use tpl::ops::{Picker, Proposal, Selection, Unsubstituter, Verdict};

use crate::theme::Theme;
use tpl::template::{Choice, Question, QuestionKind, Value};

// Share choisir's preset with dialoguer too, so moving between text, selection
// and confirmation prompts does not change the question/answer presentation.
static PROMPT_THEME: MinimalTheme = MinimalTheme::new();

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
                show_help(help);
                let answer = Confirm::with_theme(&PROMPT_THEME)
                    .with_prompt(title)
                    .default(matches!(default, Some(Value::Bool(true))))
                    .interact_on_opt(&Term::stderr())
                    .map_err(cancelled)?
                    .ok_or(EvalError::Cancelled)?;
                Ok(Value::Bool(answer))
            }

            QuestionKind::Choice => {
                let options = choices.unwrap_or_default();
                show_help(help);
                let mut select = Select::new()
                    .with_theme(&PROMPT_THEME)
                    .with_prompt(title)
                    .search(search(question));
                for (index, choice) in distinct_choices(options).enumerate() {
                    let value = Value::String(choice.value.clone());
                    if default == Some(&value) {
                        select = select.default(index);
                    }
                    // Carry the value, not the label: duplicate display labels
                    // must never cause the wrong answer to be returned.
                    select = if let Some(help) = &choice.help {
                        select.item_with_description(value, choice.label(), help)
                    } else {
                        select.item(value, choice.label())
                    };
                }
                select
                    .interact_on(&Term::buffered_stderr())
                    .map_err(cancelled)
            }

            QuestionKind::MultiChoice => {
                let options = choices.unwrap_or_default();
                let preselected: &[Value] = match default {
                    Some(Value::Array(items)) => items,
                    _ => &[],
                };

                show_help(help);
                let mut select = MultiSelect::new()
                    .with_theme(&PROMPT_THEME)
                    .with_prompt(title)
                    .search(search(question));
                if let Some(min) = question.min_choices {
                    select = select.min(min);
                }
                if let Some(max) = question.max_choices {
                    select = select.max(max);
                }
                let mut defaults = Vec::new();
                for (index, choice) in distinct_choices(options).enumerate() {
                    let value = Value::String(choice.value.clone());
                    if preselected.contains(&value) {
                        defaults.push(index);
                    }
                    select = if let Some(help) = &choice.help {
                        select.item_with_description(value, choice.label(), help)
                    } else {
                        select.item(value, choice.label())
                    };
                }

                Ok(Value::Array(
                    select
                        .defaults(&defaults)
                        .interact_on(&Term::buffered_stderr())
                        .map_err(cancelled)?,
                ))
            }

            QuestionKind::List => {
                let defaults = match default {
                    Some(Value::Array(items)) => items.as_slice(),
                    _ => &[],
                };
                let mut items = Vec::new();
                let mut index = 0;
                loop {
                    // At capacity, remaining defaults must still be explicitly removable.
                    if question.max_items.is_some_and(|max| items.len() >= max)
                        && index >= defaults.len()
                    {
                        break;
                    }
                    let previous = defaults.get(index).and_then(Value::as_str);
                    let item_title = format!("{title} #{}", index + 1);
                    let item_help = if previous.is_some() {
                        Some("Enter keeps the default; '-' removes this item")
                    } else {
                        help.or(Some("Leave blank to finish the list"))
                    };
                    let text = text_input(&item_title, item_help, previous.unwrap_or(""))?;
                    if previous.is_some() && text == "-" {
                        index += 1;
                        continue;
                    }
                    if previous.is_none() && text.trim().is_empty() {
                        match tpl::eval::validate_list(name, question, &Value::Array(items.clone()))
                        {
                            Ok(()) => break,
                            Err(error) => {
                                eprintln!("  {error}");
                                continue;
                            }
                        }
                    }
                    if question.max_items.is_some_and(|max| items.len() >= max) {
                        eprintln!(
                            "  `{name}` allows at most {} items; '-' removes this default",
                            question.max_items.unwrap()
                        );
                        continue;
                    }
                    let item = Value::String(text);
                    match tpl::eval::validate_list_item(name, question, &item, items.len() + 1) {
                        Ok(()) => {
                            items.push(item);
                            index += 1;
                        }
                        // A typo costs one prompt, not every default that followed it.
                        Err(error) => eprintln!("  {error}"),
                    }
                }
                Ok(Value::Array(items))
            }

            QuestionKind::Integer | QuestionKind::Float => {
                let placeholder = default.map(ToString::to_string).unwrap_or_default();
                loop {
                    let text = text_input(title, help, &placeholder)?;
                    match Value::parse_as(&text, question.kind.type_name()) {
                        Ok(value) => match tpl::eval::validate_value(name, question, &value) {
                            Ok(()) => return Ok(value),
                            Err(error) => eprintln!("  {error}"),
                        },
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
                    // Validate after whitespace/default normalization so the
                    // effective answer, not merely the typed text, is checked.
                    match tpl::eval::validate_value(name, question, &value) {
                        Ok(()) => return Ok(value),
                        // Re-ask rather than abort, as the integer branch does.
                        Err(error) => eprintln!("  {error}"),
                    }
                }
            }
        }
    }
}

fn distinct_choices(choices: &[Choice]) -> impl Iterator<Item = &Choice> {
    // Widget bounds count rows. Offer each value once so accepting the widget
    // cannot fail eval's distinct-value bounds; keep the first label/help/order.
    let mut seen = BTreeSet::new();
    choices
        .iter()
        .filter(move |choice| seen.insert(&choice.value))
}

/// A single-line input, returning the placeholder when nothing was typed.
fn text_input(title: &str, help: Option<&str>, placeholder: &str) -> Result<String, EvalError> {
    show_help(help);
    let mut input = Input::<String>::with_theme(&PROMPT_THEME)
        .with_prompt(title)
        .allow_empty(true);
    if !placeholder.is_empty() {
        input = input.default(placeholder.to_string());
    }

    let text = input.interact_text_on(&Term::stderr()).map_err(cancelled)?;
    Ok(if text.trim().is_empty() {
        // Dialoguer handles empty input; keep git-tpl's additional rule that
        // whitespace-only submission also accepts the displayed default.
        placeholder.to_string()
    } else {
        text
    })
}

/// Keep terminal failures at the same cancellation boundary as the old frontend.
fn cancelled(_error: impl std::fmt::Display) -> EvalError {
    EvalError::Cancelled
}

fn search(question: &Question) -> SearchMode {
    if question.fuzzy {
        SearchMode::Fuzzy
    } else {
        SearchMode::Typeahead
    }
}

/// Choisir has per-item descriptions but no prompt description yet. Keep the
/// complete explanation outside its truncated, redrawn frame during this trial.
fn show_help(help: Option<&str>) {
    if let Some(help) = help {
        eprintln!("  {help}");
    }
}

/// Confirms a template's network data sources on a terminal.
///
/// Rendering never requires trust: no template can execute anything, trusted or
/// not. This gates only what a template asks *git-tpl* to do on its behalf, and
/// today that is exactly two things — fetch a URL, and clone a repository.
pub struct Confirmer(pub Theme);

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
                "A repository is fetched with your Git credentials; its objects are retained in the local cache. \
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
            show_help(Some(&request.source));
            // None is the explicit Abort action; a cancelled interaction also
            // aborts, never granting consent or silently skipping a source.
            let choice = Select::new()
                .with_theme(&PROMPT_THEME)
                .with_prompt(format!(
                    "{question} {}?",
                    crate::theme::inline_code(&self.0, &request.name)
                ))
                .item(Some(Decision::Allow), label)
                .item(
                    Some(Decision::Skip),
                    "Skip it — the render will fail if it is needed",
                )
                .item(None, "Abort")
                .interact_on(&Term::buffered_stderr())
                .map_err(|_| DataError::Cancelled)?;

            match choice {
                Some(decision) => decisions.insert(request.name.clone(), decision),
                None => return Err(DataError::Cancelled),
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

/// Confirms reversing one substitution, on a terminal.
///
/// Separate from [`Confirmer`] because substitution consent and network consent
/// implement independent operation-layer capabilities.
pub struct Reverser(pub Theme);

impl Unsubstituter for Reverser {
    fn confirm(&mut self, proposal: &Proposal<'_>) -> Verdict {
        // Everything goes to stderr. Stdout carries the mailbox, and a prompt
        // mixed into it would be piped straight into `git am`.
        //
        // The layout is `theme::reversal`, where it can be tested; what is left
        // here is the question, which cannot be asked without a terminal.
        eprint!("{}", crate::theme::reversal(&self.0, proposal));

        show_help(Some(
            "It renders back to your file exactly. Whether it is right for every other \
                  project is what only you can say.",
        ));
        let taken = Confirm::with_theme(&PROMPT_THEME)
            .with_prompt("Send this line upstream?")
            // Demand's implicit default was affirmative; preserve Enter's meaning.
            .default(true)
            .interact_on_opt(&Term::stderr())
            // Escape, Ctrl-C and read failures decline this proposal rather
            // than interrupt the command before it can report the refused file.
            .ok()
            .flatten()
            .unwrap_or(false);

        if taken {
            Verdict::Accept
        } else {
            Verdict::Decline
        }
    }
}

/// Chooses which hunks of a change to send, on a terminal.
///
/// A type of its own, beside [`Reverser`], for the same reason it is: it needs
/// a [`Theme`], and it is constructed in exactly one command.
pub struct Chooser(pub Theme);

impl Picker for Chooser {
    fn pick(&mut self, selection: &Selection<'_>) -> Option<Vec<usize>> {
        // Everything goes to stderr. Stdout carries the mailbox, and a prompt
        // mixed into it would be piped straight into `git am`.
        //
        // The hunks are printed in full first, and the list below is a list of
        // *labels*: a `MultiSelect` option is one line, and a hunk is not.
        // Showing the bodies where the header numbers them keeps the whole
        // change visible while the choice is being made.
        eprint!("{}", crate::theme::hunks(&self.0, selection));

        show_help(Some(
            "Space toggles, Enter confirms. Everything is selected to begin with.",
        ));
        let mut select = MultiSelect::new()
            .with_theme(&PROMPT_THEME)
            .with_prompt(format!("Send which hunks of `{}`?", selection.path));

        for hunk in selection.hunks {
            // The index travels on the option, not a label matched back: two
            // hunks of a file can render to the same header text, and pairing
            // them up by string is how the wrong one gets dropped.
            select = select.item(
                hunk.index,
                format!(
                    "{}  {}  +{} −{}",
                    hunk.index + 1,
                    hunk.header,
                    hunk.insertions,
                    hunk.deletions
                ),
            );
        }

        // `None`, not an empty selection: `ops::backport` turns it into
        // `Cancelled` and stops. Escape here is the user changing their mind
        // about the whole command, and reading it as "keep no hunks" would
        // emit a patch they never approved. The reason is `cancelled`'s — a
        // page of miette for a keystroke is not a diagnostic — but the shape
        // is different, so it cannot share it.
        // Defaults are option positions, while the carried values are actual
        // hunk indices. Keep those distinct even if they currently coincide.
        select
            .defaults(&(0..selection.hunks.len()).collect::<Vec<_>>())
            .interact_on_opt(&Term::buffered_stderr())
            .ok()
            .flatten()
    }
}
