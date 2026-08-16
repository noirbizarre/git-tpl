//! Machine-readable output.
//!
//! One envelope for every failure, so a caller can branch on *why* something
//! failed rather than matching prose. The diagnostic codes it carries —
//! `tpl::<area>::<kind>` — are the stable surface; the messages are not, and a
//! consumer that matches on them will break the next time a diagnostic
//! improves, which is exactly the outcome codes exist to prevent.
//!
//! Everything here goes to **stdout**. Human output goes to stderr
//! (`commands::Session::say`), so a piped `--json` stream stays parseable even
//! when the command is chatty.

use miette::Diagnostic;
use serde_json::{Value, json};

/// Render a diagnostic as the failure envelope.
///
/// ```json
/// {"ok": false, "error": {"code": "…", "message": "…", "help": "…", "causes": [...]}}
/// ```
pub fn error<E: Diagnostic + 'static>(error: &E) -> String {
    json!({ "ok": false, "error": diagnostic(error) }).to_string()
}

/// Render a success payload, tagging it so `ok` is present on both branches.
///
/// A consumer should be able to check one field without first knowing which
/// command it ran.
pub fn success(mut payload: Value) -> String {
    if let Value::Object(map) = &mut payload {
        map.insert("ok".into(), Value::Bool(true));
    }
    payload.to_string()
}

/// The `changes` array, as `init` and `update` report it.
///
/// Shared rather than built at each call site so the two commands cannot drift
/// apart on a key name. `ChangeKind::as_str` and never `label`: the latter is
/// padded for column alignment, and a consumer matching `"added   "` would be
/// depending on a presentation decision.
///
/// Renaming a key here is a breaking change.
pub fn changes(changes: &[tpl::git::Change]) -> Value {
    Value::Array(
        changes
            .iter()
            .map(|change| json!({ "path": change.path, "kind": change.kind.as_str() }))
            .collect(),
    )
}

/// The `merge` object, as `init` and `merge` report it.
///
/// Tagged with `result`, so a caller switches on one field rather than probing
/// for which of `commit` or `conflicts` happens to be present.
///
/// Renaming a key or a `result` value here is a breaking change.
pub fn merge(outcome: &tpl::git::MergeOutcome) -> Value {
    use tpl::git::MergeOutcome;
    match outcome {
        MergeOutcome::UpToDate => json!({ "result": "upToDate" }),
        MergeOutcome::FastForward { to } => {
            json!({ "result": "fastForward", "commit": to.to_hex() })
        }
        MergeOutcome::Merged { commit } => json!({ "result": "merged", "commit": commit.to_hex() }),
        MergeOutcome::Staged => json!({ "result": "staged" }),
        // The paths, not a count: "which files conflicted" is the whole reason
        // a caller asked, and it is what it needs to drive a resolution.
        MergeOutcome::Conflicted { paths } => json!({ "result": "conflicted", "conflicts": paths }),
    }
}

/// One diagnostic, with its cause chain.
///
/// The chain is where the actionable detail lives: `RenderError::Content` says
/// only "failed to render `x`", and the `EvalError` beneath it says which
/// expression and why. Flattening the two into one string is how a caller ends
/// up parsing prose.
fn diagnostic(error: &dyn Diagnostic) -> Value {
    let mut out = json!({ "message": error.to_string() });
    let map = out.as_object_mut().expect("object");

    map.insert(
        "code".into(),
        error
            .code()
            .map(|code| Value::String(code.to_string()))
            .unwrap_or(Value::Null),
    );

    if let Some(help) = error.help() {
        map.insert("help".into(), Value::String(help.to_string()));
    }

    // `diagnostic_source` first: it is the *typed* chain and carries codes.
    // `source` is the plain `std::error::Error` chain, which does not, and is
    // only consulted when there is no diagnostic beneath — otherwise a caller
    // would see the same error twice under two names.
    let mut causes = Vec::new();
    if let Some(source) = error.diagnostic_source() {
        causes.push(diagnostic(source));
    } else if let Some(source) = std::error::Error::source(error) {
        causes.push(json!({ "message": source.to_string(), "code": Value::Null }));
    }
    if !causes.is_empty() {
        map.insert("causes".into(), Value::Array(causes));
    }

    let labels: Vec<Value> = error
        .labels()
        .into_iter()
        .flatten()
        .map(|label| {
            json!({
                "offset": label.offset(),
                "length": label.len(),
                "label": label.label(),
            })
        })
        .collect();
    if !labels.is_empty() {
        map.insert("labels".into(), Value::Array(labels));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use miette::Diagnostic;
    use miette::SourceSpan;
    use thiserror::Error;

    #[derive(Debug, Error, Diagnostic)]
    #[error("the inner thing failed")]
    #[diagnostic(code(tpl::test::inner), help("try the other thing"))]
    struct Inner;

    #[derive(Debug, Error, Diagnostic)]
    #[error("the outer thing failed")]
    #[diagnostic(code(tpl::test::outer))]
    struct Outer {
        #[source]
        #[diagnostic_source]
        source: Inner,
    }

    /// A cause that is a plain `std::error::Error`, with no `Diagnostic`.
    ///
    /// The distinction this fixture exists to pin: `Outer` carries a
    /// `#[diagnostic_source]` and so its cause has a code; this one does not,
    /// and its cause must still be reported rather than dropped.
    #[derive(Debug, Error)]
    #[error("the plain thing failed")]
    struct Plain;

    #[derive(Debug, Error, Diagnostic)]
    #[error("the wrapping thing failed")]
    #[diagnostic(code(tpl::test::wrapper))]
    struct Wrapper {
        #[source]
        source: Plain,
    }

    #[derive(Debug, Error, Diagnostic)]
    #[error("the expression is wrong")]
    #[diagnostic(code(tpl::test::spanned))]
    struct Spanned {
        #[source_code]
        source_code: String,
        #[label("the offending name")]
        span: SourceSpan,
    }

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).expect("valid JSON")
    }

    #[test]
    fn a_failure_envelope_carries_the_code() {
        let json = parse(&error(&Inner));
        assert_eq!(json["ok"], Value::Bool(false));
        assert_eq!(json["error"]["code"], "tpl::test::inner");
        assert_eq!(json["error"]["message"], "the inner thing failed");
        assert_eq!(json["error"]["help"], "try the other thing");
    }

    // The whole reason `causes` exists: the outer error names the file, the
    // inner one names the reason, and only the pair is actionable.
    #[test]
    fn the_cause_chain_is_reported_with_its_own_codes() {
        let json = parse(&error(&Outer { source: Inner }));
        assert_eq!(json["error"]["code"], "tpl::test::outer");
        assert_eq!(json["error"]["causes"][0]["code"], "tpl::test::inner");
        assert_eq!(json["error"]["causes"][0]["help"], "try the other thing");
    }

    #[test]
    fn a_diagnostic_without_a_cause_reports_no_chain() {
        let json = parse(&error(&Inner));
        assert!(json["error"].get("causes").is_none());
    }

    // A cause without a code is still worth reporting: it is usually the
    // `io::Error` or `toml::de::Error` that says what actually went wrong,
    // and dropping it leaves the caller with "failed to load `x`" and nothing.
    #[test]
    fn a_plain_error_source_is_reported_without_a_code() {
        let json = parse(&error(&Wrapper { source: Plain }));
        assert_eq!(json["error"]["code"], "tpl::test::wrapper");
        assert_eq!(
            json["error"]["causes"][0]["message"],
            "the plain thing failed"
        );
        assert_eq!(json["error"]["causes"][0]["code"], Value::Null);
    }

    // The span is what points at the character. A caller rendering its own
    // squiggle needs the offsets as numbers, not as the ASCII art miette
    // would have drawn.
    #[test]
    fn a_labelled_span_is_reported_with_its_offset_and_length() {
        let json = parse(&error(&Spanned {
            source_code: "{{ projct_name }}".into(),
            span: (3, 11).into(),
        }));
        assert_eq!(json["error"]["labels"][0]["offset"], 3);
        assert_eq!(json["error"]["labels"][0]["length"], 11);
        assert_eq!(json["error"]["labels"][0]["label"], "the offending name");
    }

    #[test]
    fn a_diagnostic_without_a_label_reports_no_spans() {
        let json = parse(&error(&Inner));
        assert!(json["error"].get("labels").is_none());
    }

    #[test]
    fn a_success_payload_is_tagged_ok() {
        let json = parse(&success(json!({ "files": [] })));
        assert_eq!(json["ok"], Value::Bool(true));
        assert_eq!(json["files"], json!([]));
    }

    // The unpadded name, because `label()` pads for column alignment and a
    // consumer matching `"added   "` would be pinned to a layout decision.
    #[test]
    fn a_change_reports_its_unpadded_kind() {
        let payload = changes(&[
            tpl::git::Change {
                kind: tpl::git::ChangeKind::Added,
                path: "Cargo.toml".into(),
            },
            tpl::git::Change {
                kind: tpl::git::ChangeKind::Deleted,
                path: "old.rs".into(),
            },
        ]);
        assert_eq!(
            payload,
            json!([
                { "path": "Cargo.toml", "kind": "added" },
                { "path": "old.rs", "kind": "deleted" },
            ])
        );
    }

    // One field to switch on. A caller should never have to probe for whether
    // `commit` or `conflicts` happens to be present to learn what happened.
    #[test]
    fn every_merge_outcome_is_tagged_with_a_result() {
        use tpl::git::MergeOutcome;
        assert_eq!(merge(&MergeOutcome::UpToDate)["result"], "upToDate");
        assert_eq!(merge(&MergeOutcome::Staged)["result"], "staged");

        let conflicted = merge(&MergeOutcome::Conflicted {
            paths: vec!["mise.toml".into()],
        });
        assert_eq!(conflicted["result"], "conflicted");
        assert_eq!(conflicted["conflicts"], json!(["mise.toml"]));
    }

    // The full hex, not the seven characters the prose abbreviates to: a
    // caller that has to `git cat-file` the commit needs an id Git will take.
    #[test]
    fn a_merge_that_moved_the_branch_reports_the_full_commit_id() {
        use tpl::git::{MergeOutcome, Oid};
        let oid = Oid::from_bytes([0xab; 20]);

        let fast_forward = merge(&MergeOutcome::FastForward { to: oid });
        assert_eq!(fast_forward["result"], "fastForward");
        assert_eq!(fast_forward["commit"], oid.to_hex());

        let merged = merge(&MergeOutcome::Merged { commit: oid });
        assert_eq!(merged["result"], "merged");
        assert_eq!(merged["commit"], oid.to_hex());
    }
}
