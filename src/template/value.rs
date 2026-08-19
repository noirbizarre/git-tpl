//! The value type shared by answers, computed values and loaded data.
//!
//! One type across the whole context, so that a value read from a TOML data
//! file, a value typed at a prompt and a value computed by an expression are
//! indistinguishable to a template — and all of them keep their type rather
//! than being flattened to strings.

use std::collections::BTreeMap;
use std::fmt;

use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A structured value.
///
/// `BTreeMap` for tables so that iteration order is deterministic. A template
/// that iterates a table must render the same bytes every time, or the rendered
/// tree changes for no reason and `update` commits noise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// A boolean.
    Bool(bool),
    /// A 64-bit signed integer.
    Integer(i64),
    /// A double-precision float.
    Float(f64),
    /// A UTF-8 string.
    String(String),
    /// An ordered list.
    Array(Vec<Value>),
    /// A key-value table, iterated in key order.
    Table(BTreeMap<String, Value>),
}

/// Errors from converting or coercing a [`Value`].
#[derive(Debug, Error, Diagnostic)]
pub enum ValueError {
    /// A value was not of the expected type and could not be coerced.
    #[error("expected {expected}, got {actual}")]
    #[diagnostic(code(tpl::value::type_mismatch))]
    TypeMismatch {
        /// What was wanted.
        expected: &'static str,
        /// What was there.
        actual: &'static str,
    },

    /// A string could not be parsed as the requested type.
    #[error("`{input}` is not a valid {expected}")]
    #[diagnostic(code(tpl::value::parse))]
    Parse {
        /// The text that failed to parse.
        input: String,
        /// The type it was being parsed as.
        expected: &'static str,
    },
}

impl Value {
    /// The type name, for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "a boolean",
            Value::Integer(_) => "an integer",
            Value::Float(_) => "a float",
            Value::String(_) => "a string",
            Value::Array(_) => "an array",
            Value::Table(_) => "a table",
        }
    }

    /// Whether the value is truthy, for `when` conditions.
    ///
    /// Follows Jinja: empty string, empty collection, zero and `false` are
    /// falsy. Anything else is truthy.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Integer(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Table(t) => !t.is_empty(),
        }
    }

    /// The value as a string, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The value as an array, if it is one.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The value as a table, if it is one.
    pub fn as_table(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    /// Follow a dotted path such as `licenses.ids` into nested tables.
    ///
    /// Used by `choices_from`, which takes a path into the context rather than
    /// requiring the template author to serialise data into a string.
    pub fn get_path(&self, path: &str) -> Option<&Value> {
        let mut current = self;
        for segment in path.split('.') {
            current = current.as_table()?.get(segment)?;
        }
        Some(current)
    }

    /// A stable, canonical rendering, used to digest the answers.
    ///
    /// Not `Display`: this exists to be hashed, and must not change when the
    /// human-facing formatting does. Tables are emitted in key order, which
    /// `BTreeMap` gives us for free.
    pub fn canonical(&self) -> String {
        match self {
            Value::Bool(b) => format!("b:{b}"),
            Value::Integer(i) => format!("i:{i}"),
            // Floats are formatted with full precision so that two values that
            // are equal are digested identically.
            Value::Float(f) => format!("f:{f:?}"),
            Value::String(s) => format!("s:{}:{s}", s.len()),
            Value::Array(items) => {
                let inner: Vec<_> = items.iter().map(Value::canonical).collect();
                format!("a:[{}]", inner.join(","))
            }
            Value::Table(map) => {
                let inner: Vec<_> = map
                    .iter()
                    .map(|(k, v)| format!("{}:{}={}", k.len(), k, v.canonical()))
                    .collect();
                format!("t:{{{}}}", inner.join(","))
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Integer(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::String(s) => write!(f, "{s}"),
            Value::Array(items) => {
                let inner: Vec<_> = items.iter().map(ToString::to_string).collect();
                write!(f, "{}", inner.join(", "))
            }
            Value::Table(map) => {
                let inner: Vec<_> = map.iter().map(|(k, v)| format!("{k} = {v}")).collect();
                write!(f, "{{{}}}", inner.join(", "))
            }
        }
    }
}

// --- conversions ------------------------------------------------------------

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Integer(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_string())
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::Array(v.into_iter().map(Into::into).collect())
    }
}

impl From<toml::Value> for Value {
    fn from(v: toml::Value) -> Self {
        match v {
            toml::Value::Boolean(b) => Value::Bool(b),
            toml::Value::Integer(i) => Value::Integer(i),
            toml::Value::Float(f) => Value::Float(f),
            toml::Value::String(s) => Value::String(s),
            toml::Value::Array(items) => Value::Array(items.into_iter().map(Into::into).collect()),
            toml::Value::Table(map) => {
                Value::Table(map.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
            // TOML datetimes have no counterpart in the context, and inventing
            // one would put a machine-formatted timestamp into a rendered tree.
            // Keeping the source text preserves the information without
            // introducing a type templates would have to reason about.
            toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        }
    }
}

impl From<serde_json::Value> for Value {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Value::String(String::new()),
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(i) => Value::Integer(i),
                // A JSON number too large for i64, or fractional. `unwrap_or`
                // rather than a panic: a data file is untrusted input.
                None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Array(items) => {
                Value::Array(items.into_iter().map(Into::into).collect())
            }
            serde_json::Value::Object(map) => {
                Value::Table(map.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
        }
    }
}

impl From<serde_norway::Value> for Value {
    fn from(v: serde_norway::Value) -> Self {
        match v {
            // A YAML null is a written-out absence, unlike a question that was
            // never asked. Mapping it to the empty string keeps it a value the
            // template can interpolate, matching what JSON `null` does.
            serde_norway::Value::Null => Value::String(String::new()),
            serde_norway::Value::Bool(b) => Value::Bool(b),
            serde_norway::Value::Number(n) => match n.as_i64() {
                Some(i) => Value::Integer(i),
                // Too large for i64, or fractional. `unwrap_or` rather than a
                // panic: a data file is untrusted input.
                None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            serde_norway::Value::String(s) => Value::String(s),
            serde_norway::Value::Sequence(items) => {
                Value::Array(items.into_iter().map(Into::into).collect())
            }
            serde_norway::Value::Mapping(map) => Value::Table(
                map.into_iter()
                    // YAML permits any node as a key; the context is addressed
                    // by name, so a non-string key has no way to be referenced.
                    // Rendering it is closer to the author's intent than
                    // dropping the entry silently.
                    .map(|(k, v)| (yaml_key(&k), v.into()))
                    .collect(),
            ),
            // A tagged node such as `!Ref foo`. The tag is dropped and the
            // value kept: git-tpl has no user-defined types, and a tag is never
            // a request to construct one here — `!!python/object:os.system` is
            // inert input, not an instruction.
            serde_norway::Value::Tagged(tagged) => Value::from(tagged.value),
        }
    }
}

/// A YAML mapping key as the name the context will address it by.
fn yaml_key(key: &serde_norway::Value) -> String {
    match key {
        serde_norway::Value::String(s) => s.clone(),
        serde_norway::Value::Bool(b) => b.to_string(),
        serde_norway::Value::Number(n) => n.to_string(),
        serde_norway::Value::Null => String::new(),
        // A sequence or mapping used as a key. Rare enough that any stable
        // rendering will do; what matters is that it is deterministic.
        other => serde_norway::to_string(other)
            .unwrap_or_default()
            .trim_end()
            .to_string(),
    }
}

impl From<Value> for minijinja::Value {
    fn from(v: Value) -> Self {
        match v {
            Value::Bool(b) => minijinja::Value::from(b),
            Value::Integer(i) => minijinja::Value::from(i),
            Value::Float(f) => minijinja::Value::from(f),
            Value::String(s) => minijinja::Value::from(s),
            Value::Array(items) => minijinja::Value::from(
                items
                    .into_iter()
                    .map(Into::<minijinja::Value>::into)
                    .collect::<Vec<_>>(),
            ),
            Value::Table(map) => minijinja::Value::from_iter(
                map.into_iter()
                    .map(|(k, v)| (k, Into::<minijinja::Value>::into(v))),
            ),
        }
    }
}

impl Value {
    /// Convert a MiniJinja value back into a [`Value`].
    ///
    /// This is how a computed value or a dynamic default keeps its type. An
    /// expression producing a boolean must yield [`Value::Bool`], not the
    /// string `"true"`, or `{% if needs_tokio %}` in a template would be true
    /// for the string `"false"`.
    pub fn from_minijinja(v: &minijinja::Value) -> Result<Self, ValueError> {
        use minijinja::value::ValueKind;

        Ok(match v.kind() {
            ValueKind::Undefined | ValueKind::None => Value::String(String::new()),
            ValueKind::Bool => Value::Bool(v.is_true()),
            ValueKind::Number => {
                if let Ok(i) = i64::try_from(v.clone()) {
                    Value::Integer(i)
                } else {
                    Value::Float(f64::try_from(v.clone()).map_err(|_| {
                        ValueError::TypeMismatch {
                            expected: "a number",
                            actual: "an unrepresentable number",
                        }
                    })?)
                }
            }
            ValueKind::String => Value::String(v.to_string()),
            ValueKind::Seq => {
                let mut items = Vec::new();
                for item in v.try_iter().map_err(|_| ValueError::TypeMismatch {
                    expected: "an array",
                    actual: "a non-iterable sequence",
                })? {
                    items.push(Value::from_minijinja(&item)?);
                }
                Value::Array(items)
            }
            ValueKind::Map => {
                let mut map = BTreeMap::new();
                for key in v.try_iter().map_err(|_| ValueError::TypeMismatch {
                    expected: "a table",
                    actual: "a non-iterable map",
                })? {
                    let value = v.get_item(&key).map_err(|_| ValueError::TypeMismatch {
                        expected: "a table",
                        actual: "a map with an unreadable key",
                    })?;
                    map.insert(key.to_string(), Value::from_minijinja(&value)?);
                }
                Value::Table(map)
            }
            // Bytes, iterators, and anything else MiniJinja may add. Rendering
            // to a string is the only meaningful thing to do, and is what a
            // template would have got by interpolating it.
            _ => Value::String(v.to_string()),
        })
    }

    /// Parse a string into a value of the requested shape.
    ///
    /// Used for `--answer key=value`, which arrives as text and must become the
    /// question's declared type — a silent coercion to a string would make
    /// `--answer ci=false` truthy.
    pub fn parse_as(input: &str, expected: &'static str) -> Result<Self, ValueError> {
        match expected {
            "a string" => Ok(Value::String(input.to_string())),
            "a boolean" => match input.to_ascii_lowercase().as_str() {
                "true" | "yes" | "y" | "1" | "on" => Ok(Value::Bool(true)),
                "false" | "no" | "n" | "0" | "off" => Ok(Value::Bool(false)),
                _ => Err(ValueError::Parse {
                    input: input.to_string(),
                    expected: "a boolean",
                }),
            },
            "an integer" => input
                .trim()
                .parse::<i64>()
                .map(Value::Integer)
                .map_err(|_| ValueError::Parse {
                    input: input.to_string(),
                    expected: "an integer",
                }),
            // A list on the command line is comma-separated. Anything richer
            // belongs in the configuration file, where it has real TOML syntax.
            "an array" => Ok(Value::Array(
                input
                    .split(',')
                    .map(|s| Value::String(s.trim().to_string()))
                    .filter(|v| !matches!(v, Value::String(s) if s.is_empty()))
                    .collect(),
            )),
            _ => Ok(Value::String(input.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(Value::Bool(true), true)]
    #[case(Value::Bool(false), false)]
    #[case(Value::Integer(1), true)]
    #[case(Value::Integer(0), false)]
    #[case(Value::String("x".into()), true)]
    #[case(Value::String("".into()), false)]
    #[case(Value::Array(vec![Value::Bool(false)]), true)]
    #[case(Value::Array(vec![]), false)]
    fn an_empty_value_is_falsy_and_a_populated_one_is_truthy(
        #[case] value: Value,
        #[case] expected: bool,
    ) {
        assert_eq!(value.is_truthy(), expected);
    }

    #[test]
    fn a_dotted_path_walks_nested_tables() {
        let value = Value::Table(BTreeMap::from([(
            "licenses".to_string(),
            Value::Table(BTreeMap::from([(
                "ids".to_string(),
                Value::Array(vec![Value::String("MIT".into())]),
            )])),
        )]));

        assert_eq!(
            value.get_path("licenses.ids"),
            Some(&Value::Array(vec![Value::String("MIT".into())]))
        );
        assert_eq!(value.get_path("licenses.missing"), None);
        assert_eq!(value.get_path("nope.ids"), None);
    }

    /// A computed value that produced the *string* `"false"` instead of the
    /// boolean would make `{% if needs_tokio %}` true, and the template would
    /// render the opposite of what it says.
    #[test]
    fn a_boolean_survives_a_round_trip_through_minijinja() {
        let original = Value::Bool(false);
        let mj: minijinja::Value = original.clone().into();
        assert_eq!(Value::from_minijinja(&mj).unwrap(), original);
    }

    #[rstest]
    #[case(Value::Integer(42))]
    #[case(Value::String("hello".into()))]
    #[case(Value::Array(vec![Value::Integer(1), Value::String("a".into())]))]
    #[case(Value::Table(BTreeMap::from([("k".to_string(), Value::Bool(true))])))]
    fn values_survive_a_round_trip_through_minijinja(#[case] original: Value) {
        let mj: minijinja::Value = original.clone().into();
        assert_eq!(Value::from_minijinja(&mj).unwrap(), original);
    }

    #[rstest]
    #[case("true", "a boolean", Value::Bool(true))]
    #[case("yes", "a boolean", Value::Bool(true))]
    #[case("false", "a boolean", Value::Bool(false))]
    #[case("0", "a boolean", Value::Bool(false))]
    #[case("8080", "an integer", Value::Integer(8080))]
    #[case("hello", "a string", Value::String("hello".into()))]
    #[case("a, b", "an array", Value::Array(vec![Value::String("a".into()), Value::String("b".into())]))]
    fn a_command_line_answer_is_parsed_as_its_declared_type(
        #[case] input: &str,
        #[case] expected_type: &'static str,
        #[case] expected: Value,
    ) {
        assert_eq!(Value::parse_as(input, expected_type).unwrap(), expected);
    }

    /// `--answer port=nope` must be an error, not the string `"nope"` silently
    /// standing in for an integer.
    #[test]
    fn an_answer_of_the_wrong_type_is_rejected_rather_than_coerced() {
        std::assert_matches!(
            Value::parse_as("nope", "an integer"),
            Err(ValueError::Parse { .. })
        );
        std::assert_matches!(
            Value::parse_as("maybe", "a boolean"),
            Err(ValueError::Parse { .. })
        );
    }

    /// The digest goes into a commit trailer and is compared across runs, so
    /// two values that differ must never canonicalise the same.
    #[test]
    fn canonicalisation_distinguishes_values_that_display_identically() {
        assert_ne!(
            Value::String("1".into()).canonical(),
            Value::Integer(1).canonical()
        );
        assert_ne!(
            Value::String("true".into()).canonical(),
            Value::Bool(true).canonical()
        );
        // `{ab: c}` and `{a: bc}` would collide under naive concatenation.
        let ab_c = Value::Table(BTreeMap::from([("ab".into(), Value::String("c".into()))]));
        let a_bc = Value::Table(BTreeMap::from([("a".into(), Value::String("bc".into()))]));
        assert_ne!(ab_c.canonical(), a_bc.canonical());
    }

    #[test]
    fn canonicalisation_is_stable_across_table_insertion_order() {
        let mut first = BTreeMap::new();
        first.insert("z".to_string(), Value::Integer(1));
        first.insert("a".to_string(), Value::Integer(2));

        let mut second = BTreeMap::new();
        second.insert("a".to_string(), Value::Integer(2));
        second.insert("z".to_string(), Value::Integer(1));

        assert_eq!(
            Value::Table(first).canonical(),
            Value::Table(second).canonical()
        );
    }
}
