//! Expression evaluation and incremental question resolution.
//!
//! Walks the [`Graph`](crate::graph::Graph) in dependency order, loading data,
//! asking questions and computing values as each becomes resolvable. Conditions
//! are evaluated against everything resolved so far, so a question that does
//! not apply is never shown — rather than asking everything and filtering
//! afterwards.

use std::collections::BTreeMap;

use miette::Diagnostic;
use thiserror::Error;

use crate::context::Context;
use crate::data::{DataError, Loader};
use crate::graph::{Graph, NodeKind};
use crate::template::{Choice, Manifest, Question, QuestionKind, Value, is_expression};

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
    /// Ask a question, given its resolved default and choices.
    fn ask(
        &mut self,
        name: &str,
        question: &Question,
        default: Option<&Value>,
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
                // everything it references is already resolved.
                let resolved =
                    render_string(&decl.source, &context, &format!("data.{}", node.key))?;
                let value = loader.load(&node.key, decl, &resolved)?;
                context.set_data(&node.key, value);
            }

            NodeKind::Computed => {
                let Some(expression) = manifest.computed.get(&node.key) else {
                    continue;
                };
                let value = evaluate(expression, &context, &format!("computed.{}", node.key))?;
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
                    let condition =
                        evaluate(when, &context, &format!("questions.{}.when", node.key))?;
                    if !condition.is_truthy() {
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

                let default = resolve_default(&node.key, question, &context)?;

                let answer = match supplied.get(&node.key) {
                    Some(value) => coerce(&node.key, question, value)?,
                    None => {
                        prompter.ask(&node.key, question, default.as_ref(), choices.as_deref())?
                    }
                };

                validate_choice(&node.key, question, &answer, choices.as_deref())?;
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
) -> Result<Option<Value>, EvalError> {
    match question.default_expression() {
        Some(expression) => {
            let value = evaluate(expression, context, &format!("questions.{name}.default"))?;
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

/// The MiniJinja environment used everywhere in git-tpl.
///
/// Built through one constructor so that expression evaluation and file
/// rendering cannot diverge: a filter available in a `default` must behave
/// identically inside a `.jinja` file.
pub fn environment() -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();

    // MiniJinja strips a template's final newline by default. For a file that
    // is wrong in a way that shows up everywhere: every rendered file would
    // lose its trailing newline, tripping POSIX conventions, `end-of-file-fixer`
    // hooks and `git diff`'s "\ No newline at end of file".
    env.set_keep_trailing_newline(true);

    // No functions, filters or globals are registered beyond MiniJinja's own.
    // Anything that would reach outside the context — reading a file, making a
    // request, running a command — is not available to templates, and will not
    // be. See docs/concepts/determinism.md#security.

    env
}

/// Evaluate an expression, preserving the type of a single-value result.
///
/// A bare `{{ expr }}` yields the value itself, so a computed boolean stays a
/// boolean. An expression with surrounding text is a string, as expected.
pub fn evaluate(expression: &str, context: &Context, location: &str) -> Result<Value, EvalError> {
    if !is_expression(expression) {
        return Ok(Value::String(expression.to_string()));
    }

    let env = environment();

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
                reason: describe(&error),
            })?;
        return Value::from_minijinja(&value).map_err(|error| EvalError::Expression {
            location: location.to_string(),
            expression: expression.to_string(),
            reason: error.to_string(),
        });
    }

    render_string(expression, context, location).map(Value::String)
}

/// Render a template to a string.
pub fn render_string(
    template: &str,
    context: &Context,
    location: &str,
) -> Result<String, EvalError> {
    if !is_expression(template) {
        return Ok(template.to_string());
    }

    let env = environment();
    env.render_str(template, context.to_minijinja())
        .map_err(|error| EvalError::Expression {
            location: location.to_string(),
            expression: template.to_string(),
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
            evaluate("MIT", &context, "test").unwrap(),
            Value::String("MIT".into())
        );
    }

    #[test]
    fn an_expression_with_surrounding_text_is_a_string() {
        let context = context_with(&[("name", Value::String("demo".into()))]);
        assert_eq!(
            evaluate("{{ name }}-suffix", &context, "test").unwrap(),
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
            evaluate("{{ cli }}", &context, "test").unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            evaluate("{{ count + 1 }}", &context, "test").unwrap(),
            Value::Integer(4)
        );
        assert_eq!(
            evaluate("{{ features }}", &context, "test").unwrap(),
            Value::Array(vec![Value::String("serde".into())])
        );
        assert_eq!(
            evaluate("{{ cli and count > 0 }}", &context, "test").unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn two_expressions_in_one_string_render_as_a_string() {
        let context = context_with(&[("a", Value::Integer(1)), ("b", Value::Integer(2))]);
        assert_eq!(
            evaluate("{{ a }}{{ b }}", &context, "test").unwrap(),
            Value::String("12".into())
        );
    }

    #[test]
    fn a_failing_expression_reports_where_it_came_from() {
        let context = Context::new();
        // An unknown filter, not `1 / 0`: MiniJinja evaluates that to `inf`
        // rather than failing, which is a fine choice but makes it useless as a
        // test of the error path.
        let error = evaluate("{{ 'x' | no_such_filter }}", &context, "computed.oops").unwrap_err();

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
            },
            dir.path(),
        );

        resolve(
            Evaluation {
                manifest: &manifest,
                graph: &graph,
                supplied: supplied
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.clone()))
                    .collect(),
            },
            &mut loader,
            &mut DefaultsOnly,
        )
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

        assert!(
            matches!(error, EvalError::Unanswered { ref question } if question == "project_name"),
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

        assert!(
            matches!(error, EvalError::InvalidChoice { .. }),
            "{error:?}"
        );
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

        assert!(
            matches!(error, EvalError::InvalidChoice { .. }),
            "{error:?}"
        );
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

        assert!(matches!(error, EvalError::WrongType { .. }), "{error:?}");
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
}
