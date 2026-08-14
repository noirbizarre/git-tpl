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
}

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
        };
        assert_eq!(question.default_expression(), None);
    }
}
