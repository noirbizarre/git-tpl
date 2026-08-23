//! Document formats, and the parsers for them.
//!
//! Shared by data sources and by answers files (`--answers-from`) so there is
//! one decision about what YAML means, in one place. A second YAML parser
//! reached by a second code path is how two documents that look identical come
//! to render differently.

use crate::template::Value;

/// The format a document is parsed as.
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
    pub fn infer(source: &str) -> Self {
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
    pub fn parse(format: &str) -> Option<Self> {
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

/// Parse bytes into a structured value.
///
/// Types are preserved: a table stays a table, `8080` stays an integer. Nothing
/// is stringified on the way in.
///
/// Only the reason is returned on failure. Each caller knows what it was
/// reading — a data source has a name, an answers file has a path — and wraps
/// the reason in a diagnostic that says so.
pub fn parse_value(format: Format, bytes: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("not valid UTF-8: {e}"))?;

    match format {
        Format::Toml => toml::from_str::<toml::Value>(text)
            .map(Value::from)
            .map_err(|e| e.message().to_string()),
        Format::Json => serde_json::from_str::<serde_json::Value>(text)
            .map(Value::from)
            .map_err(|e| e.to_string()),
        // `noyalib` rather than a YAML 1.1 parser: `no` is the string "no",
        // not `false`. See `docs/data/index.md#about-yaml`. Its own defaults
        // are looser than that contract, so `yaml_parser_config` tightens
        // them back up before parsing.
        Format::Yaml => {
            noyalib::from_str_with_config::<noyalib::Value>(text, &yaml_parser_config())
                .map(Value::from)
                .map_err(|e| e.to_string())
        }
    }
}

/// The YAML parser configuration a data source is held to.
///
/// Built on [`noyalib::ParserConfig::strict`] — a remote data source is
/// untrusted input, so the tighter depth/alias/document budgets are the
/// right starting point — with three further overrides restoring the exact
/// behavior this project has always required and that `docs/data/index.md`
/// documents:
fn yaml_parser_config() -> noyalib::ParserConfig {
    noyalib::ParserConfig::strict()
        // A duplicate key is ambiguous, not a "last one wins" preference: two
        // documents that look identical except for which value survives is
        // exactly the kind of silent divergence invariant 2 forbids.
        .duplicate_key_policy(noyalib::DuplicateKeyPolicy::Error)
        // YAML 1.2 dropped the merge-key specification; auto-merging `<<`
        // would resolve a key differently than the spec this project claims
        // to follow. Keep it a literal `<<` entry instead.
        .merge_key_policy(noyalib::MergeKeyPolicy::AsOrdinary)
        // A second `---` document is a different question answered in the
        // same file; taking the first and discarding the rest silently would
        // hide a mistake the author never intended.
        .max_documents(1)
}
