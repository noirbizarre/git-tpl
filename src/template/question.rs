//! Question definitions.

use serde::{Deserialize, Serialize};

use super::{Choice, Value};

/// What a question asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    /// Free text.
    String,
    /// Yes or no.
    Boolean,
    /// A whole number.
    Integer,
    /// One of a fixed set.
    Choice,
    /// Any number of a fixed set. The answer is an array.
    MultiChoice,
}

impl QuestionKind {
    /// The type name used in error messages and by [`Value::parse_as`].
    pub fn type_name(self) -> &'static str {
        match self {
            QuestionKind::String => "a string",
            QuestionKind::Boolean => "a boolean",
            QuestionKind::Integer => "an integer",
            QuestionKind::Choice => "a string",
            QuestionKind::MultiChoice => "an array",
        }
    }

    /// The name the manifest and `--json` use — `choice`, not `a string`.
    ///
    /// Distinct from [`QuestionKind::type_name`], which is the *value* type an
    /// answer must parse as and is dispatched on by [`Value::parse_as`].
    /// Listing a question by its value type names a type its author cannot
    /// write in a manifest, and hides the one they did.
    //
    // These are the serde `rename_all = "snake_case"` variant names; the match
    // lives next to the enum so the two spellings cannot drift apart.
    pub fn declared_name(self) -> &'static str {
        match self {
            QuestionKind::String => "string",
            QuestionKind::Boolean => "boolean",
            QuestionKind::Integer => "integer",
            QuestionKind::Choice => "choice",
            QuestionKind::MultiChoice => "multi_choice",
        }
    }

    /// Whether this kind draws its answer from a list of choices.
    pub fn is_choice(self) -> bool {
        matches!(self, QuestionKind::Choice | QuestionKind::MultiChoice)
    }

    /// Whether a value is acceptable as an answer of this kind.
    pub fn accepts(self, value: &Value) -> bool {
        match self {
            QuestionKind::String | QuestionKind::Choice => matches!(value, Value::String(_)),
            QuestionKind::Boolean => matches!(value, Value::Bool(_)),
            QuestionKind::Integer => matches!(value, Value::Integer(_)),
            QuestionKind::MultiChoice => matches!(value, Value::Array(_)),
        }
    }
}

/// A single question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    /// What kind of answer it wants.
    #[serde(rename = "type")]
    pub kind: QuestionKind,

    /// Shown to the user. Defaults to the question's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    /// A line of explanation under the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,

    /// The pre-filled value. May be an expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,

    /// Ask only if this expression is truthy.
    ///
    /// A question whose condition is false is not asked and has **no value** —
    /// it is absent from the context, not null. That distinction is what lets a
    /// template tell "not applicable" from "declined": `cli is defined` answers
    /// the first, `cli` the second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,

    /// The choices, for `choice` and `multi_choice`.
    ///
    /// A choice is a bare string, or a table carrying a `label` and `help`
    /// beside its `value`. Both `choice` and `multi_choice` take either these
    /// or `choices_from`; the two axes are independent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<Choice>>,

    /// A dotted path into the context yielding the choices.
    ///
    /// A structured reference rather than a string the template author has to
    /// serialise data into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices_from: Option<String>,

    /// Where the *prompt default* comes from. `git:<key>` only.
    ///
    /// It seeds the prompt and never the context. A value read from the
    /// machine's Git configuration reaches the tree only by way of an answer a
    /// human accepted, which is then recorded like any other answer — so the
    /// project stays reproducible for someone whose `user.name` differs
    /// (ADR-006, `docs/concepts/determinism.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_from: Option<String>,

    /// A regular expression every answer must match. `string` questions only.
    ///
    /// A pattern rather than an expression, deliberately: an arbitrary
    /// validator is code running on behalf of a template, and invariant 5 says
    /// no. Compiled when the manifest is read, so a broken pattern fails on the
    /// author's first render rather than mid-questionnaire on a user's machine.
    /// Checked wherever an answer arrives — prompted, `--answer`,
    /// `--answers-from`, or replayed from `.config/git.tpl.toml` by `update`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,

    /// What to say when `pattern` rejects an answer.
    ///
    /// Optional: without it the diagnostic quotes the pattern, which is honest
    /// but rarely kind. Meaningless without `pattern`, and refused there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The only source `default_from` accepts.
///
/// A single prefix rather than an open grammar: every other source anyone
/// would reach for — the environment, the clock — is the runtime context
/// ADR-006 refuses.
pub const GIT_PREFIX: &str = "git:";

impl Question {
    /// The prompt to show, falling back to the question's name.
    pub fn prompt_for<'a>(&'a self, name: &'a str) -> &'a str {
        self.prompt.as_deref().unwrap_or(name)
    }

    /// Whether the default is an expression rather than a literal.
    ///
    /// Only strings can be expressions; `default = true` is a literal boolean
    /// and must not be run through the template engine.
    pub fn default_expression(&self) -> Option<&str> {
        match &self.default {
            Some(Value::String(s)) if is_expression(s) => Some(s),
            _ => None,
        }
    }

    /// The Git configuration key seeding this question's prompt, if any.
    ///
    /// Never an expression and never evaluated, so it contributes no edge to
    /// the dependency graph — it references the machine, not the context.
    pub fn git_config_key(&self) -> Option<&str> {
        self.default_from
            .as_deref()
            .and_then(|source| source.strip_prefix(GIT_PREFIX))
    }

    /// What to say when `pattern` rejects an answer.
    ///
    /// Built here rather than at each call site so the prompt's retry message
    /// and the diagnostic a supplied answer produces are the same sentence.
    pub fn pattern_message(&self) -> Option<String> {
        let pattern = self.pattern.as_deref()?;
        Some(
            self.message
                .clone()
                .unwrap_or_else(|| format!("must match `{pattern}`")),
        )
    }
}

/// Whether a string contains MiniJinja syntax.
///
/// Used to decide whether a value needs evaluating. A plain `"MIT"` must be
/// left alone — running every string through the engine would make a literal
/// containing `{{` explode, and would cost a parse per literal.
pub fn is_expression(s: &str) -> bool {
    s.contains("{{") || s.contains("{%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(QuestionKind::String, Value::String("x".into()), true)]
    #[case(QuestionKind::String, Value::Bool(true), false)]
    #[case(QuestionKind::Boolean, Value::Bool(true), true)]
    #[case(QuestionKind::Boolean, Value::String("true".into()), false)]
    #[case(QuestionKind::Integer, Value::Integer(1), true)]
    #[case(QuestionKind::Integer, Value::Float(1.0), false)]
    #[case(QuestionKind::MultiChoice, Value::Array(vec![]), true)]
    #[case(QuestionKind::MultiChoice, Value::String("a".into()), false)]
    fn a_kind_accepts_only_its_own_type(
        #[case] kind: QuestionKind,
        #[case] value: Value,
        #[case] expected: bool,
    ) {
        assert_eq!(kind.accepts(&value), expected);
    }

    /// The listing and the JSON schema must name a kind the same way, or the
    /// answer to "what kind of question is this" depends on how you asked.
    #[rstest]
    #[case(QuestionKind::String)]
    #[case(QuestionKind::Boolean)]
    #[case(QuestionKind::Integer)]
    #[case(QuestionKind::Choice)]
    #[case(QuestionKind::MultiChoice)]
    fn the_declared_name_is_the_serialised_name(#[case] kind: QuestionKind) {
        let serialised = serde_json::to_value(kind).expect("serialise");
        assert_eq!(serialised, serde_json::Value::from(kind.declared_name()));
    }

    #[rstest]
    #[case("{{ project_name }}", true)]
    #[case("{% if x %}a{% endif %}", true)]
    #[case("MIT", false)]
    #[case("a { brace }", false)]
    fn expression_detection_ignores_plain_strings(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(is_expression(input), expected);
    }

    /// `default = true` is a literal. Sending it through the template engine
    /// would turn it into the string `"true"` and break `{% if ci %}`.
    #[test]
    fn a_non_string_default_is_never_treated_as_an_expression() {
        let question = Question {
            kind: QuestionKind::Boolean,
            prompt: None,
            help: None,
            default: Some(Value::Bool(true)),
            when: None,
            choices: None,
            choices_from: None,
            default_from: None,
            pattern: None,
            message: None,
        };
        assert_eq!(question.default_expression(), None);
    }

    #[rstest]
    #[case(Some("git:user.name"), Some("user.name"))]
    #[case(Some("user.name"), None)]
    #[case(None, None)]
    fn default_from_yields_its_git_config_key(
        #[case] default_from: Option<&str>,
        #[case] expected: Option<&str>,
    ) {
        let question = Question {
            kind: QuestionKind::String,
            prompt: None,
            help: None,
            default: None,
            when: None,
            choices: None,
            choices_from: None,
            default_from: default_from.map(str::to_string),
            pattern: None,
            message: None,
        };
        assert_eq!(question.git_config_key(), expected);
    }

    #[rstest]
    #[case(None, None, None)]
    #[case(Some("^[a-z]+$"), None, Some("must match `^[a-z]+$`"))]
    #[case(Some("^[a-z]+$"), Some("lowercase only"), Some("lowercase only"))]
    fn a_pattern_without_a_message_explains_itself(
        #[case] pattern: Option<&str>,
        #[case] message: Option<&str>,
        #[case] expected: Option<&str>,
    ) {
        let question = Question {
            kind: QuestionKind::String,
            prompt: None,
            help: None,
            default: None,
            when: None,
            choices: None,
            choices_from: None,
            default_from: None,
            pattern: pattern.map(str::to_string),
            message: message.map(str::to_string),
        };
        assert_eq!(question.pattern_message().as_deref(), expected);
    }
}
