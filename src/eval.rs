//! Expression evaluation and incremental question resolution.
//!
//! Walks the [`Graph`](crate::graph::Graph) in dependency order, loading data,
//! asking questions and computing values as each becomes resolvable. Conditions
//! are evaluated against everything resolved so far, so a question that does
//! not apply is never shown — rather than asking everything and filtering
//! afterwards.

use std::collections::BTreeMap;
use std::sync::Arc;

use miette::Diagnostic;
use thiserror::Error;

use crate::context::Context;
use crate::data::{DataError, Loader, Rendered};
use crate::graph::{Graph, NodeKind};
use crate::seed::SeedContext;
use crate::template::{
    Choice, Manifest, Question, QuestionKind, Value, computed_expression, is_expression,
};

/// Errors from evaluating a template.
#[derive(Debug, Error, Diagnostic)]
pub enum EvalError {
    /// An expression failed to render.
    #[error("failed to evaluate `{location}`")]
    #[diagnostic(
        code(tpl::eval::expression),
        help("expression: {expression}\nreason:     {reason}")
    )]
    Expression {
        /// Which declaration it came from.
        location: String,
        /// The expression.
        expression: String,
        /// MiniJinja's message.
        reason: String,
    },

    /// A `choices_from` path resolved to something that is not a list.
    #[error("failed to evaluate question `{question}`")]
    #[diagnostic(
        code(tpl::eval::bad_choices),
        help(
            "`{reference}` resolved to {found}; `choices_from` must point at an array of scalars"
        )
    )]
    BadChoices {
        /// The question.
        question: String,
        /// The path that was followed.
        reference: String,
        /// What was found instead.
        found: String,
    },

    /// An answer is not of the question's declared type.
    #[error("answer for `{question}` is not {expected}")]
    #[diagnostic(code(tpl::eval::wrong_type), help("expected {expected}, got {found}"))]
    WrongType {
        /// The question.
        question: String,
        /// The type it declares.
        expected: String,
        /// What was supplied.
        found: String,
    },

    /// An answer is not one of the offered choices.
    #[error("`{value}` is not a valid choice for `{question}`")]
    #[diagnostic(
        code(tpl::eval::invalid_choice),
        help(
            "choose one of: {choices}\nif this answer was recorded by an earlier render, the template no longer offers it — edit `{question}` in `.config/git.tpl.toml`"
        )
    )]
    InvalidChoice {
        /// The question.
        question: String,
        /// The rejected value.
        value: String,
        /// What was on offer.
        choices: String,
    },

    /// An answer does not match the question's `pattern`.
    #[error("`{value}` is not a valid answer for `{question}`")]
    #[diagnostic(
        code(tpl::eval::pattern_mismatch),
        help(
            "{reason}\nif this answer was recorded by an earlier render, the template has since narrowed what it accepts — edit `{question}` in `.config/git.tpl.toml`"
        )
    )]
    PatternMismatch {
        /// The question.
        question: String,
        /// The rejected value.
        value: String,
        /// Why it was rejected — the question's `message`, or its `pattern`.
        reason: String,
    },

    /// A question needs an answer and there is no way to obtain one.
    #[error("no answer for `{question}`")]
    #[diagnostic(
        code(tpl::eval::unanswered),
        help(
            "it has no default and no value was supplied. Pass `--answer {question}=<value>`, or run without `--defaults` to be prompted."
        )
    )]
    Unanswered {
        /// The question.
        question: String,
    },

    /// The user aborted the questionnaire.
    #[error("cancelled")]
    #[diagnostic(code(tpl::eval::cancelled))]
    Cancelled,

    /// A data source failed.
    ///
    /// Fatal rather than recoverable: rendering with a partially-loaded context
    /// produces a plausible tree that is wrong, and that tree becomes a commit.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Data(#[from] DataError),
}

/// How answers are obtained for questions that have none.
pub trait Prompter {
    /// Ask a question, given its resolved default, prompt seed and choices.
    ///
    /// `seed` is separate from `default` on purpose: a prompter that does not
    /// ask must not be able to use one, and a fifth argument it ignores makes
    /// that structural rather than a matter of discipline.
    fn ask(
        &mut self,
        name: &str,
        question: &Question,
        default: Option<&Value>,
        seed: Option<&Value>,
        choices: Option<&[Choice]>,
    ) -> Result<Value, EvalError>;
}

/// A prompter that never asks, taking the default instead.
///
/// Used by `--defaults`, by `tpl.interactive false`, and in tests. A question
/// with no default is an error rather than a silent empty value.
pub struct DefaultsOnly;

impl Prompter for DefaultsOnly {
    fn ask(
        &mut self,
        name: &str,
        _question: &Question,
        default: Option<&Value>,
        // Ignored, deliberately: a seed is prompt-only. A `default_from`
        // value comes from the machine's Git configuration; using it where no
        // human confirms it would make the same template render two different
        // trees on two machines.
        _seed: Option<&Value>,
        _choices: Option<&[Choice]>,
    ) -> Result<Value, EvalError> {
        default.cloned().ok_or_else(|| EvalError::Unanswered {
            question: name.to_string(),
        })
    }
}

/// Inputs to an evaluation run.
pub struct Evaluation<'a> {
    /// The manifest being evaluated.
    pub manifest: &'a Manifest,
    /// Its validated dependency graph.
    pub graph: &'a Graph,
    /// Answers already known, from `.config/git.tpl.toml` or `--answer`.
    pub supplied: BTreeMap<String, Value>,
    /// Prompt seeds, by question name, from `default_from`.
    ///
    /// Empty whenever nobody is being asked. A seed is a machine value — it
    /// may become the prompt's pre-filled text and nothing else, because a
    /// value that varies by machine reaching the tree would end determinism.
    pub seeds: &'a BTreeMap<String, Value>,
    /// The template's shared partials, importable from manifest expressions.
    ///
    /// Present so a `computed` value can `{% import %}` the same macro a
    /// `.jinja` file does. One environment, one set of names, no divergence.
    pub partials: &'a Arc<Partials>,
}

/// Resolve a template's context, prompting where needed.
///
/// A supplied answer skips its prompt but still participates in the graph, so
/// anything depending on it resolves normally.
pub fn resolve(
    evaluation: Evaluation<'_>,
    loader: &mut Loader<'_>,
    prompter: &mut dyn Prompter,
) -> Result<Context, EvalError> {
    let Evaluation {
        manifest,
        graph,
        supplied,
        seeds,
        partials,
    } = evaluation;

    let mut context = Context::new().with_template(manifest.metadata());

    for node in graph.order() {
        match node.kind {
            NodeKind::Data => {
                let Some(decl) = manifest.data.get(&node.key) else {
                    continue;
                };
                // The source may itself be an expression, so it is rendered
                // against the context as it stands. The graph guarantees
                // everything it references is already resolved — including
                // through `ref` and `path`, which are expressions too.
                let resolved = render_string(
                    &decl.source,
                    &context,
                    &format!("data.{}", node.key),
                    partials,
                )?;
                // The locations name the TOML keys the author wrote, `ref` and
                // not the Rust field's `reference`, because these strings end
                // up in the diagnostic.
                let reference = decl
                    .reference
                    .as_deref()
                    .map(|r| {
                        render_string(r, &context, &format!("data.{}.ref", node.key), partials)
                    })
                    .transpose()?;
                let path = decl
                    .path
                    .as_deref()
                    .map(|p| {
                        render_string(p, &context, &format!("data.{}.path", node.key), partials)
                    })
                    .transpose()?;
                let value = loader.load(
                    &node.key,
                    decl,
                    Rendered {
                        source: &resolved,
                        reference: reference.as_deref(),
                        path: path.as_deref(),
                    },
                )?;
                context.set_data(&node.key, value);
            }

            NodeKind::Computed => {
                let Some(declared) = manifest.computed.get(&node.key) else {
                    continue;
                };
                let value = match computed_expression(declared) {
                    Some(expression) => evaluate(
                        expression,
                        &context,
                        &format!("computed.{}", node.key),
                        partials,
                    )?,
                    // A literal keeps its TOML type verbatim, exactly as a
                    // question's literal default does: no engine, no round trip
                    // through a string, so `line_length = 100` stays an integer.
                    None => declared.clone(),
                };
                context.set_computed(&node.key, value);
            }

            NodeKind::Question => {
                let Some(question) = manifest.questions.get(&node.key) else {
                    continue;
                };

                // A question whose condition is false is not asked and gets NO
                // value — absent from the context, not null. That is what lets
                // a template distinguish "not applicable" from "declined" with
                // `cli is defined`.
                if let Some(when) = &question.when {
                    let condition = evaluate(
                        when,
                        &context,
                        &format!("questions.{}.when", node.key),
                        partials,
                    )?;
                    if !condition.is_truthy() {
                        // Opt-in (issue #117, ADR-025): still not asked and
                        // still not an answer, but a file body reading this
                        // name bare now sees the declared default instead of
                        // nothing, for an author who has decided the two
                        // never need to differ here.
                        if question.default_when_skipped
                            && let Some(default) =
                                resolve_default(&node.key, question, &context, partials)?
                        {
                            context.set_gated_default(&node.key, default);
                        }
                        continue;
                    }
                }

                let choices = resolve_choices(&node.key, question, &context)?;

                // Nothing left to offer. Treated exactly as a false `when`:
                // the question is not asked and gets no value, so `x is
                // defined` still separates "not applicable" from "declined".
                // A filter that narrows to nothing is a legitimate state —
                // `[computed]` is how choices are filtered — and erroring here
                // would make a template unable to express "this does not apply
                // to your answers".
                if choices.as_ref().is_some_and(Vec::is_empty) {
                    continue;
                }

                let default = resolve_default(&node.key, question, &context, partials)?;

                let answer = match supplied.get(&node.key) {
                    Some(value) => coerce(&node.key, question, value)?,
                    None => prompter.ask(
                        &node.key,
                        question,
                        default.as_ref(),
                        seeds.get(&node.key),
                        choices.as_deref(),
                    )?,
                };

                validate_choice(&node.key, question, &answer, choices.as_deref())?;
                // Deliberately here rather than in the prompter: this covers
                // the supplied branch too, which is where an answer replayed
                // from `.config/git.tpl.toml` by `update` arrives.
                validate_pattern(&node.key, question, &answer)?;
                context.set_answer(&node.key, answer);
            }
        }
    }

    Ok(context)
}

/// Resolve a question's choices, from `choices` or `choices_from`.
///
/// Both spellings end up as [`Choice`] through one reader, so a list moved from
/// the manifest into a data file behaves identically.
fn resolve_choices(
    name: &str,
    question: &Question,
    context: &Context,
) -> Result<Option<Vec<Choice>>, EvalError> {
    if let Some(choices) = &question.choices {
        return Ok(Some(choices.clone()));
    }

    let Some(reference) = &question.choices_from else {
        return Ok(None);
    };

    let value = context
        .get_path(reference)
        .ok_or_else(|| EvalError::BadChoices {
            question: name.to_string(),
            reference: reference.clone(),
            found: "nothing".into(),
        })?;

    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                Choice::from_value(item).map_err(|error| EvalError::BadChoices {
                    question: name.to_string(),
                    reference: reference.clone(),
                    found: error.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        // A table is the likeliest mistake — pointing at `data.licenses`
        // instead of `data.licenses.ids` — so name what was found rather than
        // reporting a generic type error.
        other => Err(EvalError::BadChoices {
            question: name.to_string(),
            reference: reference.clone(),
            found: other.type_name().to_string(),
        }),
    }
}

/// Resolve a question's default, evaluating it if it is an expression.
fn resolve_default(
    name: &str,
    question: &Question,
    context: &Context,
    partials: &Arc<Partials>,
) -> Result<Option<Value>, EvalError> {
    match question.default_expression() {
        Some(expression) => {
            let value = evaluate(
                expression,
                context,
                &format!("questions.{name}.default"),
                partials,
            )?;
            Ok(Some(value))
        }
        None => Ok(question.default.clone()),
    }
}

/// Check a supplied answer against the question's declared type.
fn coerce(name: &str, question: &Question, value: &Value) -> Result<Value, EvalError> {
    if question.kind.accepts(value) {
        return Ok(value.clone());
    }

    // A supplied answer may have arrived as a string from `--answer k=v`, in
    // which case parsing it as the declared type is correct. Anything else is a
    // genuine mismatch and must not be silently coerced — `--answer ci=maybe`
    // becoming the truthy string `"maybe"` would render the wrong template.
    if let Value::String(text) = value
        && let Ok(parsed) = Value::parse_as(text, question.kind.type_name())
        && question.kind.accepts(&parsed)
    {
        return Ok(parsed);
    }

    Err(EvalError::WrongType {
        question: name.to_string(),
        expected: question.kind.type_name().to_string(),
        found: value.type_name().to_string(),
    })
}

/// Check an answer is among the offered choices.
fn validate_choice(
    name: &str,
    question: &Question,
    answer: &Value,
    choices: Option<&[Choice]>,
) -> Result<(), EvalError> {
    let Some(choices) = choices else {
        return Ok(());
    };

    // Membership is on the choice's *value*. A label is presentation, and an
    // answer recorded as `MIT` must keep matching when the label changes.
    let offered = |value: &Value| match value {
        Value::String(s) => choices.iter().any(|c| &c.value == s),
        _ => false,
    };

    let reject = |value: &Value| EvalError::InvalidChoice {
        question: name.to_string(),
        value: value.to_string(),
        choices: Choice::describe(choices),
    };

    match question.kind {
        QuestionKind::Choice => {
            if !offered(answer) {
                return Err(reject(answer));
            }
        }
        QuestionKind::MultiChoice => {
            // A multi-choice answer that is not an array cannot be checked
            // element by element, and letting it through unvalidated is how a
            // bad `--answer` would reach the context. `coerce` normally catches
            // it first; this is the backstop.
            let Value::Array(items) = answer else {
                return Err(reject(answer));
            };
            for item in items {
                if !offered(item) {
                    return Err(reject(item));
                }
            }
        }
        _ => {}
    }

    Ok(())
}

/// Check an answer against the question's `pattern`.
///
/// Public because the interactive prompter re-asks on a mismatch rather than
/// aborting, and must reject exactly what [`resolve`] would reject. `resolve`
/// checks again regardless, so the prompt's loop is ergonomics and this is the
/// enforcement point.
pub fn validate_pattern(name: &str, question: &Question, answer: &Value) -> Result<(), EvalError> {
    let Some(pattern) = question.pattern.as_deref() else {
        return Ok(());
    };

    // Only text is matched. The manifest refuses `pattern` on any other kind,
    // so a non-string here is a value that failed to coerce, and `coerce` has
    // already reported it more precisely than a regex could.
    let Value::String(text) = answer else {
        return Ok(());
    };

    // Unreachable: `Manifest::validate` compiled this pattern at load time. A
    // panic here would turn a hypothetical into a crash, and skipping the check
    // is the same outcome as the pattern not being there.
    let Ok(regex) = regex_lite::Regex::new(pattern) else {
        return Ok(());
    };

    if regex.is_match(text) {
        return Ok(());
    }

    Err(EvalError::PatternMismatch {
        question: name.to_string(),
        value: text.clone(),
        reason: question
            .pattern_message()
            .unwrap_or_else(|| format!("must match `{pattern}`")),
    })
}

/// The templates a `{% import %}` or `{% include %}` may resolve to.
///
/// A partial is any `.jinja` blob in the template repository that lives outside
/// the render root, keyed by its repository-root-relative path. Living outside
/// the root is what makes it a partial rather than an output file: the tree walk
/// never sees it, so there is no skip rule to get wrong and no way for a macro
/// definition to leak into the rendered project.
///
/// Owned rather than borrowed, and deliberately so. [`environment`] returns an
/// `Environment<'static>` and MiniJinja's loader must be `Send + Sync + 'static`,
/// which the libgit2 backend's repository handle is not. The bytes are therefore
/// read out of the tree once, up front, by
/// [`Resolved::partials`](crate::ops::Resolved::partials).
///
/// Each name maps to *every* declaration of it across an `[extends]` chain,
/// nearest (the template actually resolved) first — for a template with no
/// parent, that is always exactly one. A `BTreeMap` rather than a `HashMap`
/// because invariant 2 forbids iteration order that varies between runs —
/// here it decides the order of the names listed when a lookup fails.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Partials(BTreeMap<String, Vec<String>>);

/// The loader namespace ADR-012 reserved for reaching an ancestor's own file.
/// See ADR-034.
pub const PARENT_PREFIX: &str = "parent:";

impl Partials {
    /// Collect one layer's partials from `name -> source` pairs.
    ///
    /// `list_tree` never yields two entries at the same path within one tree,
    /// so within one layer each name maps to exactly one declaration.
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, content) in entries {
            map.entry(name).or_default().push(content);
        }
        Self(map)
    }

    /// No partials at all — the common case, and every call site's fallback.
    pub fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Merge an `[extends]` chain's own partial sets into one.
    ///
    /// `layers` must already be ordered nearest first — the template actually
    /// resolved, then its parent, then its grandparent, and so on. A bare
    /// name resolves to the nearest layer that declares it (ADR-034): the
    /// same "the unit of override is the name" rule already applied to
    /// `[data]`, extended to files outside the render root instead of
    /// invented fresh for them.
    pub fn merge_chain(layers: impl IntoIterator<Item = Partials>) -> Self {
        let mut merged: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for layer in layers {
            for (name, mut declarations) in layer.0 {
                merged.entry(name).or_default().append(&mut declarations);
            }
        }
        Self(merged)
    }

    /// The names a template may import, in sorted order.
    ///
    /// Bare names only — `parent:x` is not a name of its own, it is a way of
    /// asking for a *shadowed* declaration of `x`, and listing it here would
    /// suggest an import a typo could not have meant.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// Drop this layer's own declaration of each name in `excluded`, if any.
    ///
    /// For an ancestor's own `[extends].remove` (ADR-034): applied to that
    /// ancestor's own single-layer `Partials`, before it is folded into the
    /// chain by [`Partials::merge_chain`], so a removed partial is absent from
    /// the merge entirely rather than merely unreachable by a bare name.
    pub fn without(mut self, excluded: &[String]) -> Self {
        for name in excluded {
            self.0.remove(name);
        }
        self
    }

    /// Whether there is nothing to load.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The source of one partial.
    ///
    /// A bare name resolves to the nearest declaration. `parent:name` means
    /// "the next declaration of that same name, one layer further out" — the
    /// value a bare reference would have resolved to had the nearer layer not
    /// overridden it. `None` when there is no such shadowed declaration,
    /// including — always — for a template with no `[extends]` at all.
    pub fn get(&self, name: &str) -> Option<&str> {
        match name.strip_prefix(PARENT_PREFIX) {
            Some(rest) => self.0.get(rest)?.get(1).map(String::as_str),
            None => self.0.get(name)?.first().map(String::as_str),
        }
    }
}

/// A shared empty partial set.
///
/// For the callers that legitimately have none — the static graph analysis, and
/// most tests. A `LazyLock` rather than an allocation per call because
/// [`environment`] is built for every rendered file and every path segment.
pub fn no_partials() -> &'static Arc<Partials> {
    static NONE: std::sync::LazyLock<Arc<Partials>> =
        std::sync::LazyLock::new(|| Arc::new(Partials::empty()));
    &NONE
}

/// The MiniJinja environment used everywhere in git-tpl.
///
/// Built through one constructor so that expression evaluation and file
/// rendering cannot diverge: a filter available in a `default` must behave
/// identically inside a `.jinja` file, and a macro importable from a `.jinja`
/// file must be importable from a `computed` expression.
pub fn environment(partials: &Arc<Partials>) -> minijinja::Environment<'static> {
    environment_with(partials, Undefined::Lenient)
}

/// How an undeclared name behaves in a rendered template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undefined {
    /// Renders to the empty string. MiniJinja's default, and what every
    /// rendered file has always done.
    Lenient,
    /// Fails the render, naming the file and the missing name.
    ///
    /// Opt-in per manifest via `strict = true`. The asymmetry it closes: the
    /// same typo in a `computed` expression is caught before the first prompt,
    /// with a suggestion, while in a file body it produced `name = ""` and an
    /// exit code of zero.
    Strict,
}

/// [`environment`], with the undefined behaviour chosen.
///
/// Still the one constructor: everything that decides how a template behaves
/// lives here, so expression evaluation and file rendering cannot drift apart.
pub fn environment_with(
    partials: &Arc<Partials>,
    undefined: Undefined,
) -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();

    if undefined == Undefined::Strict {
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    }

    // MiniJinja strips a template's final newline by default. For a file that
    // is wrong in a way that shows up everywhere: every rendered file would
    // lose its trailing newline, tripping POSIX conventions, `end-of-file-fixer`
    // hooks and `git diff`'s "\ No newline at end of file".
    env.set_keep_trailing_newline(true);

    // The set of registered filters is closed, and `slugify` is the only member.
    // A candidate qualifies only if it is pure, deterministic, and reaches
    // nothing outside its own argument. Anything that would reach outside the
    // context — reading a file, making a request, running a command — is not
    // available to templates, and will not be. There is no plugin point.
    // See docs/adr/003-minijinja-only.md and docs/concepts/determinism.md#security.
    env.add_filter("slugify", slugify_filter);

    // A loader, not an extension point. It resolves a name to bytes already
    // committed at the pinned template revision — it executes nothing and
    // reaches nothing outside the tree, so invariant 5 is untouched.
    // See docs/adr/012-template-loader.md.
    //
    // Lazy rather than `add_template_owned` in a loop: `environment()` is
    // rebuilt for every rendered file *and* every path segment, so eager
    // registration would cost O(files x partials) parses for partials that
    // nothing imports. The closure captures an `Arc` clone, which is the whole
    // per-call price.
    //
    // A miss returns `Ok(None)` rather than an error carrying a better
    // message, because MiniJinja discards a loader's `TemplateNotFound` and
    // substitutes its own — and because `Ok(None)` is what `{% include ...
    // ignore missing %}` is defined against. The names that *do* exist are
    // added to the diagnostic instead, by `describe_lookup` below.
    let loadable = Arc::clone(partials);
    env.set_loader(move |name| Ok(loadable.get(name).map(str::to_string)));

    env
}

/// MiniJinja's message, plus the partials that exist when one could not be found.
///
/// A "template not found" tells the author the name they wrote. What they do
/// not know is which names are correct — and the failure is nearly always a
/// typo, or a path written relative to the render root instead of the
/// repository root.
fn describe_lookup(error: &minijinja::Error, partials: &Partials) -> String {
    let mut message = describe(error);

    if !is_template_not_found(error) {
        return message;
    }

    if partials.is_empty() {
        message.push_str(
            "\nthis template repository defines no partials \
             (a partial is a `.jinja` file outside the render root)",
        );
    } else {
        message.push_str("\navailable partials: ");
        message.push_str(&partials.names().collect::<Vec<_>>().join(", "));
    }

    message
}

/// Whether a lookup failure is anywhere in the error's cause chain.
///
/// Not just the top: `{% include %}` inside an imported macro wraps the miss in
/// a `BadInclude`, and the hint is just as wanted there.
fn is_template_not_found(error: &minijinja::Error) -> bool {
    if error.kind() == minijinja::ErrorKind::TemplateNotFound {
        return true;
    }

    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if cause
            .downcast_ref::<minijinja::Error>()
            .is_some_and(|error| error.kind() == minijinja::ErrorKind::TemplateNotFound)
        {
            return true;
        }
        source = std::error::Error::source(cause);
    }

    false
}

/// `{{ project_name | slugify }}`.
///
/// Takes the value's string form, so an integer or a boolean slugs rather than
/// raising — a filter that fails inside a `when` condition is a worse failure
/// than a surprising string.
fn slugify_filter(value: minijinja::Value) -> String {
    slugify(&value.to_string())
}

/// Transliterate to ASCII, lowercase, and join the alphanumeric runs with `-`.
///
/// Transliterated rather than folded: dropping what does not fold would slug
/// `Москва` to the empty string, and a Cyrillic or CJK project name is not an
/// error. `-` is the tofu replacement so an untransliterable codepoint becomes
/// a separator instead of the literal `[?]` deunicode would otherwise emit.
///
/// Not to be confused with `refs::slugify`, which derives `refs/tpl/<id>` from
/// a URL. That one is deliberately ASCII-only and is *not* shared with this:
/// changing its output would rename the template ref of every existing project,
/// which invariant 3 exists to prevent.
fn slugify(s: &str) -> String {
    let ascii = deunicode::deunicode_with_tofu(s, "-");

    let mut out = String::with_capacity(ascii.len());
    let mut pending_sep = false;
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            // Only emit the separator once a keeper follows it, which trims
            // both ends and collapses runs without a second pass.
            if pending_sep && !out.is_empty() {
                out.push('-');
            }
            pending_sep = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending_sep = true;
        }
    }
    out
}

/// Evaluate an expression, preserving the type of a single-value result.
///
/// A bare `{{ expr }}` yields the value itself, so a computed boolean stays a
/// boolean. An expression with surrounding text is a string, as expected.
pub fn evaluate(
    expression: &str,
    context: &Context,
    location: &str,
    partials: &Arc<Partials>,
) -> Result<Value, EvalError> {
    if !is_expression(expression) {
        return Ok(Value::String(expression.to_string()));
    }

    let env = environment(partials);

    // A whole-expression template such as `{{ data.features }}` is evaluated
    // through `eval_expr` rather than rendered, because rendering would
    // stringify it — and `{% if needs_tokio %}` on the string "false" is true.
    if let Some(inner) = whole_expression(expression) {
        let value = env
            .compile_expression(inner)
            .and_then(|compiled| compiled.eval(context.to_minijinja()))
            .map_err(|error| EvalError::Expression {
                location: location.to_string(),
                expression: expression.to_string(),
                reason: describe_lookup(&error, partials),
            })?;
        return Value::from_minijinja(&value).map_err(|error| EvalError::Expression {
            location: location.to_string(),
            expression: expression.to_string(),
            reason: error.to_string(),
        });
    }

    render_string(expression, context, location, partials).map(Value::String)
}

/// Render a template to a string.
pub fn render_string(
    template: &str,
    context: &Context,
    location: &str,
    partials: &Arc<Partials>,
) -> Result<String, EvalError> {
    render_string_with(template, context, location, partials, Undefined::Lenient)
}

/// [`render_string`], with the undefined behaviour chosen.
///
/// Manifest expressions stay lenient because the graph has already rejected an
/// unknown name in one, with a suggestion, before any prompt. It is file bodies
/// that had no such check, and `strict = true` in the manifest is what turns it
/// on for them.
pub fn render_string_with(
    template: &str,
    context: &Context,
    location: &str,
    partials: &Arc<Partials>,
    undefined: Undefined,
) -> Result<String, EvalError> {
    if !is_expression(template) {
        return Ok(template.to_string());
    }

    let env = environment_with(partials, undefined);
    env.render_str(template, context.to_minijinja())
        .map_err(|error| EvalError::Expression {
            location: location.to_string(),
            expression: template.to_string(),
            reason: describe_lookup(&error, partials),
        })
}

/// The environment a `default_from` expression is evaluated in.
///
/// Two deliberate differences from [`environment`], both load-bearing:
///
/// - **Chainable**, not lenient. Lenient *raises* when you index into an
///   undefined value, so `{{ remote.name | default(dir.name) }}` on a project
///   that has never been pushed would fail instead of falling back. Falling
///   back is the entire point of the feature.
/// - **No partials.** A seed is not a rendering. Allowing `{% import %}` would
///   let machine values be laundered through arbitrary template code, widening
///   the narrow ADR-006 escape hatch into a general one — and there is nothing
///   here worth importing. Seeds are also built before the partials are read
///   out of the tree, so there would be none to offer.
///
/// Filters are inherited from [`environment_with`], so `slugify` — and anything
/// added to the closed set later — works here with no second registration site.
pub fn seed_environment() -> minijinja::Environment<'static> {
    let mut env = environment_with(no_partials(), Undefined::Lenient);
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Chainable);
    env
}

/// Render a `default_from` expression against the machine's seed context.
///
/// Always to a `String`. A seed is text a human is about to edit at a prompt,
/// and preserving a richer type would only reintroduce the coercion problem
/// that keeps `default_from` to `string` questions.
pub fn render_seed(
    expression: &str,
    seeds: &SeedContext,
    location: &str,
) -> Result<String, EvalError> {
    seed_environment()
        .render_str(expression, seeds.to_minijinja())
        .map_err(|error| EvalError::Expression {
            location: location.to_string(),
            expression: expression.to_string(),
            reason: describe(&error),
        })
}

/// The inner expression of a template that is nothing but `{{ ... }}`.
fn whole_expression(template: &str) -> Option<&str> {
    let trimmed = template.trim();
    let inner = trimmed.strip_prefix("{{")?.strip_suffix("}}")?;
    // Reject `{{ a }}{{ b }}`, which is two expressions and must be rendered
    // as a string rather than evaluated as one.
    if inner.contains("{{") || inner.contains("}}") || inner.contains("{%") {
        return None;
    }
    Some(inner.trim())
}

/// MiniJinja's message, plus its cause where there is one.
///
/// The cause carries the actual reason — "invalid operation" alone tells a user
/// nothing about which operation or why.
fn describe(error: &minijinja::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = std::error::Error::source(cause);
    }
    message
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::template::MANIFEST_NAME;

    fn context_with(pairs: &[(&str, Value)]) -> Context {
        let mut context = Context::new();
        for (key, value) in pairs {
            context.set_answer(*key, value.clone());
        }
        context
    }

    #[test]
    fn a_literal_is_returned_unchanged() {
        let context = Context::new();
        assert_eq!(
            evaluate("MIT", &context, "test", no_partials()).unwrap(),
            Value::String("MIT".into())
        );
    }

    #[test]
    fn an_expression_with_surrounding_text_is_a_string() {
        let context = context_with(&[("name", Value::String("demo".into()))]);
        assert_eq!(
            evaluate("{{ name }}-suffix", &context, "test", no_partials()).unwrap(),
            Value::String("demo-suffix".into())
        );
    }

    /// A computed boolean that came back as the string `"false"` would make
    /// `{% if needs_tokio %}` true, and the template would render the opposite
    /// of what it says.
    #[test]
    fn a_whole_expression_keeps_the_type_of_its_result() {
        let context = context_with(&[
            ("cli", Value::Bool(false)),
            ("count", Value::Integer(3)),
            (
                "features",
                Value::Array(vec![Value::String("serde".into())]),
            ),
        ]);

        assert_eq!(
            evaluate("{{ cli }}", &context, "test", no_partials()).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            evaluate("{{ count + 1 }}", &context, "test", no_partials()).unwrap(),
            Value::Integer(4)
        );
        assert_eq!(
            evaluate("{{ features }}", &context, "test", no_partials()).unwrap(),
            Value::Array(vec![Value::String("serde".into())])
        );
        assert_eq!(
            evaluate("{{ cli and count > 0 }}", &context, "test", no_partials()).unwrap(),
            Value::Bool(false)
        );
        // Issue #111: `+` on two sequences builds MiniJinja's lazy
        // concatenation object (kind `Iterable`), not a `Seq`. Without
        // handling that kind, `Value::from_minijinja` stringified it to
        // `"['serde', 'extra']"` instead of keeping it an array.
        assert_eq!(
            evaluate(
                "{{ features + ['extra'] }}",
                &context,
                "test",
                no_partials()
            )
            .unwrap(),
            Value::Array(vec![
                Value::String("serde".into()),
                Value::String("extra".into())
            ])
        );
    }

    #[test]
    fn two_expressions_in_one_string_render_as_a_string() {
        let context = context_with(&[("a", Value::Integer(1)), ("b", Value::Integer(2))]);
        assert_eq!(
            evaluate("{{ a }}{{ b }}", &context, "test", no_partials()).unwrap(),
            Value::String("12".into())
        );
    }

    #[test]
    fn a_failing_expression_reports_where_it_came_from() {
        let context = Context::new();
        // An unknown filter, not `1 / 0`: MiniJinja evaluates that to `inf`
        // rather than failing, which is a fine choice but makes it useless as a
        // test of the error path.
        let error = evaluate(
            "{{ 'x' | no_such_filter }}",
            &context,
            "computed.oops",
            no_partials(),
        )
        .unwrap_err();

        match error {
            EvalError::Expression {
                location, reason, ..
            } => {
                assert_eq!(location, "computed.oops");
                assert!(
                    reason.contains("no_such_filter"),
                    "the reason must name the filter: {reason}"
                );
            }
            other => panic!("expected an expression error, got {other:?}"),
        }
    }

    // --- resolution ---------------------------------------------------------

    /// Exercises the whole loop: conditional questions, dynamic defaults and
    /// computed values interleaved in dependency order.
    fn resolve_with(toml: &str, supplied: &[(&str, Value)]) -> Result<Context, EvalError> {
        resolve_seeded(toml, supplied, &[], &mut DefaultsOnly)
    }

    /// Resolve a manifest whose expressions may import shared partials.
    fn resolve_with_partials(toml: &str, partials: &[(&str, &str)]) -> Result<Context, EvalError> {
        let manifest = Manifest::parse(toml, MANIFEST_NAME).expect("manifest should parse");
        let graph = Graph::build(&manifest).expect("graph should build");

        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::libgit2::LibGit2::init(dir.path()).unwrap();
        let tree = {
            use crate::git::GitBackend;
            repo.build_tree(&[]).unwrap()
        };
        let mut loader = Loader::new(
            crate::data::TemplateTree {
                repo: &repo,
                tree,
                revision: tree,
                // No data sources in these cases, so the loader is never
                // reached — this name is never read.
                reference: "test".to_string(),
            },
            Some(dir.path().to_path_buf()),
        );

        let partials =
            Arc::new(Partials::new(partials.iter().map(|(name, source)| {
                ((*name).to_string(), (*source).to_string())
            })));
        let seeds = BTreeMap::new();

        resolve(
            Evaluation {
                manifest: &manifest,
                graph: &graph,
                supplied: BTreeMap::new(),
                seeds: &seeds,
                partials: &partials,
            },
            &mut loader,
            &mut DefaultsOnly,
        )
    }

    /// The same, with prompt seeds and a prompter of the caller's choosing.
    fn resolve_seeded(
        toml: &str,
        supplied: &[(&str, Value)],
        seeds: &[(&str, Value)],
        prompter: &mut dyn Prompter,
    ) -> Result<Context, EvalError> {
        let manifest = Manifest::parse(toml, MANIFEST_NAME).expect("manifest should parse");
        let graph = Graph::build(&manifest).expect("graph should build");

        // No data sources in these cases, so the loader is never reached.
        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::libgit2::LibGit2::init(dir.path()).unwrap();
        let tree = {
            use crate::git::GitBackend;
            repo.build_tree(&[]).unwrap()
        };
        let mut loader = Loader::new(
            crate::data::TemplateTree {
                repo: &repo,
                tree,
                revision: tree,
                // No data sources in these cases, so the loader is never
                // reached — this name is never read.
                reference: "test".to_string(),
            },
            Some(dir.path().to_path_buf()),
        );

        let seeds: BTreeMap<String, Value> = seeds
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect();

        resolve(
            Evaluation {
                manifest: &manifest,
                graph: &graph,
                supplied: supplied
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.clone()))
                    .collect(),
                seeds: &seeds,
                partials: no_partials(),
            },
            &mut loader,
            prompter,
        )
    }

    /// Records what it was offered, then answers with whatever it would have
    /// pre-filled. Stands in for a terminal, which the tests do not have.
    struct Recording {
        seen: Vec<(String, Option<Value>, Option<Value>)>,
    }

    impl Prompter for Recording {
        fn ask(
            &mut self,
            name: &str,
            _question: &Question,
            default: Option<&Value>,
            seed: Option<&Value>,
            _choices: Option<&[Choice]>,
        ) -> Result<Value, EvalError> {
            self.seen
                .push((name.to_string(), default.cloned(), seed.cloned()));
            seed.or(default)
                .cloned()
                .ok_or_else(|| EvalError::Unanswered {
                    question: name.to_string(),
                })
        }
    }

    const CONDITIONAL: &str = r#"
        name = "t"

        [questions.project_type]
        type = "choice"
        choices = ["library", "application"]
        default = "library"

        [questions.cli]
        type = "boolean"
        when = "{{ project_type == 'application' }}"
        default = true

        [computed]
        package_name = "{{ project_name | lower | replace(' ', '-') }}"

        [questions.project_name]
        type = "string"
        default = "My Project"
    "#;

    #[test]
    fn a_question_whose_condition_is_false_is_absent_rather_than_null() {
        let context = resolve_with(CONDITIONAL, &[]).unwrap();

        assert_eq!(
            context.get_path("project_type"),
            Some(&Value::String("library".into()))
        );
        assert_eq!(
            context.get_path("cli"),
            None,
            "a skipped question must be absent, so `cli is defined` can tell \
             'not applicable' from 'declined'"
        );
        assert!(!context.answers().contains_key("cli"));
    }

    #[test]
    fn a_question_whose_condition_is_true_is_asked() {
        let context = resolve_with(
            CONDITIONAL,
            &[("project_type", Value::String("application".into()))],
        )
        .unwrap();

        assert_eq!(context.get_path("cli"), Some(&Value::Bool(true)));
    }

    const CONDITIONAL_WITH_GATED_DEFAULT: &str = r#"
        name = "t"

        [questions.project_type]
        type = "choice"
        choices = ["library", "application"]
        default = "library"

        [questions.cli]
        type = "boolean"
        when = "{{ project_type == 'application' }}"
        default = true
        default_when_skipped = true

        [questions.project_name]
        type = "string"
        default = "My Project"

        [questions.package_name]
        type = "string"
        when = "{{ project_type == 'application' }}"
        default = "{{ project_name | lower | replace(' ', '-') }}"
        default_when_skipped = true
    "#;

    /// The opt-in the issue asks for (#117): the default is visible, but the
    /// question is still not asked and still not an answer.
    #[test]
    fn a_default_when_skipped_question_exposes_its_default_but_is_not_answered() {
        let context = resolve_with(CONDITIONAL_WITH_GATED_DEFAULT, &[]).unwrap();

        assert_eq!(context.get_path("cli"), Some(&Value::Bool(true)));
        assert!(
            !context.answers().contains_key("cli"),
            "a skipped question stays unanswered even once its default is exposed"
        );
        assert!(context.gated_defaults().contains_key("cli"));
    }

    /// An expression default for a skipped question is evaluated the same
    /// way an asked question's would be — against what resolved before it.
    #[test]
    fn a_default_when_skipped_expression_is_evaluated_against_the_resolved_context() {
        let context = resolve_with(
            CONDITIONAL_WITH_GATED_DEFAULT,
            &[("project_name", Value::String("My Great Project".into()))],
        )
        .unwrap();

        assert_eq!(
            context.get_path("package_name"),
            Some(&Value::String("my-great-project".into()))
        );
        assert!(!context.answers().contains_key("package_name"));
    }

    /// A question whose condition is true is asked normally — the flag only
    /// changes what happens when it is skipped.
    #[test]
    fn a_default_when_skipped_question_is_asked_normally_once_its_when_is_true() {
        let context = resolve_with(
            CONDITIONAL_WITH_GATED_DEFAULT,
            &[("project_type", Value::String("application".into()))],
        )
        .unwrap();

        assert_eq!(context.get_path("cli"), Some(&Value::Bool(true)));
        assert!(context.answers().contains_key("cli"));
        assert!(!context.gated_defaults().contains_key("cli"));
    }

    /// The digest, recorded in commit trailers and compared by `status`, must
    /// not change depending on whether a skipped question exposed its
    /// default — only answers are input.
    #[test]
    fn a_gated_default_does_not_change_the_answers_digest() {
        let with_flag = resolve_with(CONDITIONAL_WITH_GATED_DEFAULT, &[]).unwrap();
        let without_flag = resolve_with(CONDITIONAL, &[]).unwrap();

        assert_eq!(
            with_flag.answers_digest(),
            without_flag.answers_digest(),
            "the two manifests answer the same questions the same way; only \
             `cli`'s exposure to skipped file bodies differs"
        );
    }

    #[test]
    fn a_dynamic_default_is_evaluated_against_the_resolved_context() {
        let context = resolve_with(
            CONDITIONAL,
            &[("project_name", Value::String("My Great Project".into()))],
        )
        .unwrap();

        assert_eq!(
            context.get_path("package_name"),
            Some(&Value::String("my-great-project".into()))
        );
    }

    #[test]
    fn a_supplied_answer_skips_its_prompt_but_still_feeds_its_dependents() {
        let context = resolve_with(
            r#"
            name = "t"
            [questions.project_name]
            type = "string"
            [questions.package_name]
            type = "string"
            default = "{{ project_name | lower }}"
            "#,
            &[("project_name", Value::String("Demo".into()))],
        )
        .unwrap();

        assert_eq!(
            context.get_path("package_name"),
            Some(&Value::String("demo".into()))
        );
    }

    #[test]
    fn a_question_with_no_default_and_no_answer_is_an_error_under_defaults() {
        let error = resolve_with(
            r#"
            name = "t"
            [questions.project_name]
            type = "string"
            "#,
            &[],
        )
        .unwrap_err();

        std::assert_matches!(
            error,
            EvalError::Unanswered { ref question } if question == "project_name",
            "{error:?}"
        );
    }

    #[test]
    fn an_answer_outside_the_offered_choices_is_rejected() {
        let error = resolve_with(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices = ["MIT", "Apache-2.0"]
            "#,
            &[("license", Value::String("GPL-3.0".into()))],
        )
        .unwrap_err();

        std::assert_matches!(error, EvalError::InvalidChoice { .. }, "{error:?}");
    }

    /// The rejection has to name the values, not the labels: a value is what
    /// `--answer` and `.config/git.tpl.toml` take, so offering "MIT License"
    /// would be telling the user to type something that does not work.
    #[test]
    fn a_rejected_answer_lists_the_offered_values_not_the_labels() {
        let error = resolve_with(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices = [
              { value = "MIT", label = "MIT License" },
              { value = "Apache-2.0", label = "Apache License 2.0" },
            ]
            "#,
            &[("license", Value::String("GPL-3.0".into()))],
        )
        .unwrap_err();

        match error {
            EvalError::InvalidChoice { choices, .. } => {
                assert_eq!(choices, "MIT, Apache-2.0");
            }
            other => panic!("expected an invalid choice, got {other:?}"),
        }
    }

    /// A label is presentation. It must not affect which answers are accepted,
    /// or a template could not rename one without breaking every project.
    #[test]
    fn a_labelled_choice_is_answered_by_its_value() {
        let context = resolve_with(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices = [{ value = "MIT", label = "MIT License" }]
            "#,
            &[("license", Value::String("MIT".into()))],
        )
        .unwrap();

        assert_eq!(
            context.get_path("license"),
            Some(&Value::String("MIT".into()))
        );
    }

    /// Answering with the label rather than the value is a mistake worth
    /// catching, not a second accepted spelling.
    #[test]
    fn a_labelled_choice_is_not_answered_by_its_label() {
        let error = resolve_with(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices = [{ value = "MIT", label = "MIT License" }]
            "#,
            &[("license", Value::String("MIT License".into()))],
        )
        .unwrap_err();

        std::assert_matches!(error, EvalError::InvalidChoice { .. }, "{error:?}");
    }

    /// Every element is checked, not just the array as a whole.
    #[test]
    fn one_bad_element_rejects_a_multi_choice_answer() {
        let error = resolve_with(
            r#"
            name = "t"
            [questions.features]
            type = "multi_choice"
            choices = ["serde", "async"]
            "#,
            &[(
                "features",
                Value::Array(vec![
                    Value::String("serde".into()),
                    Value::String("nope".into()),
                ]),
            )],
        )
        .unwrap_err();

        match error {
            EvalError::InvalidChoice { value, .. } => assert_eq!(value, "nope"),
            other => panic!("expected an invalid choice, got {other:?}"),
        }
    }

    /// `type` and the source of the choices are independent axes. This is the
    /// combination that looked as though it needed a `multi_choice_from` key.
    #[test]
    fn a_multi_choice_may_draw_its_choices_from_a_computed_list() {
        let context = resolve_with(
            r#"
            name = "t"

            [computed]
            available = "{{ ['serde', 'async', 'cli'] }}"

            [questions.features]
            type = "multi_choice"
            choices_from = "available"
            "#,
            &[("features", Value::String("serde,cli".into()))],
        )
        .unwrap();

        assert_eq!(
            context.get_path("features"),
            Some(&Value::Array(vec![
                Value::String("serde".into()),
                Value::String("cli".into()),
            ]))
        );
    }

    /// Choices are filtered with `[computed]`, and a filter that narrows to
    /// nothing is a legitimate state — it means "this does not apply". The
    /// question is then absent, exactly as a false `when` leaves it, so
    /// `is defined` still separates the two cases.
    #[test]
    fn a_question_whose_choices_all_filtered_out_is_absent_rather_than_null() {
        let context = resolve_with(
            r#"
            name = "t"

            [questions.kind]
            type = "choice"
            choices = ["library", "application"]
            default = "library"

            [computed]
            servers = "{{ ['nginx', 'caddy'] if kind == 'application' else [] }}"

            [questions.server]
            type = "choice"
            choices_from = "servers"
            "#,
            &[],
        )
        .unwrap();

        assert_eq!(context.get_path("server"), None);
    }

    /// The same template with the other answer must ask the question, or the
    /// test above would pass just as well against a permanently broken filter.
    #[test]
    fn a_question_keeps_the_choices_a_filter_leaves() {
        let context = resolve_with(
            r#"
            name = "t"

            [questions.kind]
            type = "choice"
            choices = ["library", "application"]
            default = "application"

            [computed]
            servers = "{{ ['nginx', 'caddy'] if kind == 'application' else [] }}"

            [questions.server]
            type = "choice"
            choices_from = "servers"
            default = "caddy"
            "#,
            &[],
        )
        .unwrap();

        assert_eq!(
            context.get_path("server"),
            Some(&Value::String("caddy".into()))
        );
    }

    /// A data source may carry labels, in the same shape the manifest uses.
    #[test]
    fn choices_from_a_reference_may_be_labelled() {
        let context = resolve_with(
            r#"
            name = "t"

            [computed]
            licenses = "{{ [dict(value='MIT', label='MIT License')] }}"

            [questions.license]
            type = "choice"
            choices_from = "licenses"
            "#,
            &[("license", Value::String("MIT".into()))],
        )
        .unwrap();

        assert_eq!(
            context.get_path("license"),
            Some(&Value::String("MIT".into()))
        );
    }

    /// Pointing `choices_from` at something whose entries are not choices must
    /// name the problem rather than producing a prompt full of debug output.
    #[test]
    fn a_reference_to_unusable_choices_is_reported() {
        let error = resolve_with(
            r#"
            name = "t"

            [computed]
            broken = "{{ [dict(label='no value here')] }}"

            [questions.license]
            type = "choice"
            choices_from = "broken"
            "#,
            &[],
        )
        .unwrap_err();

        match error {
            EvalError::BadChoices { found, .. } => assert!(
                found.contains("`value`"),
                "the reason should name the missing key: {found}"
            ),
            other => panic!("expected bad choices, got {other:?}"),
        }
    }

    /// The invariant the whole label design rests on. The digest goes into the
    /// `Answers-Digest` trailer, and the rendered tree is built from answers —
    /// so if a label reached either, editing presentation text in a template
    /// would produce a commit in every project that uses it.
    #[test]
    fn renaming_a_choice_label_does_not_change_the_answers_digest() {
        let before = resolve_with(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices = [{ value = "MIT", label = "MIT License" }]
            "#,
            &[("license", Value::String("MIT".into()))],
        )
        .unwrap();

        let after = resolve_with(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices = [{ value = "MIT", label = "The MIT Licence", help = "Permissive" }]
            "#,
            &[("license", Value::String("MIT".into()))],
        )
        .unwrap();

        assert_eq!(before.answers_digest(), after.answers_digest());
    }

    #[test]
    fn an_answer_of_the_wrong_type_is_rejected() {
        let error = resolve_with(
            r#"
            name = "t"
            [questions.port]
            type = "integer"
            "#,
            &[("port", Value::String("nope".into()))],
        )
        .unwrap_err();

        std::assert_matches!(error, EvalError::WrongType { .. }, "{error:?}");
    }

    /// `--answer ci=true` arrives as text and must become a boolean.
    #[test]
    fn a_command_line_string_is_parsed_into_the_declared_type() {
        let context = resolve_with(
            r#"
            name = "t"
            [questions.ci]
            type = "boolean"
            [questions.port]
            type = "integer"
            "#,
            &[
                ("ci", Value::String("true".into())),
                ("port", Value::String("8080".into())),
            ],
        )
        .unwrap();

        assert_eq!(context.get_path("ci"), Some(&Value::Bool(true)));
        assert_eq!(context.get_path("port"), Some(&Value::Integer(8080)));
    }

    #[test]
    fn computed_values_may_depend_on_other_computed_values() {
        let context = resolve_with(
            r#"
            name = "t"
            [questions.project_name]
            type = "string"
            default = "My Project"
            [computed]
            module_name = "{{ package_name | replace('-', '_') }}"
            package_name = "{{ project_name | lower | replace(' ', '-') }}"
            "#,
            &[],
        )
        .unwrap();

        assert_eq!(
            context.get_path("module_name"),
            Some(&Value::String("my_project".into()))
        );
    }

    /// A stringified sequence and a real one render identically in the easy
    /// cases, so a regression that started collapsing computed values to text
    /// would show up only when a later expression indexed or iterated one.
    /// Assert the variant, and then use it downstream.
    #[test]
    fn a_computed_sequence_stays_a_sequence_through_evaluate() {
        let context = resolve_with(
            r#"
            name = "t"
            [computed]
            available = "{{ ['serde', 'clap', 'tokio'] }}"
            selected = "{{ available | select('ne', 'clap') | list }}"
            first = "{{ selected[0] }}"
            count = "{{ selected | length }}"
            "#,
            &[],
        )
        .unwrap();

        assert_eq!(
            context.get_path("selected"),
            Some(&Value::Array(vec![
                Value::String("serde".into()),
                Value::String("tokio".into()),
            ]))
        );
        assert_eq!(
            context.get_path("first"),
            Some(&Value::String("serde".into()))
        );
        assert_eq!(context.get_path("count"), Some(&Value::Integer(2)));
    }

    /// The same defence for a table. `dictsort` keeps the iteration assertion
    /// independent of map ordering, so it cannot become a source of
    /// non-determinism itself.
    #[test]
    fn a_computed_table_stays_a_table_through_evaluate() {
        let context = resolve_with(
            r#"
            name = "t"
            [questions.project_name]
            type = "string"
            default = "My Project"
            [computed]
            meta = "{{ dict(name=project_name, slug=project_name | lower | replace(' ', '-')) }}"
            slug = "{{ meta.slug }}"
            keys = "{{ meta | dictsort | map(attribute=0) | join(',') }}"
            "#,
            &[],
        )
        .unwrap();

        assert_eq!(
            context.get_path("meta"),
            Some(&Value::Table(BTreeMap::from([
                ("name".to_string(), Value::String("My Project".into())),
                ("slug".to_string(), Value::String("my-project".into())),
            ])))
        );
        assert_eq!(
            context.get_path("slug"),
            Some(&Value::String("my-project".into()))
        );
        assert_eq!(
            context.get_path("keys"),
            Some(&Value::String("name,slug".into()))
        );
    }

    /// A literal must reach the context as itself. Round-tripping it through the
    /// engine would turn `100` into `"100"`, which is exactly the bug that made
    /// `"{{ 100 }}"` necessary in the first place.
    #[test]
    fn a_literal_computed_value_keeps_its_toml_type() {
        let context = resolve_with(
            r#"
            name = "t"
            [computed]
            line_length = 100
            strict = true
            ratio = 1.5
            editors = ["vim", "helix"]
            title = "a plain string"
            wrapped = "{{ line_length - 20 }}"
            "#,
            &[],
        )
        .unwrap();

        assert_eq!(context.get_path("line_length"), Some(&Value::Integer(100)));
        assert_eq!(context.get_path("strict"), Some(&Value::Bool(true)));
        assert_eq!(context.get_path("ratio"), Some(&Value::Float(1.5)));
        assert_eq!(
            context.get_path("editors"),
            Some(&Value::Array(vec![
                Value::String("vim".into()),
                Value::String("helix".into()),
            ]))
        );
        assert_eq!(
            context.get_path("title"),
            Some(&Value::String("a plain string".into()))
        );

        // Arithmetic, not concatenation: proof the literal arrived as a number.
        assert_eq!(context.get_path("wrapped"), Some(&Value::Integer(80)));
    }

    /// Only answers are recorded in `.config/git.tpl.toml`; computed values are
    /// a function of them and are recomputed every render.
    #[test]
    fn only_answers_are_recorded() {
        let context = resolve_with(CONDITIONAL, &[]).unwrap();

        assert!(context.answers().contains_key("project_name"));
        assert!(
            !context.answers().contains_key("package_name"),
            "a computed value must not be recorded as an answer"
        );
    }

    /// The reason `slugify` exists: `lower | replace(' ', '-')` leaves the
    /// accents in place, and folding to ASCII would erase a Cyrillic or CJK
    /// name entirely.
    #[rstest]
    #[case("Café Déjà-Vu", "cafe-deja-vu")]
    #[case("Größe", "grosse")]
    #[case("Москва", "moskva")]
    #[case("北京", "bei-jing")]
    #[case("Ångström", "angstrom")]
    fn slugify_transliterates_non_ascii_letters(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(slugify(input), expected);
    }

    #[rstest]
    #[case("Hello, World!", "hello-world")]
    #[case("a  --  b", "a-b")]
    #[case("  spaced  ", "spaced")]
    #[case("My Project 2", "my-project-2")]
    #[case("under_score", "under-score")]
    fn slugify_collapses_runs_of_punctuation_to_one_separator(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(slugify(input), expected);
    }

    /// An empty result rather than an error. A filter that fails inside a `when`
    /// condition aborts the whole render; an empty string is visible at the
    /// prompt and fixable by the template author.
    #[rstest]
    #[case("")]
    #[case("!!!")]
    #[case("   ")]
    fn slugify_of_nothing_sluggable_is_empty(#[case] input: &str) {
        assert_eq!(slugify(input), "");
    }

    /// The whole point of the single `environment()` constructor: a filter
    /// available in a `default` behaves identically inside a `.jinja` file.
    #[test]
    fn slugify_is_available_in_an_expression_and_in_a_rendered_file() {
        let context = context_with(&[("project_name", Value::String("Café Déjà-Vu".into()))]);

        assert_eq!(
            evaluate(
                "{{ project_name | slugify }}",
                &context,
                "test",
                no_partials()
            )
            .unwrap(),
            Value::String("cafe-deja-vu".into())
        );
        assert_eq!(
            render_string(
                "src/{{ project_name | slugify }}/mod.rs",
                &context,
                "test",
                no_partials()
            )
            .unwrap(),
            "src/cafe-deja-vu/mod.rs"
        );
    }

    /// Invariant 2. `slugify` reads a table and nothing else, so two runs of the
    /// same input cannot differ — the property the whole ref model rests on.
    #[test]
    fn slugify_is_deterministic() {
        let context = context_with(&[("project_name", Value::String("Ünïcödé Prôjèct".into()))]);
        let once = evaluate(
            "{{ project_name | slugify }}",
            &context,
            "test",
            no_partials(),
        )
        .unwrap();
        let twice = evaluate(
            "{{ project_name | slugify }}",
            &context,
            "test",
            no_partials(),
        )
        .unwrap();

        assert_eq!(once, twice);
        assert_eq!(once, Value::String("unicode-project".into()));
    }

    /// The seed environment's reason to exist. Under MiniJinja's lenient
    /// behaviour, indexing into an undefined value *raises*, so this would
    /// abort instead of falling back — and a project that has never been pushed
    /// is exactly when a template most wants the directory name.
    #[test]
    fn an_absent_seed_namespace_falls_through_to_default() {
        let seeds = SeedContext::from_roots(BTreeMap::from([
            ("remote".to_string(), Value::Table(BTreeMap::new())),
            (
                "dir".to_string(),
                Value::Table(BTreeMap::from([(
                    "name".to_string(),
                    Value::String("My Project".into()),
                )])),
            ),
        ]));

        let rendered = render_seed(
            "{{ remote.name | default(dir.name) | slugify }}",
            &seeds,
            "questions.slug.default_from",
        )
        .unwrap();

        assert_eq!(rendered, "my-project");
    }

    /// And when the remote is there, it wins — otherwise the fallback would be
    /// the only branch anyone ever exercised.
    #[test]
    fn a_present_seed_value_is_preferred_to_its_fallback() {
        let seeds = SeedContext::from_roots(BTreeMap::from([
            (
                "remote".to_string(),
                Value::Table(BTreeMap::from([(
                    "name".to_string(),
                    Value::String("git-tpl".into()),
                )])),
            ),
            (
                "dir".to_string(),
                Value::Table(BTreeMap::from([(
                    "name".to_string(),
                    Value::String("checkout".into()),
                )])),
            ),
        ]));

        let rendered = render_seed(
            "{{ remote.name | default(dir.name) | slugify }}",
            &seeds,
            "questions.slug.default_from",
        )
        .unwrap();

        assert_eq!(rendered, "git-tpl");
    }

    /// A non-string argument slugs rather than raising, so a `slugify` in a
    /// condition cannot abort a render over a type.
    #[test]
    fn slugify_accepts_a_non_string_value() {
        let context = context_with(&[("version", Value::Integer(2))]);

        assert_eq!(
            evaluate("{{ version | slugify }}", &context, "test", no_partials()).unwrap(),
            Value::String("2".into())
        );
    }

    // --- shared partials ----------------------------------------------------

    /// The same principle as the `slugify` test above, for the loader: a macro
    /// importable from a `.jinja` file must be importable from a `computed`,
    /// because both go through the one `environment()` constructor.
    #[test]
    fn a_macro_is_importable_from_a_computed_expression() {
        let context = resolve_with_partials(
            r#"
                name = "t"

                [questions.project_name]
                type = "string"
                default = "Demo"

                [computed]
                package_name = "{% import 'macros.jinja' as m %}{{ m.pkg(project_name) }}"
            "#,
            &[(
                "macros.jinja",
                "{% macro pkg(n) %}{{ n | slugify }}-rs{% endmacro %}",
            )],
        )
        .unwrap();

        assert_eq!(
            context.get_path("package_name"),
            Some(&Value::String("demo-rs".into()))
        );
    }

    /// A miss inside a manifest expression gets the same hint a file does.
    #[test]
    fn a_missing_partial_in_a_computed_expression_lists_the_available_names() {
        let error = resolve_with_partials(
            r#"
                name = "t"

                [computed]
                oops = "{% import 'marcos.jinja' as m %}{{ m.pkg() }}"
            "#,
            &[("macros.jinja", "{% macro pkg() %}x{% endmacro %}")],
        )
        .unwrap_err();

        let EvalError::Expression { reason, .. } = &error else {
            panic!("expected an expression failure, got {error:?}");
        };
        assert!(
            reason.contains("available partials: macros.jinja"),
            "{reason}"
        );
    }

    /// Without a loader an unknown name would be a bare MiniJinja message. The
    /// hint has to say that the concept exists at all.
    #[test]
    fn importing_when_a_template_has_no_partials_says_there_are_none() {
        let error = resolve_with_partials(
            r#"
                name = "t"

                [computed]
                oops = "{% import 'macros.jinja' as m %}{{ m.pkg() }}"
            "#,
            &[],
        )
        .unwrap_err();

        let EvalError::Expression { reason, .. } = &error else {
            panic!("expected an expression failure, got {error:?}");
        };
        assert!(reason.contains("defines no partials"), "{reason}");
    }

    /// `ignore missing` is MiniJinja's contract for an optional include, and
    /// the loader returns `Ok(None)` on a miss specifically so it still holds.
    #[test]
    fn an_optional_include_of_a_missing_partial_is_not_an_error() {
        let context = context_with(&[]);

        assert_eq!(
            render_string(
                "{% include 'absent.jinja' ignore missing %}ok",
                &context,
                "test",
                no_partials(),
            )
            .unwrap(),
            "ok"
        );
    }

    // --- prompt seeds -------------------------------------------------------

    const SEEDED: &str = r#"
        name = "t"

        [questions.author]
        type = "string"
        default = "anonymous"
        default_from = "git:user.name"
    "#;

    /// A seed is what the prompt is pre-filled with, ahead of the template's
    /// own default — the point of asking the machine at all.
    #[test]
    fn a_seed_becomes_the_prompt_default_ahead_of_the_declared_default() {
        let mut prompter = Recording { seen: Vec::new() };
        let context = resolve_seeded(
            SEEDED,
            &[],
            &[("author", Value::String("Some One".into()))],
            &mut prompter,
        )
        .unwrap();

        assert_eq!(
            prompter.seen,
            vec![(
                "author".to_string(),
                Some(Value::String("anonymous".into())),
                Some(Value::String("Some One".into())),
            )]
        );
        assert_eq!(
            context.answers().get("author"),
            Some(&Value::String("Some One".into()))
        );
    }

    /// A seed is prompt-only, stated as a test. Nobody is asked, so the
    /// machine's value is not an answer — the template's own default is.
    #[test]
    fn a_seed_is_ignored_when_questions_are_not_asked() {
        let context = resolve_seeded(
            SEEDED,
            &[],
            &[("author", Value::String("Some One".into()))],
            &mut DefaultsOnly,
        )
        .unwrap();

        assert_eq!(
            context.answers().get("author"),
            Some(&Value::String("anonymous".into()))
        );
    }

    /// A supplied answer outranks a seed, as it outranks every default.
    #[test]
    fn a_supplied_answer_outranks_a_seed() {
        let mut prompter = Recording { seen: Vec::new() };
        let context = resolve_seeded(
            SEEDED,
            &[("author", Value::String("From The File".into()))],
            &[("author", Value::String("Some One".into()))],
            &mut prompter,
        )
        .unwrap();

        assert!(prompter.seen.is_empty(), "the question must not be asked");
        assert_eq!(
            context.answers().get("author"),
            Some(&Value::String("From The File".into()))
        );
    }
}
