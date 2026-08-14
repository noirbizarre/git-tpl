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

    /// The expected sha256 of the raw bytes, as lowercase hex.
    ///
    /// A mismatch is an error, never a warning: the point of a pin is that the
    /// render stops rather than producing a plausible tree from content the
    /// template did not vouch for. Chiefly for remote sources, but accepted on
    /// any kind — a rule that applied to only one kind would cost more to
    /// explain than the check costs to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
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

            // An empty list can never produce a prompt, and unlike an empty
            // list arriving from `choices_from` it is knowable now. A dynamic
            // list that resolves to nothing is a legitimate runtime state and
            // skips the question; a literal `choices = []` is a mistake.
            if question.choices.as_ref().is_some_and(Vec::is_empty) {
                return Err(ManifestError::InvalidQuestion {
                    name: name.clone(),
                    reason: "`choices` is empty, so the question could never be answered".into(),
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

            if let Some(source) = &question.default_from {
                // Rejected here rather than at prompt time: a template author
                // who wrote `env:USER` must find out on the first render, not
                // on the machine of the user who has that variable set.
                let Some(key) = source.strip_prefix(super::question::GIT_PREFIX) else {
                    return Err(ManifestError::InvalidQuestion {
                        name: name.clone(),
                        reason: format!(
                            "`default_from = \"{source}\"` names no known source; the only form is `git:<key>`, as in `git:user.name`"
                        ),
                    });
                };

                if key.trim().is_empty() {
                    return Err(ManifestError::InvalidQuestion {
                        name: name.clone(),
                        reason: "`default_from` has no key after `git:`".into(),
                    });
                }

                // Git configuration values are strings. Coercing one into a
                // boolean or a choice would fail at the prompt, on one
                // machine, for one user — the worst place to discover it.
                if question.kind != super::QuestionKind::String {
                    return Err(ManifestError::InvalidQuestion {
                        name: name.clone(),
                        reason: format!(
                            "`default_from` only applies to `string` questions, not `{}`",
                            question.kind.type_name()
                        ),
                    });
                }
            }

            if let Some(pattern) = &question.pattern {
                // A pattern is checked against text. On a boolean or a choice
                // it would either never fail or always fail, and the author
                // would have to render to find out which.
                if question.kind != super::QuestionKind::String {
                    return Err(ManifestError::InvalidQuestion {
                        name: name.clone(),
                        reason: format!(
                            "`pattern` only applies to `string` questions, not `{}`",
                            question.kind.type_name()
                        ),
                    });
                }

                // Compiled now and discarded. The cost is one compile per load;
                // the alternative is a template author learning their pattern
                // does not parse from a bug report by one of their users.
                if let Err(error) = regex_lite::Regex::new(pattern) {
                    return Err(ManifestError::InvalidQuestion {
                        name: name.clone(),
                        reason: format!("`pattern` is not a valid regular expression: {error}"),
                    });
                }
            } else if question.message.is_some() {
                // Almost always a `pattern` that was renamed or deleted and a
                // `message` left behind, which would then never be shown.
                return Err(ManifestError::InvalidQuestion {
                    name: name.clone(),
                    reason: "`message` has no `pattern` to explain".into(),
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
    use crate::template::{Choice, QuestionKind};
    use rstest::rstest;

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

    /// A dynamic list that resolves to nothing skips the question, but a
    /// literal empty list is knowable at load time and can only be a mistake.
    #[test]
    fn an_empty_literal_choices_list_is_rejected() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.license]
            type = "choice"
            choices = []
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ManifestError::InvalidQuestion { ref reason, .. } if reason.contains("empty")
        ));
    }

    #[test]
    fn a_labelled_choice_parses_inline() {
        let manifest = Manifest::parse(
            r#"
            name = "x"
            [questions.license]
            type = "choice"
            choices = [
              "Apache-2.0",
              { value = "MIT", label = "MIT License", help = "Permissive" },
            ]
            "#,
            MANIFEST_NAME,
        )
        .unwrap();

        let choices = manifest.questions["license"].choices.as_ref().unwrap();
        assert_eq!(choices[0], Choice::bare("Apache-2.0"));
        assert_eq!(choices[1].value, "MIT");
        assert_eq!(choices[1].label(), "MIT License");
        assert_eq!(choices[1].help.as_deref(), Some("Permissive"));
    }

    /// The combination that was thought to need a `multi_choice_from` key. It
    /// is `type` and the source being independent, and it already worked.
    #[test]
    fn a_multi_choice_may_draw_its_choices_from_a_reference() {
        let manifest = Manifest::parse(
            r#"
            name = "x"
            [data.features]
            source = "data/features.toml"
            [questions.features]
            type = "multi_choice"
            choices_from = "data.features.all"
            "#,
            MANIFEST_NAME,
        )
        .unwrap();

        assert_eq!(
            manifest.questions["features"].choices_from.as_deref(),
            Some("data.features.all")
        );
    }

    /// A choice value has to survive `--answer x=v`, which parses as a string.
    #[test]
    fn a_non_string_choice_value_is_rejected_at_load_time() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.port]
            type = "choice"
            choices = [8080, 9090]
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();
        assert!(matches!(error, ManifestError::Parse { .. }));
    }

    /// A prompt seed is read from the machine, so the only source is the one
    /// ADR-006 sanctions. `env:` and friends stay shut.
    #[rstest]
    #[case("env:USER", "names no known source")]
    #[case("git:", "no key after")]
    fn default_from_must_name_a_supported_source(#[case] source: &str, #[case] expected: &str) {
        let error = Manifest::parse(
            &format!(
                r#"
                name = "x"
                [questions.author]
                type = "string"
                default_from = "{source}"
                "#
            ),
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { name, reason } = error else {
            panic!("expected an invalid question, got {error:?}");
        };
        assert_eq!(name, "author");
        assert!(reason.contains(expected), "reason was: {reason}");
    }

    /// Git configuration values are strings. Coercion would fail on one
    /// machine only, which is the worst place to find out.
    #[test]
    fn default_from_is_rejected_on_a_non_string_question() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.ci]
            type = "boolean"
            default_from = "git:tpl.ci"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { name, reason } = error else {
            panic!("expected an invalid question, got {error:?}");
        };
        assert_eq!(name, "ci");
        assert!(reason.contains("only applies to `string` questions"));
    }

    /// A pattern is checked against text; on any other kind it would either
    /// never fail or always fail, and rendering is a poor way to find out.
    #[test]
    fn a_pattern_is_rejected_on_a_non_string_question() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.ci]
            type = "boolean"
            pattern = "^y"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { name, reason } = error else {
            panic!("expected an invalid question, got {error:?}");
        };
        assert_eq!(name, "ci");
        assert!(reason.contains("`pattern` only applies to `string` questions"));
    }

    /// Compiled at load time so the author finds out, rather than a user of
    /// their template halfway through a questionnaire.
    #[test]
    fn an_uncompilable_pattern_is_rejected_at_load_time() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.slug]
            type = "string"
            pattern = "^[a-z"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { name, reason } = error else {
            panic!("expected an invalid question, got {error:?}");
        };
        assert_eq!(name, "slug");
        assert!(
            reason.contains("not a valid regular expression"),
            "{reason}"
        );
    }

    /// Otherwise a deleted `pattern` leaves a message that is never shown.
    #[test]
    fn a_message_without_a_pattern_is_rejected() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.slug]
            type = "string"
            message = "lowercase only"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { name, reason } = error else {
            panic!("expected an invalid question, got {error:?}");
        };
        assert_eq!(name, "slug");
        assert!(reason.contains("`message` has no `pattern`"), "{reason}");
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
