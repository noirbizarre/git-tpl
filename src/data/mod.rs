//! Data sources.
//!
//! Templates declare the data they want; they cannot fetch it themselves.
//! This layer owns resolution, loading, parsing, caching, validation and
//! provenance, and the expression engine only ever consumes the result. There
//! is no `load_file()` or `http_get()` available to a template, and there will
//! not be — see `docs/concepts/determinism.md#security`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use miette::Diagnostic;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub mod format;

pub use format::Format;

use crate::git::{GitBackend, Oid};
use crate::template::{DataSourceDecl, Value};

/// The most a remote response may be, in bytes.
///
/// Enforced while reading the body, never taken from `Content-Length` — that
/// header is a claim made by the party whose input is being bounded.
pub const REMOTE_LIMIT_BYTES: u64 = 5 * 1024 * 1024;

/// How long a remote source has to produce its whole response.
///
/// Global rather than per-read: a server dribbling one byte at a time would
/// satisfy any read timeout indefinitely.
const REMOTE_TIMEOUT: Duration = Duration::from_secs(30);

/// How many redirects are followed before the fetch is abandoned.
const REMOTE_MAX_REDIRECTS: u32 = 5;

/// Where a data source's bytes come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A path in the template repository, read from its Git tree.
    ///
    /// The common case, and the only one pinned by the template revision.
    TemplateFile,
    /// A path in the project being rendered.
    LocalFile,
    /// An `http(s)` URL.
    Remote,
}

impl fmt::Display for SourceKind {
    /// The same spelling as the provenance trailer, so an error and a trailer
    /// describing one source never disagree about what it is.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl SourceKind {
    /// The label used in provenance trailers.
    pub fn label(&self) -> &'static str {
        match self {
            SourceKind::TemplateFile => "template",
            SourceKind::LocalFile => "local",
            SourceKind::Remote => "remote",
        }
    }

    /// Infer the kind from a resolved source string.
    pub fn infer(source: &str) -> Self {
        if source.starts_with("http://") || source.starts_with("https://") {
            SourceKind::Remote
        } else if source.starts_with("./") || source.starts_with("../") {
            SourceKind::LocalFile
        } else {
            SourceKind::TemplateFile
        }
    }

    /// Parse an explicit `kind`.
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "template" => Some(SourceKind::TemplateFile),
            "local" => Some(SourceKind::LocalFile),
            "remote" => Some(SourceKind::Remote),
            _ => None,
        }
    }
}

/// Errors from loading data.
#[derive(Debug, Error, Diagnostic)]
pub enum DataError {
    /// The source could not be read.
    //
    // The location and the reason go in the `help`, not just the fields: a
    // diagnostic that says only "could not load `things`" names the thing the
    // user already knows and withholds the two facts they need.
    #[error("could not load template data source `{name}`")]
    #[diagnostic(
        code(tpl::data::load),
        help("source: {location}\nkind:   {kind}\nreason: {reason}")
    )]
    Load {
        /// The declared name.
        name: String,
        /// The resolved source.
        location: String,
        /// Which kind of source it is.
        kind: SourceKind,
        /// Why it failed.
        reason: String,
    },

    /// The source could not be parsed.
    #[error("could not parse template data source `{name}`")]
    #[diagnostic(code(tpl::data::parse), help("source: {location}\nreason: {reason}"))]
    Parse {
        /// The declared name.
        name: String,
        /// The resolved source.
        location: String,
        /// The parser's message.
        reason: String,
    },

    /// A local path tried to escape the project root.
    #[error("data source `{name}` points outside the project")]
    #[diagnostic(
        code(tpl::data::escapes_root),
        help("`{location}` leaves the project root. A local data path must stay within it.")
    )]
    EscapesRoot {
        /// The declared name.
        name: String,
        /// The offending path.
        location: String,
    },

    /// The declared `kind` or `format` is not one we know.
    #[error("data source `{name}` declares an unknown {what} `{value}`")]
    #[diagnostic(code(tpl::data::unknown_setting))]
    UnknownSetting {
        /// The declared name.
        name: String,
        /// `kind` or `format`.
        what: &'static str,
        /// What was declared.
        value: String,
        /// What is accepted.
        #[help]
        accepted: Option<String>,
    },

    /// A remote fetch was not confirmed.
    ///
    /// Deliberately an error rather than an empty value: a CI runner is the
    /// worst possible place to grant a capability by omission, and a render
    /// that quietly proceeded without the data would produce a plausible tree
    /// that is wrong — and that tree becomes a commit.
    #[error("data source `{name}` was not fetched, because the template is not trusted")]
    #[diagnostic(
        code(tpl::data::untrusted),
        url("https://noirbizarre.github.io/git-tpl/data/remote/"),
        help(
            "source: {location}\npass `--trust` to allow this template's remote data sources for this run, or answer the confirmation interactively"
        )
    )]
    Untrusted {
        /// The declared name.
        name: String,
        /// The URL that would have been fetched.
        location: String,
    },

    /// A source became a URL only after interpolation.
    ///
    /// The trust confirmation lists every remote source before any of them is
    /// fetched, and it can only do that from the declaration. A source whose
    /// URL appears after an answer is substituted would slip past the list, so
    /// it is refused rather than fetched unannounced.
    #[error("data source `{name}` resolved to a URL but is not declared remote")]
    #[diagnostic(
        code(tpl::data::undeclared_remote),
        help(
            "resolved: {location}\nadd `kind = \"remote\"` to `[data.{name}]` so the fetch can be confirmed before it happens"
        )
    )]
    UndeclaredRemote {
        /// The declared name.
        name: String,
        /// What the source interpolated to.
        location: String,
    },

    /// The user declined to decide, at the confirmation prompt.
    #[error("cancelled")]
    #[diagnostic(code(tpl::data::cancelled))]
    Cancelled,

    /// The content does not match the declared `sha256`.
    #[error("data source `{name}` does not match its recorded checksum")]
    #[diagnostic(
        code(tpl::data::checksum),
        help(
            "source:   {location}\nexpected: {expected}\nactual:   {actual}\nthe content changed, or is not what the template pinned"
        )
    )]
    ChecksumMismatch {
        /// The declared name.
        name: String,
        /// The resolved source.
        location: String,
        /// The declared digest.
        expected: String,
        /// What was actually received.
        actual: String,
    },
}

/// A remote source a template wants to fetch, as it was declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRequest {
    /// The declared name.
    pub name: String,
    /// The source string, before interpolation.
    pub source: String,
}

/// What to do about one remote source. Per invocation; nothing is remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Fetch it.
    Allow,
    /// Do not fetch it, and fail when it is needed.
    Skip,
}

/// Decides whether a template's remote data sources may be fetched.
///
/// Called **once, before evaluation**, with every declared remote source.
/// Loading is lazy and interleaved with the questionnaire, so confirming at
/// fetch time would scatter network prompts through the questions; the rule is
/// that everything is shown in full before any of it happens.
pub trait TrustGate {
    /// Decide for each request. `limit_bytes` is shown to the user, because a
    /// size bound they cannot see is not a bound they can consent to.
    fn confirm(
        &mut self,
        requests: &[RemoteRequest],
        limit_bytes: u64,
    ) -> Result<BTreeMap<String, Decision>, DataError>;
}

/// Allows every remote source without asking. `--trust`.
///
/// Per invocation. It writes nothing anywhere, and the next run asks again.
pub struct AlwaysTrust;

impl TrustGate for AlwaysTrust {
    fn confirm(
        &mut self,
        requests: &[RemoteRequest],
        _limit_bytes: u64,
    ) -> Result<BTreeMap<String, Decision>, DataError> {
        Ok(requests
            .iter()
            .map(|r| (r.name.clone(), Decision::Allow))
            .collect())
    }
}

/// Refuses every remote source, for when there is nobody to ask.
///
/// `--defaults`, `tpl.interactive false`, CI. The refusal is loud at the point
/// of use — see [`DataError::Untrusted`] — rather than a silent omission.
pub struct RefuseRemote;

impl TrustGate for RefuseRemote {
    fn confirm(
        &mut self,
        requests: &[RemoteRequest],
        _limit_bytes: u64,
    ) -> Result<BTreeMap<String, Decision>, DataError> {
        Ok(requests
            .iter()
            .map(|r| (r.name.clone(), Decision::Skip))
            .collect())
    }
}

/// Every data source that is declared remote, in declaration order.
///
/// From the *declaration*, not from a resolved string: this is what the trust
/// confirmation lists, and it has to be computable before anything is
/// evaluated. A source that only becomes a URL after interpolation is refused
/// at load time instead — see [`DataError::UndeclaredRemote`].
pub fn declared_remotes(data: &BTreeMap<String, DataSourceDecl>) -> Vec<RemoteRequest> {
    data.iter()
        .filter(|(_, decl)| match &decl.kind {
            Some(explicit) => explicit == "remote",
            None => SourceKind::infer(&decl.source) == SourceKind::Remote,
        })
        .map(|(name, decl)| RemoteRequest {
            name: name.clone(),
            source: decl.source.clone(),
        })
        .collect()
}

/// Where a loaded value came from, recorded in the rendered commit's trailers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The declared name.
    pub name: String,
    /// Which kind of source it was.
    pub kind: SourceKind,
    /// The resolved source string.
    pub location: String,
    /// The template commit it was read at, for template files.
    ///
    /// Template data is pinned by the template revision, so recording the
    /// commit is what makes "which data produced this tree?" answerable from
    /// Git alone.
    pub revision: Option<Oid>,
    /// The sha256 of the bytes, for remote sources.
    ///
    /// Recorded whether or not the template pinned one. Provenance exists to
    /// answer "which bytes produced this tree", and computing the digest only
    /// when a pin was declared would answer it precisely for the sources that
    /// needed it least.
    pub checksum: Option<String>,
}

impl Provenance {
    /// The trailer value, `<kind>:<location>[@<revision>|@sha256:<digest>]`.
    ///
    /// The `sha256:` prefix is what keeps a digest distinguishable from a short
    /// oid, so a reader — and the existing parser — never has to guess.
    pub fn trailer(&self) -> String {
        match (&self.revision, &self.checksum) {
            (Some(oid), _) => format!("{}:{}@{}", self.kind.label(), self.location, oid.short()),
            (None, Some(digest)) => {
                format!("{}:{}@sha256:{}", self.kind.label(), self.location, digest)
            }
            (None, None) => format!("{}:{}", self.kind.label(), self.location),
        }
    }
}

/// Where a loader reads template files from.
pub struct TemplateTree<'a> {
    /// The repository holding the template.
    ///
    /// Held as a trait object rather than a concrete backend: this is a data
    /// carrier, not a hot path, and naming the implementation here is how the
    /// abstraction stopped being load-bearing above `src/git/` before.
    pub repo: &'a dyn GitBackend,
    /// The tree of the resolved template revision.
    pub tree: Oid,
    /// The commit that tree came from, for provenance.
    pub revision: Oid,
}

/// Loads and caches data sources.
///
/// Caching is keyed by the *resolved* source string, so several questions
/// drawing on one source cause one read. A declared source that nothing
/// references is never loaded at all, which is what lets a template offer
/// data-backed choices on a conditional branch without imposing the cost on
/// everyone.
pub struct Loader<'a> {
    template: TemplateTree<'a>,
    project_root: PathBuf,
    cache: BTreeMap<String, Value>,
    provenance: Vec<Provenance>,
    decisions: BTreeMap<String, Decision>,
    // Built on the first fetch and reused, so a template with several remote
    // sources opens one connection pool rather than one per source. `None` for
    // the overwhelmingly common template that has no remote data at all.
    agent: Option<ureq::Agent>,
}

impl<'a> Loader<'a> {
    /// A loader reading template files from `template` and local files from
    /// `project_root`.
    ///
    /// No remote source is permitted until [`with_decisions`](Self::with_decisions)
    /// says so. Defaulting to "allowed" would mean every future caller had to
    /// remember to close the gate.
    pub fn new(template: TemplateTree<'a>, project_root: impl Into<PathBuf>) -> Self {
        Self {
            template,
            project_root: project_root.into(),
            cache: BTreeMap::new(),
            provenance: Vec::new(),
            decisions: BTreeMap::new(),
            agent: None,
        }
    }

    /// Record what the trust gate decided, by source name.
    pub fn with_decisions(mut self, decisions: BTreeMap<String, Decision>) -> Self {
        self.decisions = decisions;
        self
    }

    /// What contributed to this run, in load order.
    pub fn provenance(&self) -> &[Provenance] {
        &self.provenance
    }

    /// Load a declared source whose `source` has already been rendered.
    pub fn load(
        &mut self,
        name: &str,
        decl: &DataSourceDecl,
        resolved_source: &str,
    ) -> Result<Value, DataError> {
        let kind = match &decl.kind {
            Some(explicit) => {
                SourceKind::parse(explicit).ok_or_else(|| DataError::UnknownSetting {
                    name: name.to_string(),
                    what: "kind",
                    value: explicit.clone(),
                    accepted: Some("expected `template`, `local` or `remote`".into()),
                })?
            }
            None => SourceKind::infer(resolved_source),
        };

        let format = match &decl.format {
            Some(explicit) => Format::parse(explicit).ok_or_else(|| DataError::UnknownSetting {
                name: name.to_string(),
                what: "format",
                value: explicit.clone(),
                accepted: Some("expected `toml`, `json` or `yaml`".into()),
            })?,
            None => Format::infer(resolved_source),
        };

        // The cache key includes the kind, because `data/x.toml` means
        // different files depending on whether it is a template or local path.
        let cache_key = format!("{}:{resolved_source}", kind.label());
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        let bytes = match kind {
            SourceKind::TemplateFile => self.read_template_file(name, resolved_source)?,
            SourceKind::LocalFile => self.read_local_file(name, resolved_source)?,
            SourceKind::Remote => self.fetch(name, resolved_source)?,
        };

        // Computed for every remote source, pinned or not, because the digest
        // is the only thing that makes a remote trailer reproducible. Skipped
        // for the other kinds unless a pin asked for it: a template file is
        // already pinned by the template revision.
        let expected = expected_digest(name, decl)?;
        let digest = (expected.is_some() || kind == SourceKind::Remote).then(|| digest_of(&bytes));

        if let (Some(expected), Some(actual)) = (&expected, &digest)
            && expected != actual
        {
            return Err(DataError::ChecksumMismatch {
                name: name.to_string(),
                location: resolved_source.to_string(),
                expected: expected.clone(),
                actual: actual.clone(),
            });
        }

        let value = parse(name, resolved_source, format, &bytes)?;

        self.cache.insert(cache_key, value.clone());
        self.provenance.push(Provenance {
            name: name.to_string(),
            kind,
            location: resolved_source.to_string(),
            revision: match kind {
                SourceKind::TemplateFile => Some(self.template.revision),
                _ => None,
            },
            checksum: match kind {
                SourceKind::Remote => digest,
                _ => None,
            },
        });

        Ok(value)
    }

    /// Fetch a remote source over HTTP.
    ///
    /// The response is untrusted input from a third party: it is bounded, timed
    /// out, and parsed defensively. Nothing about it can cause execution, and
    /// there is no fallback — a failure stops the render rather than
    /// substituting a cached copy, an empty table, or the last known value.
    fn fetch(&mut self, name: &str, url: &str) -> Result<Vec<u8>, DataError> {
        // The gate is consulted by name, and a source absent from it was never
        // shown to the user — which for a URL produced by interpolation is the
        // whole point of refusing it.
        match self.decisions.get(name) {
            Some(Decision::Allow) => {}
            Some(Decision::Skip) => {
                return Err(DataError::Untrusted {
                    name: name.to_string(),
                    location: url.to_string(),
                });
            }
            None => {
                return Err(DataError::UndeclaredRemote {
                    name: name.to_string(),
                    location: url.to_string(),
                });
            }
        }

        // `kind = "remote"` can be declared on any string, so the scheme is
        // checked here rather than relying on the inference that a declared
        // kind bypasses.
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(DataError::Load {
                name: name.to_string(),
                location: url.to_string(),
                kind: SourceKind::Remote,
                reason: "unsupported scheme: a remote data source must be http or https".into(),
            });
        }

        let agent = self.agent.get_or_insert_with(|| {
            ureq::Agent::config_builder()
                .timeout_global(Some(REMOTE_TIMEOUT))
                .max_redirects(REMOTE_MAX_REDIRECTS)
                .user_agent(concat!("git-tpl/", env!("CARGO_PKG_VERSION")))
                .build()
                .into()
        });

        // `http_status_as_error` is left on: a 404 body is an error page, and
        // parsing it as TOML would report a syntax error instead of the status
        // the user needs to see.
        let mut response = agent.get(url).call().map_err(|e| DataError::Load {
            name: name.to_string(),
            location: url.to_string(),
            kind: SourceKind::Remote,
            reason: e.to_string(),
        })?;

        response
            .body_mut()
            .with_config()
            .limit(REMOTE_LIMIT_BYTES)
            .read_to_vec()
            .map_err(|e| DataError::Load {
                name: name.to_string(),
                location: url.to_string(),
                kind: SourceKind::Remote,
                reason: match e {
                    ureq::Error::BodyExceedsLimit(limit) => {
                        format!("the response is larger than the {limit} byte limit")
                    }
                    other => other.to_string(),
                },
            })
    }

    /// Read a file from the template repository at the resolved revision.
    ///
    /// From the Git tree, not from a checkout: that is what makes the template
    /// repository a self-contained, pinned data source, with no way for a
    /// template's files and its data to drift apart.
    fn read_template_file(&self, name: &str, path: &str) -> Result<Vec<u8>, DataError> {
        let normalised = path.trim_start_matches("./");
        self.template
            .repo
            .read_path(self.template.tree, normalised)
            .map_err(|e| DataError::Load {
                name: name.to_string(),
                location: path.to_string(),
                kind: SourceKind::TemplateFile,
                reason: e.to_string(),
            })?
            .ok_or_else(|| DataError::Load {
                name: name.to_string(),
                location: path.to_string(),
                kind: SourceKind::TemplateFile,
                reason: format!(
                    "no such file in the template repository at revision {}",
                    self.template.revision.short()
                ),
            })
    }

    /// Read a file from the project.
    fn read_local_file(&self, name: &str, path: &str) -> Result<Vec<u8>, DataError> {
        let candidate = self.project_root.join(path);

        // Reject traversal rather than resolving it. `../../../etc/passwd` in a
        // template repository is untrusted input asking to read a file outside
        // the project.
        if !within(&self.project_root, &candidate) {
            return Err(DataError::EscapesRoot {
                name: name.to_string(),
                location: path.to_string(),
            });
        }

        std::fs::read(&candidate).map_err(|e| DataError::Load {
            name: name.to_string(),
            location: path.to_string(),
            kind: SourceKind::LocalFile,
            reason: e.to_string(),
        })
    }
}

/// Whether `candidate` stays within `root` once `..` segments are folded.
///
/// Lexical rather than `canonicalize`, because the path need not exist yet and
/// `canonicalize` would also follow symlinks — which is a different question
/// than the one being asked.
fn within(root: &Path, candidate: &Path) -> bool {
    let mut depth: i32 = 0;
    for component in candidate
        .strip_prefix(root)
        .unwrap_or(candidate)
        .components()
    {
        match component {
            std::path::Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return false,
            std::path::Component::CurDir => {}
        }
    }
    true
}

/// The lowercase hex sha256 of some bytes.
fn digest_of(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The declared `sha256`, validated.
///
/// Checked for shape when it is read rather than when it is compared, so a
/// typo'd pin is reported as a typo instead of as a mismatch against a digest
/// it could never have equalled.
fn expected_digest(name: &str, decl: &DataSourceDecl) -> Result<Option<String>, DataError> {
    let Some(declared) = &decl.sha256 else {
        return Ok(None);
    };

    let normalised = declared.trim().to_ascii_lowercase();
    if normalised.len() != 64 || !normalised.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(DataError::UnknownSetting {
            name: name.to_string(),
            what: "sha256",
            value: declared.clone(),
            accepted: Some("expected 64 hexadecimal characters".into()),
        });
    }

    Ok(Some(normalised))
}

/// Parse bytes into a structured value, naming the source that failed.
///
/// The formats themselves live in `data::format`, shared with answers files so
/// that YAML means one thing in this project rather than two.
fn parse(name: &str, location: &str, format: Format, bytes: &[u8]) -> Result<Value, DataError> {
    format::parse_value(format, bytes).map_err(|reason| DataError::Parse {
        name: name.to_string(),
        location: location.to_string(),
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("data/licenses.toml", SourceKind::TemplateFile)]
    #[case("licenses.toml", SourceKind::TemplateFile)]
    #[case("./project-data.toml", SourceKind::LocalFile)]
    #[case("../shared.toml", SourceKind::LocalFile)]
    #[case("https://example.com/licenses.toml", SourceKind::Remote)]
    #[case("http://example.com/licenses.toml", SourceKind::Remote)]
    fn the_kind_is_inferred_from_the_source(#[case] source: &str, #[case] expected: SourceKind) {
        assert_eq!(SourceKind::infer(source), expected);
    }

    #[rstest]
    #[case("data/licenses.toml", Format::Toml)]
    #[case("data/registry.json", Format::Json)]
    #[case("data/REGISTRY.JSON", Format::Json)]
    #[case("data/teams.yaml", Format::Yaml)]
    #[case("data/teams.yml", Format::Yaml)]
    #[case("data/TEAMS.YML", Format::Yaml)]
    #[case("https://example.com/registry", Format::Toml)]
    fn the_format_is_inferred_from_the_extension(#[case] source: &str, #[case] expected: Format) {
        assert_eq!(Format::infer(source), expected);
    }

    /// A choice list feeding `choices_from` must be an array of scalars, and a
    /// port number must stay a number, or a template's `{% if %}` breaks.
    #[test]
    fn parsing_preserves_types() {
        let value = parse(
            "ci",
            "data/ci.toml",
            Format::Toml,
            br#"
            [versions]
            rust = ["1.88", "stable"]
            timeout = 30
            strict = true
            "#,
        )
        .unwrap();

        assert_eq!(
            value.get_path("versions.timeout"),
            Some(&Value::Integer(30))
        );
        assert_eq!(value.get_path("versions.strict"), Some(&Value::Bool(true)));
        assert!(matches!(
            value.get_path("versions.rust"),
            Some(Value::Array(_))
        ));
    }

    #[test]
    fn json_parses_to_the_same_value_shape_as_toml() {
        let from_json = parse(
            "x",
            "x.json",
            Format::Json,
            br#"{"versions": {"timeout": 30, "strict": true}}"#,
        )
        .unwrap();
        let from_toml = parse(
            "x",
            "x.toml",
            Format::Toml,
            b"[versions]\ntimeout = 30\nstrict = true\n",
        )
        .unwrap();

        assert_eq!(from_json, from_toml);
    }

    #[test]
    fn yaml_parses_to_the_same_value_shape_as_toml() {
        let from_yaml = parse(
            "x",
            "x.yaml",
            Format::Yaml,
            b"versions:\n  timeout: 30\n  strict: true\n",
        )
        .unwrap();
        let from_toml = parse(
            "x",
            "x.toml",
            Format::Toml,
            b"[versions]\ntimeout = 30\nstrict = true\n",
        )
        .unwrap();

        assert_eq!(from_yaml, from_toml);
    }

    /// The reason YAML is acceptable at all, and the reason the parser is
    /// pinned to a 1.2 implementation. Under YAML 1.1 every one of these
    /// resolves to something else — `no` to false, `12:30:00` to 45000 — which
    /// would silently change a rendered tree. If this test ever fails, the
    /// dependency has regressed to 1.1 and YAML support should be withdrawn
    /// rather than patched around.
    #[rstest]
    #[case(b"country: no\n", "country", Value::String("no".into()))]
    #[case(b"country: NO\n", "country", Value::String("NO".into()))]
    #[case(b"answer: yes\n", "answer", Value::String("yes".into()))]
    #[case(b"toggle: on\n", "toggle", Value::String("on".into()))]
    #[case(b"at: 12:30:00\n", "at", Value::String("12:30:00".into()))]
    #[case(b"mode: 0755\n", "mode", Value::String("0755".into()))]
    #[case(b"real: true\n", "real", Value::Bool(true))]
    fn yaml_uses_the_1_2_scalar_rules(
        #[case] input: &[u8],
        #[case] key: &str,
        #[case] expected: Value,
    ) {
        let parsed = parse("x", "x.yaml", Format::Yaml, input).unwrap();
        let Value::Table(table) = parsed else {
            panic!("expected a table, got {parsed:?}");
        };
        assert_eq!(table.get(key), Some(&expected));
    }

    /// Anchors are expanded, but `<<` is an ordinary key: the merge key is a
    /// separate specification that YAML 1.2 dropped. A template author who
    /// expects `d.x` here gets `d['<<'].x`, so it is worth failing loudly in a
    /// test rather than in someone's rendered file.
    #[test]
    fn a_yaml_alias_is_expanded_but_a_merge_key_is_not_merged() {
        let parsed = parse(
            "x",
            "x.yaml",
            Format::Yaml,
            b"base: &b\n  x: 1\nuse: *b\nd:\n  <<: *b\n  y: 2\n",
        )
        .unwrap();
        let Value::Table(table) = parsed else {
            panic!("expected a table");
        };

        assert_eq!(
            table.get("use"),
            Some(&Value::Table(BTreeMap::from([(
                "x".to_string(),
                Value::Integer(1)
            )])))
        );
        let Some(Value::Table(d)) = table.get("d") else {
            panic!("expected `d` to be a table");
        };
        assert!(d.contains_key("<<"), "the merge key stays a literal key");
        assert!(!d.contains_key("x"), "and is not merged into the mapping");
    }

    /// A data source is untrusted input, and these are the three ways a YAML
    /// document turns that into a problem: ambiguity, unbounded expansion, and
    /// a tag asking to construct something. All three must fail or defuse
    /// rather than surprise.
    #[rstest]
    #[case::duplicate_keys(b"a: 1\na: 2\n".to_vec())]
    #[case::more_than_one_document(b"a: 1\n---\nb: 2\n".to_vec())]
    #[case::billion_laughs(billion_laughs())]
    fn a_hostile_yaml_document_is_refused(#[case] input: Vec<u8>) {
        assert!(parse("x", "x.yaml", Format::Yaml, &input).is_err());
    }

    /// A tag is not an instruction. `!!python/object:os.system` is the classic
    /// YAML deserialisation exploit, and here it is inert: the tag is dropped
    /// and the scalar kept, because git-tpl constructs nothing from data.
    #[test]
    fn a_yaml_tag_is_inert() {
        let parsed = parse(
            "x",
            "x.yaml",
            Format::Yaml,
            b"a: !!python/object:os.system 'ls'\n",
        )
        .unwrap();
        let Value::Table(table) = parsed else {
            panic!("expected a table");
        };
        assert_eq!(table.get("a"), Some(&Value::String("ls".into())));
    }

    fn billion_laughs() -> Vec<u8> {
        let mut yaml = String::from("a: &a [x, x, x, x, x, x, x, x, x]\n");
        for i in 0..8u8 {
            let (this, prev) = ((b'b' + i) as char, (b'a' + i) as char);
            let refs = std::iter::repeat_n(format!("*{prev}"), 9)
                .collect::<Vec<_>>()
                .join(", ");
            yaml.push_str(&format!("{this}: &{this} [{refs}]\n"));
        }
        yaml.into_bytes()
    }

    #[test]
    fn malformed_data_is_reported_with_the_source_that_failed() {
        let error = parse(
            "licenses",
            "data/licenses.toml",
            Format::Toml,
            b"not = = toml",
        )
        .unwrap_err();

        match error {
            DataError::Parse { name, location, .. } => {
                assert_eq!(name, "licenses");
                assert_eq!(location, "data/licenses.toml");
            }
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[rstest]
    #[case("data/x.toml", true)]
    #[case("./nested/x.toml", true)]
    #[case("a/../b/x.toml", true)]
    #[case("../outside.toml", false)]
    #[case("a/../../outside.toml", false)]
    #[case("/etc/passwd", false)]
    fn traversal_out_of_the_project_is_rejected(#[case] path: &str, #[case] allowed: bool) {
        let root = Path::new("/project");
        assert_eq!(within(root, &root.join(path)), allowed, "for `{path}`");
    }

    /// A trailer must identify the data precisely enough to reproduce it, which
    /// for template files means the commit it was read at.
    #[test]
    fn a_template_file_trailer_records_the_revision() {
        let oid = Oid::parse("4f2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29").unwrap();
        let provenance = Provenance {
            name: "licenses".into(),
            kind: SourceKind::TemplateFile,
            location: "data/licenses.toml".into(),
            revision: Some(oid),
            checksum: None,
        };
        assert_eq!(provenance.trailer(), "template:data/licenses.toml@4f2c1a9");
    }

    /// A project file's own commit is the containing commit, so recording it
    /// would be circular.
    #[test]
    fn a_local_file_trailer_records_only_the_path() {
        let provenance = Provenance {
            name: "overrides".into(),
            kind: SourceKind::LocalFile,
            location: "config/tpl-data.toml".into(),
            revision: None,
            checksum: None,
        };
        assert_eq!(provenance.trailer(), "local:config/tpl-data.toml");
    }

    /// Nothing pins a remote source except the bytes it returned, so the
    /// trailer has to carry the digest for the record to mean anything. The
    /// `sha256:` prefix is what keeps it from being read as a short oid.
    #[test]
    fn a_remote_trailer_records_the_digest() {
        let provenance = Provenance {
            name: "licenses".into(),
            kind: SourceKind::Remote,
            location: "https://example.com/licenses.json".into(),
            revision: None,
            checksum: Some("a".repeat(64)),
        };
        assert_eq!(
            provenance.trailer(),
            format!(
                "remote:https://example.com/licenses.json@sha256:{}",
                "a".repeat(64)
            )
        );
    }

    fn decl(sha256: Option<String>) -> DataSourceDecl {
        DataSourceDecl {
            source: "https://example.com/x.json".into(),
            kind: Some("remote".into()),
            format: None,
            sha256,
        }
    }

    /// A malformed pin is reported as a malformed pin. Comparing it and
    /// reporting a mismatch would send the author looking at the server.
    #[rstest]
    #[case::too_short(Some("abc123".to_string()), false)]
    #[case::not_hex(Some("z".repeat(64)), false)]
    #[case::uppercase_is_accepted(Some("A".repeat(64)), true)]
    #[case::well_formed(Some("a".repeat(64)), true)]
    #[case::absent(None, true)]
    fn a_declared_sha256_must_be_hex(#[case] declared: Option<String>, #[case] valid: bool) {
        assert_eq!(expected_digest("x", &decl(declared)).is_ok(), valid);
    }

    /// Case is not part of the value, so a pin copied from a tool that emits
    /// uppercase still matches.
    #[test]
    fn a_declared_sha256_is_compared_in_lowercase() {
        let digest = expected_digest("x", &decl(Some("AB".repeat(32))))
            .unwrap()
            .unwrap();
        assert_eq!(digest, "ab".repeat(32));
    }

    #[test]
    fn the_digest_is_the_sha256_of_the_raw_bytes() {
        // The empty string's sha256, which is worth pinning literally: a
        // regression that hashed something else would still look plausible.
        assert_eq!(
            digest_of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// The confirmation prompt is built from declarations, before anything is
    /// evaluated, so this is what decides whether a source can be fetched at
    /// all.
    #[test]
    fn declared_remotes_finds_urls_and_explicit_kinds() {
        let mut data = BTreeMap::new();
        data.insert(
            "by_url".to_string(),
            DataSourceDecl {
                source: "https://example.com/a.json".into(),
                kind: None,
                format: None,
                sha256: None,
            },
        );
        data.insert(
            "by_kind".to_string(),
            DataSourceDecl {
                source: "{{ registry }}/b.json".into(),
                kind: Some("remote".into()),
                format: None,
                sha256: None,
            },
        );
        data.insert(
            "local".to_string(),
            DataSourceDecl {
                source: "data/c.toml".into(),
                kind: None,
                format: None,
                sha256: None,
            },
        );

        let names: Vec<_> = declared_remotes(&data)
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, vec!["by_kind", "by_url"]);
    }

    /// An interpolated source that is not declared remote cannot appear in the
    /// confirmation list, so it must not be fetchable either — otherwise the
    /// list is not the whole truth.
    #[test]
    fn an_interpolated_url_is_not_a_declared_remote() {
        let mut data = BTreeMap::new();
        data.insert(
            "sneaky".to_string(),
            DataSourceDecl {
                source: "{{ base }}/licenses.json".into(),
                kind: None,
                format: None,
                sha256: None,
            },
        );
        assert!(declared_remotes(&data).is_empty());
    }
}
