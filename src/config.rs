//! `.config/git.tpl.toml` — the project's shared, versioned configuration.
//!
//! It contains only the template reference and the answers used to render it.
//! Nothing generated, no sync state, no local preferences. See
//! `docs/adr/010-config-location.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::template::Value;

/// Where the configuration lives, relative to the project root.
///
/// Under `.config/` rather than at the repository root: a generated project's
/// root is already crowded, and this is a file a user reads rarely.
pub const CONFIG_PATH: &str = ".config/git.tpl.toml";

/// Errors from reading or writing the project configuration.
#[derive(Debug, Error, Diagnostic)]
pub enum ConfigError {
    /// No configuration file at the expected path.
    #[error("no git-tpl configuration found at `{}`", path.display())]
    #[diagnostic(
        code(tpl::config::missing),
        help("run `git tpl init <template>` to attach a template to this repository")
    )]
    Missing {
        /// The path that was looked for.
        path: PathBuf,
    },

    /// The file could not be parsed as TOML, or does not match the schema.
    #[error("invalid configuration in `{name}`: {message}")]
    #[diagnostic(code(tpl::config::parse))]
    Parse {
        /// The file's display name.
        name: String,
        /// The parser's message.
        message: String,
        #[source_code]
        /// The file, for the diagnostic snippet.
        src: NamedSource<String>,
        #[label("here")]
        /// Where in it the parser gave up.
        span: SourceSpan,
    },

    /// The file could not be read or written.
    #[error("could not access `{}`", path.display())]
    #[diagnostic(code(tpl::config::io))]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Serialising the configuration failed.
    #[error("could not serialise the configuration")]
    #[diagnostic(code(tpl::config::serialise))]
    Serialise(#[from] toml::ser::Error),
}

/// The project configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Which template this project came from.
    pub template: TemplateRef,

    /// The answers used to render it.
    ///
    /// `BTreeMap` rather than a hash map so the file is written in a stable
    /// order. A configuration file whose key order changed on every write would
    /// produce a diff on every `update`, and reviewing it would be pointless.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub answers: BTreeMap<String, Value>,
}

/// The `[template]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateRef {
    /// Any Git URL, or a path on this machine.
    pub source: String,

    /// Branch, tag or commit. `None` means the remote's default branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,

    /// Override the derived template id, and so the ref name.
    ///
    /// Set this when a template moves address but is conceptually the same
    /// template: the id determines `refs/tpl/<id>`, and a changed id starts a
    /// new ref with no shared history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Override the manifest's rendered subdirectory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

impl Config {
    /// Create a configuration for a freshly initialised project.
    pub fn new(source: impl Into<String>, r#ref: Option<String>) -> Self {
        Self {
            template: TemplateRef {
                source: source.into(),
                r#ref,
                id: None,
                root: None,
            },
            answers: BTreeMap::new(),
        }
    }

    /// The configuration path for a project root.
    pub fn path_in(project_root: &Path) -> PathBuf {
        project_root.join(CONFIG_PATH)
    }

    /// Load the configuration from a project root.
    pub fn load(project_root: &Path) -> Result<Self, ConfigError> {
        let path = Self::path_in(project_root);
        if !path.exists() {
            return Err(ConfigError::Missing { path });
        }
        let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Io {
            path: path.clone(),
            source,
        })?;
        Self::parse(&text, &path.display().to_string())
    }

    /// Parse configuration text.
    pub fn parse(text: &str, name: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|error| {
            // The span lets miette underline the offending line rather than
            // making the user count lines from a message.
            let span = error
                .span()
                .map(|s| SourceSpan::from(s.start..s.end))
                .unwrap_or_else(|| SourceSpan::from(0..0));
            ConfigError::Parse {
                name: name.to_string(),
                message: error.message().to_string(),
                src: NamedSource::new(name, text.to_string()),
                span,
            }
        })
    }

    /// Write the configuration to a project root, creating `.config/`.
    pub fn save(&self, project_root: &Path) -> Result<PathBuf, ConfigError> {
        let path = Self::path_in(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&path, self.to_toml()?).map_err(|source| ConfigError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Render the configuration as TOML.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        let body = toml::to_string_pretty(self)?;
        // A header, because this file is meant to be read and hand-edited —
        // changing an answer and running `git tpl update` is the supported way
        // to change your mind about a choice made at init time.
        Ok(format!(
            "# git-tpl — https://noirbizarre.github.io/git-tpl/configuration/\n\
             #\n\
             # This file is versioned with the project. Edit an answer and run\n\
             # `git tpl update` to re-render the template with it.\n\
             \n\
             {body}"
        ))
    }

    /// Whether a configuration exists for a project root.
    pub fn exists_in(project_root: &Path) -> bool {
        Self::path_in(project_root).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_minimal_configuration_parses() {
        let config = Config::parse(
            r#"
            [template]
            source = "https://github.com/noirbizarre/rust-library"
            "#,
            "test.toml",
        )
        .unwrap();

        assert_eq!(
            config.template.source,
            "https://github.com/noirbizarre/rust-library"
        );
        assert_eq!(config.template.r#ref, None);
        assert!(config.answers.is_empty());
    }

    #[test]
    fn answers_keep_their_types() {
        let config = Config::parse(
            r#"
            [template]
            source = "../tpl"

            [answers]
            project_name = "example"
            ci = true
            port = 8080
            features = ["serde", "async"]
            "#,
            "test.toml",
        )
        .unwrap();

        assert_eq!(
            config.answers["project_name"],
            Value::String("example".into())
        );
        assert_eq!(config.answers["ci"], Value::Bool(true));
        assert_eq!(config.answers["port"], Value::Integer(8080));
        assert_eq!(
            config.answers["features"],
            Value::Array(vec![
                Value::String("serde".into()),
                Value::String("async".into())
            ])
        );
    }

    /// Writing must be lossless, or an `update` would silently drop an answer
    /// it did not understand and re-render without it.
    #[test]
    fn a_configuration_round_trips_through_toml() {
        let original = Config::parse(
            r#"
            [template]
            source = "https://github.com/noirbizarre/rust-library"
            ref = "v1.4.0"
            id = "legacy-name"
            root = "src"

            [answers]
            project_name = "example"
            ci = true
            port = 8080
            features = ["serde"]
            "#,
            "test.toml",
        )
        .unwrap();

        let text = original.to_toml().unwrap();
        let reparsed = Config::parse(&text, "test.toml").unwrap();

        assert_eq!(original, reparsed);
    }

    /// The file is hand-edited and reviewed in diffs, so a write that reordered
    /// keys would produce a spurious diff on every update.
    #[test]
    fn answers_are_written_in_a_stable_order() {
        let mut config = Config::new("../tpl", None);
        for key in ["zebra", "alpha", "mango"] {
            config
                .answers
                .insert(key.to_string(), Value::String(key.into()));
        }

        let first = config.to_toml().unwrap();
        let second = Config::parse(&first, "t.toml").unwrap().to_toml().unwrap();

        assert_eq!(first, second);
        let alpha = first.find("alpha").unwrap();
        let mango = first.find("mango").unwrap();
        let zebra = first.find("zebra").unwrap();
        assert!(alpha < mango && mango < zebra, "not alphabetical:\n{first}");
    }

    #[test]
    fn a_missing_source_is_a_parse_error_pointing_at_the_file() {
        let error = Config::parse("[template]\n", "test.toml").unwrap_err();
        std::assert_matches!(error, ConfigError::Parse { .. });
    }

    #[test]
    fn a_missing_file_is_reported_with_the_path_that_was_looked_for() {
        let dir = tempfile::tempdir().unwrap();
        let error = Config::load(dir.path()).unwrap_err();
        match error {
            ConfigError::Missing { path } => {
                assert!(path.ends_with(CONFIG_PATH), "unexpected path: {path:?}");
            }
            other => panic!("expected Missing, got {other:?}"),
        }
    }

    #[test]
    fn saving_creates_the_config_directory() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::new("../tpl", Some("main".into()));

        let path = config.save(dir.path()).unwrap();

        assert!(path.exists());
        assert!(Config::exists_in(dir.path()));
        assert_eq!(Config::load(dir.path()).unwrap(), config);
    }
}
