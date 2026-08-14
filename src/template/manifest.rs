//! The `template.toml` manifest.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{Question, Value};

/// The manifest's filename, at the template repository root.
pub const MANIFEST_NAME: &str = "template.toml";

/// The subdirectory rendered when the manifest does not say otherwise.
///
/// Only this subtree is rendered, so a template repository can carry its own
/// README, LICENSE and CI without them being rendered into every project.
pub const DEFAULT_ROOT: &str = "template";

/// Errors from loading a manifest.
#[derive(Debug, Error, Diagnostic)]
pub enum ManifestError {
    /// The template repository has no manifest.
    #[error("`{origin}` has no {MANIFEST_NAME}")]
    #[diagnostic(
        code(tpl::manifest::missing),
        help("a template is a Git repository with a `template.toml` at its root")
    )]
    Missing {
        /// The template source.
        // Not named `source`: thiserror reserves that name for `#[source]`.
        origin: String,
    },

    /// The manifest is not valid TOML, or does not match the schema.
    #[error("invalid {MANIFEST_NAME}: {message}")]
    #[diagnostic(code(tpl::manifest::parse))]
    Parse {
        /// The parser's message.
        message: String,
        #[source_code]
        /// The manifest, for the diagnostic snippet.
        src: NamedSource<String>,
        #[label("here")]
        /// Where in it the parser gave up.
        span: SourceSpan,
    },

    /// A question and a computed value share a name.
    #[error("`{name}` is declared both as a question and as a computed value")]
    #[diagnostic(
        code(tpl::manifest::name_collision),
        help(
            "answers and computed values share one namespace, so a template cannot tell them apart. Rename one."
        )
    )]
    NameCollision {
        /// The colliding name.
        name: String,
    },

    /// A choice question has neither `choices` nor `choices_from`, or has both.
    #[error("question `{name}`: {reason}")]
    #[diagnostic(code(tpl::manifest::invalid_question))]
    InvalidQuestion {
        /// The question's name.
        name: String,
        /// What is wrong with it.
        reason: String,
    },
}

/// A declared data source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataSourceDecl {
    /// Where the data comes from. May be an expression.
    pub source: String,

    /// `template`, `local` or `remote`. Inferred from `source` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// `toml` or `json`. Inferred from the extension when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// The parsed `template.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// The template's name.
    pub name: String,

    /// One line, shown when prompting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The subdirectory that gets rendered.
    #[serde(default = "default_root")]
    pub root: String,

    /// Declared data sources, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, DataSourceDecl>,

    /// The questions, in declaration order.
    ///
    /// Order is preserved because it breaks ties in the dependency sort — two
    /// questions that do not depend on each other are asked in the order they
    /// were written, which is the only ordering a template author controls.
    #[serde(default)]
    pub questions: IndexMap<String, Question>,

    /// Computed values, by name.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub computed: IndexMap<String, String>,
}

fn default_root() -> String {
    DEFAULT_ROOT.to_string()
}

impl Manifest {
    /// Parse a manifest and validate what can be validated without a context.
    pub fn parse(text: &str, name: &str) -> Result<Self, ManifestError> {
        let manifest: Self = toml::from_str(text).map_err(|error| {
            let span = error
                .span()
                .map(|s| SourceSpan::from(s.start..s.end))
                .unwrap_or_else(|| SourceSpan::from(0..0));
            ManifestError::Parse {
                message: error.message().to_string(),
                src: NamedSource::new(name, text.to_string()),
                span,
            }
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Checks that do not need a resolved context.
    ///
    /// Run at load time so a broken manifest fails before any prompt appears,
    /// rather than after the user has answered six questions.
    fn validate(&self) -> Result<(), ManifestError> {
        for name in self.computed.keys() {
            if self.questions.contains_key(name) {
                return Err(ManifestError::NameCollision { name: name.clone() });
            }
        }

        for (name, question) in &self.questions {
            let has_choices = question.choices.is_some();
            let has_choices_from = question.choices_from.is_some();

            if question.kind.is_choice() && !has_choices && !has_choices_from {
                return Err(ManifestError::InvalidQuestion {
                    name: name.clone(),
                    reason: format!(
                        "a `{}` question needs `choices` or `choices_from`",
                        match question.kind {
                            super::QuestionKind::MultiChoice => "multi_choice",
                            _ => "choice",
                        }
                    ),
                });
            }

            if has_choices && has_choices_from {
                return Err(ManifestError::InvalidQuestion {
                    name: name.clone(),
                    reason: "`choices` and `choices_from` are mutually exclusive".into(),
                });
            }

            if !question.kind.is_choice() && (has_choices || has_choices_from) {
                return Err(ManifestError::InvalidQuestion {
                    name: name.clone(),
                    reason: format!(
                        "`choices` only applies to `choice` and `multi_choice` questions, not `{}`",
                        question.kind.type_name()
                    ),
                });
            }
        }

        Ok(())
    }

    /// Template metadata, as exposed to expressions under `template`.
    ///
    /// Deliberately excludes the template's own revision: a file rendering its
    /// template's SHA would change on every template commit, which is exactly
    /// the non-determinism the design avoids. The revision goes in the commit
    /// trailers instead. See `docs/adr/006-no-runtime-context.md`.
    pub fn metadata(&self) -> Value {
        let mut map = BTreeMap::new();
        map.insert("name".to_string(), Value::String(self.name.clone()));
        if let Some(description) = &self.description {
            map.insert(
                "description".to_string(),
                Value::String(description.clone()),
            );
        }
        Value::Table(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::QuestionKind;

    const FULL: &str = r#"
        name = "rust-library"
        description = "A small Rust library"

        [data.licenses]
        source = "data/licenses.toml"

        [questions.project_name]
        type = "string"
        prompt = "Project name"

        [questions.license]
        type = "choice"
        prompt = "License"
        choices = ["MIT", "Apache-2.0"]
        default = "MIT"

        [questions.ci]
        type = "boolean"
        prompt = "Enable CI?"
        default = true

        [questions.cli]
        type = "boolean"
        prompt = "Create a CLI?"
        when = "{{ project_type == 'application' }}"

        [computed]
        package_name = "{{ project_name | lower | replace(' ', '-') }}"
    "#;

    #[test]
    fn a_full_manifest_parses() {
        let manifest = Manifest::parse(FULL, MANIFEST_NAME).unwrap();

        assert_eq!(manifest.name, "rust-library");
        assert_eq!(
            manifest.description.as_deref(),
            Some("A small Rust library")
        );
        assert_eq!(manifest.root, DEFAULT_ROOT);
        assert_eq!(manifest.questions.len(), 4);
        assert_eq!(manifest.data.len(), 1);
        assert_eq!(manifest.computed.len(), 1);

        let license = manifest.questions.get("license").unwrap();
        assert_eq!(license.kind, QuestionKind::Choice);
        assert_eq!(license.default, Some(Value::String("MIT".into())));
    }

    /// Declaration order breaks ties in the dependency sort, so it is the only
    /// ordering a template author controls and must survive parsing.
    #[test]
    fn questions_keep_their_declaration_order() {
        let manifest = Manifest::parse(FULL, MANIFEST_NAME).unwrap();
        let names: Vec<_> = manifest.questions.keys().cloned().collect();
        assert_eq!(names, ["project_name", "license", "ci", "cli"]);
    }

    #[test]
    fn the_rendered_root_defaults_to_template() {
        let manifest = Manifest::parse(r#"name = "x""#, MANIFEST_NAME).unwrap();
        assert_eq!(manifest.root, "template");
    }

    #[test]
    fn the_rendered_root_can_be_overridden() {
        let manifest = Manifest::parse("name = \"x\"\nroot = \"src\"", MANIFEST_NAME).unwrap();
        assert_eq!(manifest.root, "src");
    }

    /// Answers and computed values share one namespace, so a collision would
    /// mean one silently shadowing the other depending on evaluation order.
    #[test]
    fn a_question_and_a_computed_value_may_not_share_a_name() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.package_name]
            type = "string"
            [computed]
            package_name = "{{ x }}"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ManifestError::NameCollision { ref name } if name == "package_name"
        ));
    }

    #[test]
    fn a_choice_question_without_choices_is_rejected() {
        let error = Manifest::parse(
            "name = \"x\"\n[questions.license]\ntype = \"choice\"",
            MANIFEST_NAME,
        )
        .unwrap_err();
        assert!(matches!(error, ManifestError::InvalidQuestion { .. }));
    }

    #[test]
    fn a_question_with_both_choices_and_choices_from_is_rejected() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.license]
            type = "choice"
            choices = ["MIT"]
            choices_from = "data.licenses"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();
        assert!(matches!(error, ManifestError::InvalidQuestion { .. }));
    }

    #[test]
    fn choices_on_a_non_choice_question_are_rejected() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.ci]
            type = "boolean"
            choices = ["yes", "no"]
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();
        assert!(matches!(error, ManifestError::InvalidQuestion { .. }));
    }

    #[test]
    fn a_missing_name_is_a_parse_error() {
        let error = Manifest::parse("description = \"x\"", MANIFEST_NAME).unwrap_err();
        assert!(matches!(error, ManifestError::Parse { .. }));
    }

    /// The template's revision is deliberately absent: a file rendering its own
    /// template's SHA would change on every template commit.
    #[test]
    fn template_metadata_exposes_only_name_and_description() {
        let manifest = Manifest::parse(FULL, MANIFEST_NAME).unwrap();
        let table = manifest.metadata();
        let table = table.as_table().unwrap();

        assert_eq!(table.len(), 2);
        assert!(table.contains_key("name"));
        assert!(table.contains_key("description"));
    }
}
