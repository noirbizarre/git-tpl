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

impl Trust {
    /// Whether a template source matches one of the patterns.
    ///
    /// The source is normalised first, so a single entry covers every way of
    /// writing the same repository:
    ///
    /// ```text
    /// github.com/org/t   matches  https://github.com/org/t
    ///                             git@github.com:org/t.git
    ///                             ssh://git@github.com:22/org/T/
    /// ```
    pub fn allows(&self, source: &str) -> bool {
        let value = normalise(source);
        self.templates
            .iter()
            .any(|pattern| matches(&normalise(pattern), &value))
    }
}

/// Reduce a source to the part that identifies the repository.
///
/// Scheme, userinfo, port, a trailing `.git` and a trailing slash go, and what
/// is left is folded to lower case. An scp-style `host:path` is rewritten
/// `host/path`, so it normalises to the same thing its URL form does.
///
/// Deliberately *not* shared with `refs::normalise`, which exists to slugify a
/// ref name. Coupling them would mean a change to trust matching could change a
/// ref name, and template refs are append-only.
fn normalise(source: &str) -> String {
    // A backslash is a path separator, so a local Windows source is a sequence
    // of segments like every other source rather than one opaque blob that no
    // `*` can ever match. Both the pattern and the value come through here, so
    // `C:\\templates\\rust` and `C:/templates/*` meet in the same shape.
    let source = source.replace('\\', "/");
    let mut rest = source.as_str();

    // Scheme. Matched by `://` rather than against a list, so a scheme this
    // code has never heard of is still stripped.
    if let Some((_, after)) = rest.split_once("://") {
        rest = after;
    }

    // Userinfo. Only within the authority, or a `user@` appearing in a path
    // would take the host with it.
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let mut owned;
    if let Some(at) = rest[..authority_end].rfind('@') {
        owned = rest[at + 1..].to_string();
    } else {
        owned = rest.to_string();
    }

    // A port, or the `:` of an scp-style `host:org/repo`. Both sit between the
    // host and the path, and both must end up as a single `/`.
    let host_end = owned.find('/').unwrap_or(owned.len());
    if let Some(colon) = owned[..host_end].find(':') {
        let tail = owned[colon + 1..].trim_start_matches('/');
        // A numeric port carries no identity; anything else is a path.
        let tail = match tail.split_once('/') {
            Some((first, path)) if first.chars().all(|c| c.is_ascii_digit()) => path,
            _ => tail,
        };
        owned = format!("{}/{}", &owned[..colon], tail);
    }

    owned = owned.trim_end_matches('/').to_string();
    owned = owned.strip_suffix(".git").unwrap_or(&owned).to_string();
    owned.to_lowercase()
}

/// Glob matching over `/`-separated segments.
///
/// `*` matches within one segment, `**` matches across them. No regex and no
/// negation: a trust list that needs debugging is a trust list that will be got
/// wrong, and this one decides whether a fetch happens.
fn matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let value: Vec<&str> = value.split('/').collect();
    segments(&pattern, &value)
}

fn segments(pattern: &[&str], value: &[&str]) -> bool {
    match pattern.first() {
        None => value.is_empty(),
        Some(&"**") => {
            // Every possible span, shortest first. The recursion is bounded by
            // the number of segments in a URL, which is small.
            (0..=value.len()).any(|skip| segments(&pattern[1..], &value[skip..]))
        }
        Some(head) => match value.first() {
            Some(segment) if segment_matches(head, segment) => segments(&pattern[1..], &value[1..]),
            _ => false,
        },
    }
}

/// `*` within a single segment, matching any run of characters but never a `/`.
fn segment_matches(pattern: &str, value: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == value,
        Some((prefix, rest)) => {
            let Some(after) = value.strip_prefix(prefix) else {
                return false;
            };
            // Try every split point, so a pattern with several `*` works
            // without a second pass.
            (0..=after.len())
                .any(|at| after.is_char_boundary(at) && segment_matches(rest, &after[at..]))
        }
    }
}

/// The XDG config directory, if this machine has one.
///
/// `$XDG_CONFIG_HOME` first, then `$HOME/.config`. Hand-written rather than
/// taken from a crate: it is a handful of lines, and a crate would still have
/// to be told which of the rules below to apply.
///
/// One function because there were two, and they disagreed: a relative
/// `XDG_CONFIG_HOME` found a global ignore file but no user configuration, and
/// a Windows user with only `USERPROFILE` got the reverse.
///
/// `None` when nothing is set.
pub fn config_home() -> Option<PathBuf> {
    // An empty or relative value is unset, per the XDG specification.
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME")
        && PathBuf::from(&value).is_absolute()
    {
        return Some(PathBuf::from(value));
    }
    // `USERPROFILE` is the Windows fallback libgit2 itself uses. Git for
    // Windows usually exports `HOME`, but nothing guarantees it, and without
    // this a Windows user's global ignore file would simply not be found —
    // silently including files `git add -A` leaves out.
    let home = ["HOME", "USERPROFILE"]
        .into_iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))?;
    Some(PathBuf::from(home).join(".config"))
}

impl UserConfig {
    /// Where the configuration lives, if this machine has a home directory.
    ///
    /// `None` when there is no home — a container without one is a normal
    /// place to run git-tpl, and it simply has no user configuration.
    pub fn path() -> Option<PathBuf> {
        Some(config_home()?.join(CONFIG_DIR).join(CONFIG_FILE))
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
        // A singular `[shortcut]`, which is the mistake people actually make.
        let error =
            UserConfig::parse("[shortcut]\ngh = \"https://x/\"\n", "config.toml").unwrap_err();
        std::assert_matches!(error, UserConfigError::Parse { .. });
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
        std::assert_matches!(error, UserConfigError::Shortcut { .. });
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

    // --- [trust] ------------------------------------------------------------

    fn trusting(patterns: &[&str]) -> Trust {
        Trust {
            templates: patterns.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[rstest::rstest]
    // One entry covers every way of writing the same repository.
    #[case("https://github.com/org/t")]
    #[case("http://github.com/org/t")]
    #[case("ssh://git@github.com/org/t")]
    #[case("ssh://git@github.com:22/org/t")]
    #[case("git@github.com:org/t.git")]
    #[case("https://github.com/org/t.git")]
    #[case("https://user:token@github.com/org/t/")]
    #[case("https://github.com/ORG/T")]
    fn one_pattern_covers_every_form_of_the_same_url(#[case] source: &str) {
        assert!(
            trusting(&["github.com/org/t"]).allows(source),
            "{source} should have matched"
        );
    }

    #[test]
    fn a_pattern_may_be_written_as_a_url_too() {
        // Both sides are normalised, so pasting the URL you cloned works.
        assert!(trusting(&["https://github.com/org/*"]).allows("git@github.com:org/t.git"));
    }

    #[test]
    fn a_star_stays_within_one_segment() {
        let trust = trusting(&["github.com/org/*"]);
        assert!(trust.allows("https://github.com/org/t"));
        // Otherwise `org/*` would silently cover every repository of every
        // organisation whose name starts with `org`.
        assert!(!trust.allows("https://github.com/org/group/t"));
        assert!(!trust.allows("https://github.com/other/t"));
    }

    #[test]
    fn a_double_star_crosses_segments() {
        let trust = trusting(&["github.com/org/**"]);
        assert!(trust.allows("https://github.com/org/t"));
        assert!(trust.allows("https://github.com/org/group/t"));
        assert!(!trust.allows("https://github.com/other/t"));
    }

    #[test]
    fn a_star_is_not_a_substring_match() {
        // `github.com` must not match `evil-github.com`, and a host that merely
        // ends the right way must not match either.
        let trust = trusting(&["github.com/**"]);
        assert!(!trust.allows("https://evil-github.com/org/t"));
        assert!(!trust.allows("https://github.com.evil.test/org/t"));
    }

    #[test]
    fn an_empty_list_trusts_nothing() {
        assert!(!Trust::default().allows("https://github.com/org/t"));
    }

    #[test]
    fn a_local_path_is_matchable() {
        assert!(trusting(&["**/templates/*"]).allows("/home/someone/templates/rust"));
    }

    #[test]
    fn a_windows_path_is_matchable_too() {
        // A backslash is a separator, not a character inside one enormous
        // segment that no `*` could ever match. The drive letter survives as a
        // segment of its own, which is consistent on both sides.
        let trust = trusting(&["c:/templates/*"]);
        assert!(trust.allows(r"C:\templates\rust"));
        assert!(!trust.allows(r"C:\elsewhere\rust"));
    }

    #[test]
    fn an_absent_file_is_the_empty_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let config = UserConfig::load_from(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(config, UserConfig::default());
    }
}
