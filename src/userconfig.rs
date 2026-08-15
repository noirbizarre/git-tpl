//! `~/.config/git-tpl/config.toml` — preferences belonging to a person.
//!
//! The third of three configuration owners, and the one that is never shared:
//!
//! ```text
//! .config/git.tpl.toml          →  the project. Versioned. Everyone gets it.
//! ~/.config/git-tpl/config.toml →  you. Never committed, never read by anyone else.
//! .git/config, ~/.gitconfig     →  tpl.* preferences
//! ```
//!
//! Nothing here may reach a rendered tree. `[defaults]` seeds a prompt and is
//! ignored when nobody is prompted, `[shortcuts]` are expanded before the URL
//! is recorded, and `[trust]` authorises a fetch rather than changing what is
//! rendered. See `docs/adr/013-user-configuration.md`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;

use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::Deserialize;
use thiserror::Error;

use crate::template::Value;

/// The application directory under `$XDG_CONFIG_HOME`.
pub const CONFIG_DIR: &str = "git-tpl";

/// The file within it.
pub const CONFIG_FILE: &str = "config.toml";

/// URL schemes a shortcut name may not shadow.
///
/// Expansion triggers on a leading `<name>:`, which is the shape of a scheme.
/// A shortcut called `https` would make `https://example.com` unaddressable,
/// so the name is refused rather than the collision resolved.
const RESERVED: &[&str] = &["https", "http", "ssh", "git", "file"];

/// Errors from reading the user configuration.
#[derive(Debug, Error, Diagnostic)]
pub enum UserConfigError {
    /// The file could not be read.
    #[error("could not read `{}`", path.display())]
    #[diagnostic(code(tpl::userconfig::io))]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// The file could not be parsed as TOML, or does not match the schema.
    #[error("invalid user configuration in `{name}`: {message}")]
    #[diagnostic(
        code(tpl::userconfig::parse),
        url("https://noirbizarre.github.io/git-tpl/configuration/")
    )]
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

    /// A `[shortcuts]` name cannot be used.
    #[error("`{name}` cannot be a shortcut: {reason}")]
    #[diagnostic(
        code(tpl::userconfig::shortcut),
        help("rename it to something that is neither a URL scheme nor a path")
    )]
    Shortcut {
        /// The offending name.
        name: String,
        /// Why it was refused.
        reason: String,
    },
}

/// The user's preferences.
///
/// `deny_unknown_fields`, unlike the project configuration: nothing generates
/// this file, so an unrecognised key is a typo, and silently ignoring it is how
/// somebody spends an afternoon wondering why a shortcut had no effect.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    /// Prompt defaults, keyed by question name.
    ///
    /// A seed and never an answer. See [`UserConfig`]'s module documentation
    /// and `docs/adr/013-user-configuration.md`.
    #[serde(default)]
    pub defaults: BTreeMap<String, Value>,

    /// URL prefixes, keyed by the name written before the `:`.
    #[serde(default)]
    pub shortcuts: BTreeMap<String, String>,

    /// Templates whose declared capabilities are authorised without a prompt.
    #[serde(default)]
    pub trust: Trust,
}

/// The `[trust]` table.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trust {
    /// Glob patterns matched against the normalised template URL.
    #[serde(default)]
    pub templates: Vec<String>,
}

impl UserConfig {
    /// Where the configuration lives, if this machine has a home directory.
    ///
    /// `$XDG_CONFIG_HOME` first, then `$HOME/.config`. Hand-written rather than
    /// taken from a crate: it is six lines, and it is the same rule
    /// `git::libgit2` already applies when looking for SSH keys.
    ///
    /// `None` when neither variable is set — a container with no home is a
    /// normal place to run git-tpl, and it simply has no user configuration.
    pub fn path() -> Option<PathBuf> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            // An empty or relative value is unset, per the XDG specification.
            Some(value) if PathBuf::from(&value).is_absolute() => PathBuf::from(value),
            _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
        };
        Some(base.join(CONFIG_DIR).join(CONFIG_FILE))
    }

    /// Load the user configuration, or the empty one.
    ///
    /// An absent file is the normal case and not an error. A file that exists
    /// but cannot be read or parsed *is*: the user wrote it, and silently
    /// ignoring what they wrote is worse than stopping.
    pub fn load() -> Result<Self, UserConfigError> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        Self::load_from(&path)
    }

    /// Load from an explicit path, returning the empty configuration if absent.
    pub fn load_from(path: &std::path::Path) -> Result<Self, UserConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(UserConfigError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        Self::parse(&text, &path.display().to_string())
    }

    /// Parse configuration text and validate it.
    pub fn parse(text: &str, name: &str) -> Result<Self, UserConfigError> {
        let config: Self = toml::from_str(text).map_err(|error| {
            // The span lets miette underline the offending line rather than
            // making the user count lines from a message.
            let span = error
                .span()
                .map(|s| SourceSpan::from(s.start..s.end))
                .unwrap_or_else(|| SourceSpan::from(0..0));
            UserConfigError::Parse {
                name: name.to_string(),
                message: error.message().to_string(),
                src: NamedSource::new(name, text.to_string()),
                span,
            }
        })?;

        config.validate()?;
        Ok(config)
    }

    /// Expand a leading `<name>:` written on the command line.
    ///
    /// The expanded URL is what gets recorded in `.config/git.tpl.toml` and
    /// what derives the template id, so a shortcut never leaves this machine.
    /// Without that rule, a project created by someone with a `mine:` shortcut
    /// is unusable by everyone else, and `refs/tpl/<id>` differs per
    /// contributor for the same template.
    ///
    /// An unknown prefix is left alone: it may be a real scheme, and this
    /// function cannot know every one of them. Expansion is applied once and
    /// never recursively — a shortcut whose expansion is another shortcut would
    /// make the recorded URL depend on the order of a map.
    pub fn expand<'a>(&self, source: &'a str) -> Cow<'a, str> {
        let Some((name, rest)) = source.split_once(':') else {
            return Cow::Borrowed(source);
        };
        match self.shortcuts.get(name) {
            Some(prefix) => Cow::Owned(format!("{prefix}{rest}")),
            None => Cow::Borrowed(source),
        }
    }

    /// Refuse shortcut names that cannot work.
    ///
    /// Checked when the file is read rather than when a shortcut is used: a
    /// name that shadows `https:` should fail on the day it is written, not on
    /// the day somebody happens to pass an HTTPS URL.
    fn validate(&self) -> Result<(), UserConfigError> {
        for name in self.shortcuts.keys() {
            let reason = if name.is_empty() {
                "a shortcut name cannot be empty"
            } else if name.contains('/') {
                "a shortcut name cannot contain `/`"
            } else if RESERVED.contains(&name.to_ascii_lowercase().as_str()) {
                "that is a URL scheme, and shadowing it would make real URLs unusable"
            } else {
                continue;
            };
            return Err(UserConfigError::Shortcut {
                name: name.clone(),
                reason: reason.to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_parses_to_the_empty_configuration() {
        let config = UserConfig::parse("", "config.toml").unwrap();
        assert_eq!(config, UserConfig::default());
    }

    #[test]
    fn all_three_sections_parse() {
        let config = UserConfig::parse(
            r#"
            [defaults]
            author = "Axel Haustant"
            with_ci = true

            [shortcuts]
            gh = "https://github.com/"

            [trust]
            templates = ["github.com/noirbizarre/*"]
            "#,
            "config.toml",
        )
        .unwrap();

        assert_eq!(
            config.defaults.get("author"),
            Some(&Value::String("Axel Haustant".into()))
        );
        // Types are preserved rather than stringified: a boolean question's
        // seed has to still be a boolean when it reaches the prompt.
        assert_eq!(config.defaults.get("with_ci"), Some(&Value::Bool(true)));
        assert_eq!(
            config.shortcuts.get("gh").map(String::as_str),
            Some("https://github.com/")
        );
        assert_eq!(config.trust.templates, ["github.com/noirbizarre/*"]);
    }

    #[test]
    fn an_unknown_section_is_refused() {
        let error = UserConfig::parse("[defualts]\nauthor = \"x\"\n", "config.toml").unwrap_err();
        assert!(matches!(error, UserConfigError::Parse { .. }));
    }

    #[test]
    fn a_malformed_file_carries_a_span() {
        let error = UserConfig::parse("[shortcuts\n", "config.toml").unwrap_err();
        let UserConfigError::Parse { span, .. } = error else {
            panic!("expected a parse error");
        };
        assert!(!span.is_empty() || span.offset() > 0);
    }

    #[test]
    fn a_shortcut_named_like_a_scheme_is_refused() {
        let error =
            UserConfig::parse("[shortcuts]\nhttps = \"https://x/\"\n", "config.toml").unwrap_err();
        let UserConfigError::Shortcut { name, .. } = error else {
            panic!("expected a shortcut error");
        };
        assert_eq!(name, "https");
    }

    #[test]
    fn a_shortcut_name_containing_a_slash_is_refused() {
        let error = UserConfig::parse("[shortcuts]\n\"a/b\" = \"https://x/\"\n", "config.toml")
            .unwrap_err();
        assert!(matches!(error, UserConfigError::Shortcut { .. }));
    }

    fn shortcuts() -> UserConfig {
        UserConfig::parse(
            r#"
            [shortcuts]
            gh = "https://github.com/"
            ghs = "ssh://git@github.com/"
            mine = "https://github.com/noirbizarre/"
            "#,
            "config.toml",
        )
        .unwrap()
    }

    #[test]
    fn a_known_prefix_expands() {
        assert_eq!(
            shortcuts().expand("gh:org/thing"),
            "https://github.com/org/thing"
        );
        assert_eq!(
            shortcuts().expand("mine:git-tpl"),
            "https://github.com/noirbizarre/git-tpl"
        );
    }

    #[test]
    fn an_unknown_prefix_is_left_alone() {
        // It may be a real scheme, and there is no list of every one of them.
        for source in [
            "https://github.com/org/thing",
            "ssh://git@github.com/org/thing",
            "git@github.com:org/thing.git",
            "../a-local-template",
            "unknown:whatever",
        ] {
            assert_eq!(shortcuts().expand(source), source);
        }
    }

    #[test]
    fn expansion_is_not_recursive() {
        // `ghs` expands to a value beginning `ssh:`, which is a prefix in its
        // own right. Expanding again would make the recorded URL depend on the
        // iteration order of a map.
        assert_eq!(
            shortcuts().expand("ghs:org/thing"),
            "ssh://git@github.com/org/thing"
        );
    }

    #[test]
    fn an_absent_file_is_the_empty_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let config = UserConfig::load_from(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(config, UserConfig::default());
    }
}
