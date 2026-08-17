//! The `template.toml` manifest.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{Question, Value, is_expression};

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

    /// Both note forms are declared.
    #[error("`note` and `note_file` are mutually exclusive")]
    #[diagnostic(
        code(tpl::manifest::conflicting_note),
        help(
            "keep one. `note_file` names a path in the template repository, \
             read once and shown after `init`; `note` is a literal, and is \
             better for a single line."
        )
    )]
    ConflictingNote,

    /// A declared remote is unusable.
    #[error("remote `{name}`: {reason}")]
    #[diagnostic(
        code(tpl::manifest::invalid_remote),
        help("a remote is `<name> = \"<url>\"` under `[remotes]`; the URL may be an expression")
    )]
    InvalidRemote {
        /// The remote's name, as written.
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

    /// `template`, `local`, `remote` or `git`. Inferred from `source` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// The revision a `git` source is read at — branch, tag or SHA. May be an
    /// expression.
    //
    // `reference`, not `ref`: `ref` is a Rust keyword, and the one-name-per-
    // concept rule reserves `revision` for the resolved `Oid`. The TOML key
    // stays `ref`, because that is what a template author writes.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// The path inside a `git` source's repository. May be an expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

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

impl DataSourceDecl {
    /// The location this declaration names, before anything is evaluated.
    ///
    /// `repo@ref:path` when the explicit triple is used, and the bare `source`
    /// otherwise. One producer, so the confirmation prompt, the cache key, the
    /// provenance trailer and every error's `location:` cannot come to
    /// disagree about what a source is.
    pub fn declared_location(&self) -> String {
        match (&self.reference, &self.path) {
            (Some(reference), Some(path)) => format!("{}@{reference}:{path}", self.source),
            // A half-declared triple is refused at load time; showing what was
            // actually written is more use here than inventing the missing half.
            _ => self.source.clone(),
        }
    }
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

    /// Fail on an undeclared name in a rendered file, rather than rendering it
    /// to an empty string.
    ///
    /// MiniJinja is lenient by default, which means `{{ typo }}` produces
    /// nothing and the command succeeds — leaving a `Cargo.toml` with
    /// `name = ""` that parses, or a workflow with `runs-on: ` that is valid
    /// YAML. A manifest expression with the same typo is a hard error before
    /// the first prompt; a file body was not, and the asymmetry is not
    /// defensible once noticed.
    ///
    /// Opt-in for now: `git tpl lint` reports the same names as warnings, so
    /// that the default flipping in a later release is a change template
    /// authors have already been told about. See ADR-014.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,

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

    /// A note shown to the user once, after `init`. May be an expression.
    ///
    /// The one thing a template could not previously do: say anything at all.
    /// A template that renders a `scripts/bootstrap.sh` had no way to mention
    /// it, which is most of what motivated the post-render tasks declined in
    /// ADR-019.
    ///
    /// A literal, and so bounded by what fits comfortably in a TOML string.
    /// [`Self::note_file`] is the choice for anything longer.
    ///
    /// Not named `message`: `message` is already a *question's* key, the one
    /// that explains its `pattern`. TOML being what it is, a top-level
    /// `message =` written after any table would silently become that
    /// question's, which is a mistake nothing could diagnose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,

    /// A path in the *template repository* whose content is shown after `init`.
    ///
    /// Repository-root-relative, like a partial and unlike a rendered file —
    /// the note is read from the template, never written into the project. A
    /// note is guidance, and a template that wants a durable file renders one
    /// and says so in the note.
    ///
    /// Rendered if and only if the path ends in `.jinja`, which is the same
    /// rule the renderer applies to files. The path itself may be an
    /// expression, so a template can choose its note by the answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_file: Option<String>,

    /// Git remotes to add on `init`, by name. URLs may be expressions.
    ///
    /// Not to be confused with a `remote` *data source*, which is an HTTP URL
    /// the loader reads. These are Git remotes, added through `GitBackend` —
    /// never fetched and never pushed.
    ///
    /// An `IndexMap`, so the order they are added and reported in is the order
    /// they were written: the only ordering a template author controls, and one
    /// a `BTreeMap` would silently replace with alphabetical.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub remotes: IndexMap<String, String>,
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
        // One note, or none. A manifest declaring both is not ambiguous by
        // accident — it is a `note` that was moved into a file and left
        // behind, and picking one silently would show the stale half.
        if self.note.is_some() && self.note_file.is_some() {
            return Err(ManifestError::ConflictingNote);
        }

        for (name, url) in &self.remotes {
            // Checked at load time because the failure is otherwise discovered
            // after the render, after the merge, at the very last step of an
            // `init` that has already written to the user's repository.
            if name.trim().is_empty() {
                return Err(ManifestError::InvalidRemote {
                    name: name.clone(),
                    reason: "a remote needs a name".into(),
                });
            }
            if url.trim().is_empty() {
                return Err(ManifestError::InvalidRemote {
                    name: name.clone(),
                    reason: "the URL is empty".into(),
                });
            }
            // Git's own restriction, applied here so that the diagnostic names
            // the manifest rather than surfacing from libgit2 at the end of an
            // `init`.
            if name.contains(['/', ' ', '\t']) {
                return Err(ManifestError::InvalidRemote {
                    name: name.clone(),
                    reason: "a remote name may not contain a slash or whitespace".into(),
                });
            }
        }

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
                // Everything below is rejected here rather than at prompt time:
                // a template author who wrote `env:USER` must find out on the
                // first render, not on the machine of the user who has that
                // variable set.
                if let Some(key) = source.strip_prefix(super::question::GIT_PREFIX) {
                    if key.trim().is_empty() {
                        return Err(ManifestError::InvalidQuestion {
                            name: name.clone(),
                            reason: "`default_from` has no key after `git:`".into(),
                        });
                    }
                } else if is_expression(source) {
                    validate_seed_expression(name, source)?;
                } else {
                    return Err(ManifestError::InvalidQuestion {
                        name: name.clone(),
                        reason: format!(
                            "`default_from = \"{source}\"` names no known source; \
                             it is either `git:<key>`, as in `git:user.name`, \
                             or an expression over {}, as in \
                             `{{{{ remote.name | default(dir.name) | slugify }}}}`",
                            namespace_list()
                        ),
                    });
                }

                // A seed is text. Coercing it into a boolean or a choice would
                // fail at the prompt, on one machine, for one user — the worst
                // place to discover it.
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

/// The seed namespaces, as an English list for a diagnostic.
fn namespace_list() -> String {
    crate::seed::NAMESPACES
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Check a `default_from` expression before anything can be prompted.
///
/// Two checks, both of which exist so that the author sees the mistake and not
/// their user:
///
/// 1. It parses, using the same engine that will evaluate it.
/// 2. Every root it names is a seed namespace. This is what stops
///    `{{ project_name }}` — a name from the *render* context, which the seed
///    context deliberately does not have — from silently seeding nothing. The
///    seed environment is chainable, so without this check the mistake would
///    produce an empty prompt and no message at all.
fn validate_seed_expression(name: &str, expression: &str) -> Result<(), ManifestError> {
    let env = crate::eval::seed_environment();
    let template =
        env.template_from_str(expression)
            .map_err(|error| ManifestError::InvalidQuestion {
                name: name.to_string(),
                reason: format!("`default_from` is not a valid expression: {error}"),
            })?;

    // `nested: false` yields roots only. A missing leaf under a real namespace
    // is the case `| default(...)` is for, and refusing it here would forbid
    // the idiom the feature exists to enable.
    for reference in template.undeclared_variables(false) {
        if crate::seed::NAMESPACES.contains(&reference.as_str()) {
            continue;
        }
        let suggestion = crate::suggest::closest(&reference, crate::seed::NAMESPACES)
            .map(|name| format!("; did you mean `{name}`?"))
            .unwrap_or_default();
        return Err(ManifestError::InvalidQuestion {
            name: name.to_string(),
            reason: format!(
                "`default_from` references `{reference}`, which is not a seed namespace; \
                 a seed may read only {}{suggestion}",
                namespace_list()
            ),
        });
    }

    Ok(())
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

    /// Git configuration values are strings, and so is a rendered seed
    /// expression. Coercion would fail on one machine only, which is the worst
    /// place to find out. The rule predates expressions and must survive them.
    #[rstest]
    #[case("git:tpl.ci")]
    #[case("{{ remote.name }}")]
    fn default_from_is_rejected_on_a_non_string_question(#[case] source: &str) {
        let error = Manifest::parse(
            &format!(
                r#"
                name = "x"
                [questions.ci]
                type = "boolean"
                default_from = "{source}"
                "#
            ),
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { name, reason } = error else {
            panic!("expected an invalid question, got {error:?}");
        };
        assert_eq!(name, "ci");
        assert!(reason.contains("only applies to `string` questions"));
    }

    /// The form every manifest written before expressions existed uses. It must
    /// keep loading, unchanged, or the feature is a breaking change.
    #[test]
    fn the_git_shorthand_is_still_accepted() {
        let manifest = Manifest::parse(
            r#"
            name = "x"
            [questions.author]
            type = "string"
            default_from = "git:user.name"
            "#,
            MANIFEST_NAME,
        )
        .unwrap();

        let question = &manifest.questions["author"];
        assert_eq!(question.git_config_key(), Some("user.name"));
        assert_eq!(question.default_from_expression(), None);
    }

    #[test]
    fn a_seed_expression_is_accepted_and_is_not_a_config_key() {
        let manifest = Manifest::parse(
            r#"
            name = "x"
            [questions.slug]
            type = "string"
            default_from = "{{ remote.name | default(dir.name) | slugify }}"
            "#,
            MANIFEST_NAME,
        )
        .unwrap();

        let question = &manifest.questions["slug"];
        assert_eq!(question.git_config_key(), None);
        assert_eq!(
            question.default_from_expression(),
            Some("{{ remote.name | default(dir.name) | slugify }}")
        );
    }

    /// Parsed with the engine that will evaluate it, so a syntax error is the
    /// author's problem on their first render and never anybody else's.
    #[test]
    fn a_default_from_expression_is_parsed_at_load_time() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.slug]
            type = "string"
            default_from = "{{ remote.name | }}"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { name, reason } = error else {
            panic!("expected an invalid question, got {error:?}");
        };
        assert_eq!(name, "slug");
        assert!(
            reason.contains("not a valid expression"),
            "reason was: {reason}"
        );
    }

    /// The seed context is not the render context. Without this check the
    /// mistake is invisible: a chainable environment renders the unknown name
    /// to nothing, and the author gets an empty prompt with no explanation.
    #[test]
    fn a_default_from_expression_may_only_reference_the_seed_namespaces() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.project_name]
            type = "string"
            [questions.slug]
            type = "string"
            default_from = "{{ project_name | slugify }}"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { name, reason } = error else {
            panic!("expected an invalid question, got {error:?}");
        };
        assert_eq!(name, "slug");
        assert!(
            reason.contains("not a seed namespace"),
            "reason was: {reason}"
        );
    }

    #[test]
    fn a_misspelled_seed_namespace_is_suggested() {
        let error = Manifest::parse(
            r#"
            name = "x"
            [questions.slug]
            type = "string"
            default_from = "{{ remotes.name }}"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidQuestion { reason, .. } = error else {
            panic!("expected an invalid question");
        };
        assert!(
            reason.contains("did you mean `remote`?"),
            "reason was: {reason}"
        );
    }

    /// A missing *leaf* under a real namespace is the whole point of
    /// `| default(...)` and must not be confused with a missing namespace.
    #[test]
    fn an_unset_key_under_a_real_namespace_is_not_a_load_time_error() {
        Manifest::parse(
            r#"
            name = "x"
            [questions.author]
            type = "string"
            default_from = "{{ git.user.nickname | default(git.user.name) }}"
            "#,
            MANIFEST_NAME,
        )
        .unwrap();
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

    /// ADR-019. A template may address the user; the note is not in the
    /// render context and a rendered file cannot read it.
    #[test]
    fn a_note_is_not_exposed_to_the_render_context() {
        let manifest =
            Manifest::parse("name = \"x\"\nnote = \"run bootstrap.sh\"", MANIFEST_NAME).unwrap();
        let table = manifest.metadata();
        let table = table.as_table().unwrap();
        assert!(!table.contains_key("note"));
    }

    #[test]
    fn both_note_forms_at_once_are_rejected() {
        let error = Manifest::parse(
            r#"
            name = "x"
            note = "hi"
            note_file = "docs/NEXT.md"
            "#,
            MANIFEST_NAME,
        )
        .unwrap_err();
        assert!(matches!(error, ManifestError::ConflictingNote));
    }

    #[test]
    fn either_note_form_alone_parses() {
        let inline = Manifest::parse("name = \"x\"\nnote = \"hi\"", MANIFEST_NAME).unwrap();
        assert_eq!(inline.note.as_deref(), Some("hi"));
        assert_eq!(inline.note_file, None);

        let from_file =
            Manifest::parse("name = \"x\"\nnote_file = \"docs/N.md\"", MANIFEST_NAME).unwrap();
        assert_eq!(from_file.note_file.as_deref(), Some("docs/N.md"));
        assert_eq!(from_file.note, None);
    }

    /// The rename exists because of exactly this: TOML would fold a top-level
    /// `message` written after a table into that question, silently.
    #[test]
    fn a_top_level_note_does_not_collide_with_a_questions_own_message() {
        let manifest = Manifest::parse(
            r#"
            name = "x"

            [questions.slug]
            type = "string"
            pattern = "^[a-z]+$"
            message = "lowercase only"

            [remotes]
            origin = "https://example.invalid/x.git"
            "#,
            MANIFEST_NAME,
        )
        .unwrap();

        assert_eq!(manifest.note, None);
        assert_eq!(
            manifest.questions["slug"].message.as_deref(),
            Some("lowercase only")
        );
    }

    /// Declaration order is the only ordering a template author controls, and
    /// it is the order the remotes are added and reported in.
    #[test]
    fn remotes_keep_their_declaration_order() {
        let manifest = Manifest::parse(
            r#"
            name = "x"
            [remotes]
            upstream = "https://example.invalid/up.git"
            origin = "https://example.invalid/origin.git"
            "#,
            MANIFEST_NAME,
        )
        .unwrap();
        let names: Vec<_> = manifest.remotes.keys().cloned().collect();
        assert_eq!(names, ["upstream", "origin"]);
    }

    /// Caught at load time, because the alternative is finding out after the
    /// render and after the merge, at the last step of an `init`.
    #[rstest]
    #[case("\"\"", "https://x.invalid/r.git", "needs a name")]
    #[case("origin", "", "URL is empty")]
    #[case("or/igin", "https://x.invalid/r.git", "slash or whitespace")]
    fn an_unusable_remote_is_rejected_at_load_time(
        #[case] name: &str,
        #[case] url: &str,
        #[case] expected: &str,
    ) {
        // The empty-name case has to be written as a quoted key.
        let key = if name == "\"\"" {
            name.to_string()
        } else {
            format!("\"{name}\"")
        };
        let error = Manifest::parse(
            &format!("name = \"x\"\n[remotes]\n{key} = \"{url}\""),
            MANIFEST_NAME,
        )
        .unwrap_err();

        let ManifestError::InvalidRemote { reason, .. } = error else {
            panic!("expected an invalid remote, got {error:?}");
        };
        assert!(reason.contains(expected), "reason was: {reason}");
    }

    /// The keys are additive: a manifest written before ADR-019 still parses,
    /// and declares neither.
    #[test]
    fn a_manifest_without_a_note_or_remotes_still_parses() {
        let manifest = Manifest::parse(FULL, MANIFEST_NAME).unwrap();
        assert_eq!(manifest.note, None);
        assert_eq!(manifest.note_file, None);
        assert!(manifest.remotes.is_empty());
    }
}
