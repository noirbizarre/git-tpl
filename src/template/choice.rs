//! A single offered choice.

use serde::{Deserialize, Serialize, Serializer, de};
use thiserror::Error;

use super::Value;

/// The keys a choice table may carry in the manifest.
const KNOWN_KEYS: [&str; 3] = ["value", "label", "help"];

/// One option a `choice` or `multi_choice` question offers.
///
/// The same shape whether it was written inline in the manifest or read from a
/// data source, and read by the same code either way, so the two cannot drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// What is recorded as the answer.
    ///
    /// Only this reaches the context and the answers digest. A label is
    /// presentation, and changing one must not change a rendered tree.
    pub value: String,

    /// Shown instead of the value. Defaults to the value.
    pub label: Option<String>,

    /// Shown beside the label, for a choice that needs explaining.
    pub help: Option<String>,
}

impl Choice {
    /// A choice that is its own label.
    pub fn bare(value: impl Into<String>) -> Self {
        Choice {
            value: value.into(),
            label: None,
            help: None,
        }
    }

    /// What to show for this choice.
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.value)
    }

    /// Read a choice from a value that came from a **data source**.
    ///
    /// Lenient about extra keys, because a data file is a record that
    /// legitimately carries more than git-tpl needs — a licence list with
    /// `url` and `osi_approved` alongside `value` and `label` is not a mistake.
    /// The manifest is held to a stricter standard; see the `Deserialize` impl.
    pub fn from_value(value: &Value) -> Result<Self, ChoiceError> {
        Self::read(value, Strictness::IgnoreUnknown)
    }

    /// The shared reader behind both the manifest and the data path.
    fn read(value: &Value, strictness: Strictness) -> Result<Self, ChoiceError> {
        match value {
            Value::String(s) => Ok(Choice::bare(s.clone())),
            Value::Table(map) => {
                if strictness == Strictness::RejectUnknown
                    && let Some(unknown) = map.keys().find(|k| !KNOWN_KEYS.contains(&k.as_str()))
                {
                    return Err(ChoiceError::UnknownKey {
                        key: unknown.clone(),
                    });
                }

                let Some(value) = map.get("value") else {
                    return Err(ChoiceError::MissingValue);
                };
                let Value::String(value) = value else {
                    return Err(ChoiceError::NonStringValue {
                        found: value.type_name().to_string(),
                    });
                };

                Ok(Choice {
                    value: value.clone(),
                    label: text(map.get("label")),
                    help: text(map.get("help")),
                })
            }
            other => Err(ChoiceError::NonStringValue {
                found: other.type_name().to_string(),
            }),
        }
    }

    /// The offered values, for an error message.
    ///
    /// Values rather than labels: this is shown when an answer was rejected,
    /// and a value is what `--answer` and `.config/git.tpl.toml` take. Being
    /// told to choose "MIT License" when the accepted spelling is `MIT` would
    /// be worse than saying nothing.
    pub fn describe(choices: &[Choice]) -> String {
        choices
            .iter()
            .map(|c| c.value.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Whether an unrecognised key is a mistake or just extra information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strictness {
    /// The manifest: a key we do not know is a typo.
    RejectUnknown,
    /// A data source: a key we do not know is somebody else's field.
    IgnoreUnknown,
}

/// A label or help string, ignored unless it is text.
fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

impl<'de> Deserialize<'de> for Choice {
    /// Via [`Value`] rather than a derived untagged enum.
    ///
    /// An untagged enum reports a bad choice as "data did not match any variant
    /// of untagged enum", which names neither the key nor the problem. Going
    /// through `Value` means the manifest and a data source are read by the
    /// same function and report the same errors.
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Choice::read(&value, Strictness::RejectUnknown).map_err(de::Error::custom)
    }
}

impl Serialize for Choice {
    /// A choice with nothing but a value round-trips to the shorthand, so a
    /// serialised manifest reads the way it was written.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        if self.label.is_none() && self.help.is_none() {
            return serializer.serialize_str(&self.value);
        }

        let len = 1 + usize::from(self.label.is_some()) + usize::from(self.help.is_some());
        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("value", &self.value)?;
        if let Some(label) = &self.label {
            map.serialize_entry("label", label)?;
        }
        if let Some(help) = &self.help {
            map.serialize_entry("help", help)?;
        }
        map.end()
    }
}

/// Why a value could not be read as a choice.
///
/// No `Diagnostic`/`tpl::` code here, unlike most error types: a `ChoiceError`
/// never reaches a diagnostic on its own — `eval.rs` folds it into
/// `EvalError::BadChoices` (`tpl::eval::bad_choices`) via `to_string()`, so a
/// code of its own would be unreachable and would undercut "codes are the
/// stable surface".
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChoiceError {
    /// A table with no `value` key.
    #[error("a choice table needs a `value` key")]
    MissingValue,
    /// A choice whose value is not a string.
    #[error("a choice value must be a string, found {found}")]
    NonStringValue {
        /// What was found instead.
        found: String,
    },
    /// A manifest choice carrying a key git-tpl does not know.
    #[error("unknown key `{key}` in a choice; expected one of `value`, `label`, `help`")]
    UnknownKey {
        /// The offending key.
        key: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn table(pairs: &[(&str, Value)]) -> Value {
        Value::Table(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Wrapper {
        c: Choice,
    }

    #[test]
    fn a_scalar_choice_is_its_own_label() {
        let choice = Choice::from_value(&Value::String("MIT".into())).unwrap();
        assert_eq!(choice.value, "MIT");
        assert_eq!(choice.label(), "MIT");
        assert_eq!(choice.help, None);
    }

    #[test]
    fn a_labelled_choice_keeps_the_value_and_the_label_apart() {
        let choice = Choice::from_value(&table(&[
            ("value", Value::String("MIT".into())),
            ("label", Value::String("MIT License".into())),
            ("help", Value::String("Permissive".into())),
        ]))
        .unwrap();

        assert_eq!(choice.value, "MIT");
        assert_eq!(choice.label(), "MIT License");
        assert_eq!(choice.help.as_deref(), Some("Permissive"));
    }

    #[test]
    fn a_choice_table_without_a_value_is_rejected() {
        let error = Choice::from_value(&table(&[("label", Value::String("MIT License".into()))]))
            .unwrap_err();
        assert_eq!(error, ChoiceError::MissingValue);
    }

    /// `choices = [1, 2]` prompted correctly but could never be answered with
    /// `--answer x=1`, because a choice is parsed as a string. Rejecting it is
    /// how that stops being a silent half-working state.
    #[test]
    fn a_non_string_choice_value_is_rejected() {
        std::assert_matches!(
            Choice::from_value(&Value::Integer(1)),
            Err(ChoiceError::NonStringValue { .. })
        );
        std::assert_matches!(
            Choice::from_value(&table(&[("value", Value::Integer(1))])),
            Err(ChoiceError::NonStringValue { .. })
        );
    }

    /// Pointing `choices_from` at the wrong path is the likeliest mistake, and
    /// a nested array must not become a choice named `a`.
    #[test]
    fn a_nested_structure_is_not_a_choice() {
        assert!(Choice::from_value(&Value::Array(vec![Value::String("a".into())])).is_err());
    }

    #[test]
    fn the_shorthand_and_the_table_form_parse_to_the_same_choice() {
        let bare = toml::from_str::<Wrapper>(r#"c = "MIT""#).unwrap().c;
        let full = toml::from_str::<Wrapper>(r#"c = { value = "MIT" }"#)
            .unwrap()
            .c;
        assert_eq!(bare, full);
    }

    /// An unrecognised key in a manifest choice is a mistake the author wants
    /// to hear about before the first prompt, not a key that silently does
    /// nothing. `title` for `label` is the easy one to write.
    #[test]
    fn an_unknown_key_in_a_manifest_choice_is_rejected() {
        let error = toml::from_str::<Wrapper>(r#"c = { value = "MIT", title = "x" }"#).unwrap_err();
        assert!(
            error.to_string().contains("title"),
            "the error should name the offending key, got: {error}"
        );
    }

    /// A data file is a record, not a declaration. A licence list carrying
    /// `url` and `osi_approved` beside `value` is not a mistake, and refusing
    /// it would make most real data unusable without reshaping.
    #[test]
    fn an_extra_key_from_a_data_source_is_ignored() {
        let choice = Choice::from_value(&table(&[
            ("value", Value::String("MIT".into())),
            ("url", Value::String("https://example.com".into())),
        ]))
        .unwrap();
        assert_eq!(choice.value, "MIT");
    }

    #[test]
    fn a_bare_choice_serialises_back_to_the_shorthand() {
        let wrapper = Wrapper {
            c: Choice::bare("MIT"),
        };
        assert_eq!(toml::to_string(&wrapper).unwrap().trim(), r#"c = "MIT""#);
    }

    #[test]
    fn a_labelled_choice_survives_a_round_trip() {
        let wrapper = Wrapper {
            c: Choice {
                value: "MIT".into(),
                label: Some("MIT License".into()),
                help: None,
            },
        };
        let text = toml::to_string(&wrapper).unwrap();
        assert_eq!(toml::from_str::<Wrapper>(&text).unwrap().c, wrapper.c);
    }

    #[test]
    fn the_offered_values_are_described_by_value_not_by_label() {
        let choices = [
            Choice {
                value: "MIT".into(),
                label: Some("MIT License".into()),
                help: None,
            },
            Choice::bare("Apache-2.0"),
        ];
        assert_eq!(Choice::describe(&choices), "MIT, Apache-2.0");
    }
}
