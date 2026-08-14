//! Data sources.
//!
//! Templates declare the data they want; they cannot fetch it themselves.
//! This layer owns resolution, loading, parsing, caching, validation and
//! provenance, and the expression engine only ever consumes the result. There
//! is no `load_file()` or `http_get()` available to a template, and there will
//! not be — see `docs/concepts/determinism.md#security`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miette::Diagnostic;
use thiserror::Error;

use crate::git::{Oid, libgit2::LibGit2};
use crate::template::{DataSourceDecl, Value};

/// Where a data source's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The format a data source is parsed as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// TOML.
    Toml,
    /// JSON.
    Json,
    /// YAML 1.2.
    Yaml,
}

impl Format {
    /// Infer from a path's extension, defaulting to TOML.
    fn infer(source: &str) -> Self {
        let path = source.split(['?', '#']).next().unwrap_or(source);
        match path.rsplit('.').next() {
            Some(e) if e.eq_ignore_ascii_case("json") => Format::Json,
            Some(e) if e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml") => {
                Format::Yaml
            }
            _ => Format::Toml,
        }
    }

    /// Parse an explicit `format`.
    fn parse(format: &str) -> Option<Self> {
        match format {
            "toml" => Some(Format::Toml),
            "json" => Some(Format::Json),
            // Both spellings, because a source whose extension is `.yml` and
            // whose `format` must be written `yaml` is a papercut with no
            // upside.
            "yaml" | "yml" => Some(Format::Yaml),
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
        kind: String,
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

    /// Remote data sources are not implemented yet.
    #[error("data source `{name}` is remote, which is not implemented yet")]
    #[diagnostic(
        code(tpl::data::remote_unsupported),
        url("https://noirbizarre.github.io/git-tpl/data/remote/"),
        help(
            "remote data sources are designed but not implemented in this release. \
             Move the data into the template repository, where it is also pinned by the template revision."
        )
    )]
    RemoteUnsupported {
        /// The declared name.
        name: String,
        /// The URL that was declared.
        location: String,
    },
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
}

impl Provenance {
    /// The trailer value, `<kind>:<location>[@<revision>]`.
    pub fn trailer(&self) -> String {
        match &self.revision {
            Some(oid) => format!("{}:{}@{}", self.kind.label(), self.location, oid.short()),
            None => format!("{}:{}", self.kind.label(), self.location),
        }
    }
}

/// Where a loader reads template files from.
pub struct TemplateTree<'a> {
    /// The repository holding the template.
    pub repo: &'a LibGit2,
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
}

impl<'a> Loader<'a> {
    /// A loader reading template files from `template` and local files from
    /// `project_root`.
    pub fn new(template: TemplateTree<'a>, project_root: impl Into<PathBuf>) -> Self {
        Self {
            template,
            project_root: project_root.into(),
            cache: BTreeMap::new(),
            provenance: Vec::new(),
        }
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
                accepted: Some("expected `toml` or `json`".into()),
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
            // Fetching is designed for but not implemented. Failing loudly with
            // a pointer is better than silently rendering with absent data,
            // which would produce a plausible tree that is wrong — and that
            // tree would become a commit.
            SourceKind::Remote => {
                return Err(DataError::RemoteUnsupported {
                    name: name.to_string(),
                    location: resolved_source.to_string(),
                });
            }
        };

        let value = parse(name, resolved_source, format, &bytes)?;

        self.cache.insert(cache_key, value.clone());
        self.provenance.push(Provenance {
            name: name.to_string(),
            kind: kind.clone(),
            location: resolved_source.to_string(),
            revision: match kind {
                SourceKind::TemplateFile => Some(self.template.revision),
                _ => None,
            },
        });

        Ok(value)
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
                kind: "template".into(),
                reason: e.to_string(),
            })?
            .ok_or_else(|| DataError::Load {
                name: name.to_string(),
                location: path.to_string(),
                kind: "template".into(),
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
            kind: "local".into(),
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

/// Parse bytes into a structured value.
///
/// Types are preserved: a table stays a table, `8080` stays an integer. Nothing
/// is stringified on the way in.
fn parse(name: &str, location: &str, format: Format, bytes: &[u8]) -> Result<Value, DataError> {
    let text = std::str::from_utf8(bytes).map_err(|e| DataError::Parse {
        name: name.to_string(),
        location: location.to_string(),
        reason: format!("not valid UTF-8: {e}"),
    })?;

    match format {
        Format::Toml => toml::from_str::<toml::Value>(text)
            .map(Value::from)
            .map_err(|e| DataError::Parse {
                name: name.to_string(),
                location: location.to_string(),
                reason: e.message().to_string(),
            }),
        Format::Json => serde_json::from_str::<serde_json::Value>(text)
            .map(Value::from)
            .map_err(|e| DataError::Parse {
                name: name.to_string(),
                location: location.to_string(),
                reason: e.to_string(),
            }),
        Format::Yaml => serde_norway::from_str::<serde_norway::Value>(text)
            .map(Value::from)
            .map_err(|e| DataError::Parse {
                name: name.to_string(),
                location: location.to_string(),
                reason: e.to_string(),
            }),
    }
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
        };
        assert_eq!(provenance.trailer(), "local:config/tpl-data.toml");
    }
}
