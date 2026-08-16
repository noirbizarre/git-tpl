//! The diagnostic codes are a public surface.
//!
//! `--json` reports `error.code`, and callers branch on it. That makes the set
//! something we promise rather than something that happens, so this pins it:
//! adding a code without documenting it fails here, and so does renaming one
//! without noticing that it is a breaking change.
//!
//! Messages are deliberately *not* pinned anywhere. Pinning prose is how error
//! messages stop improving.

use std::collections::BTreeSet;
use std::path::Path;

/// Every code the crate defines, gathered from the source.
///
/// Read from the tree rather than a registry, because a registry is a second
/// place to forget. Two shapes count, and nothing else does — a `tpl::x::y`
/// path in a doc comment is a link, not a code:
///
/// - `code(tpl::area::kind)` inside a `#[diagnostic]`
/// - `"tpl::lint::kind"` string literals, which `lint` uses for findings that
///   are not errors and so have no `#[diagnostic]` to carry them
fn defined_codes() -> BTreeSet<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = BTreeSet::new();
    collect(&src, &mut out);
    out
}

fn collect(dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read source");
        for opener in ["code(", "\""] {
            for (index, _) in text.match_indices(opener) {
                let rest = &text[index + opener.len()..];
                if !rest.starts_with("tpl::") {
                    continue;
                }
                let end = rest
                    .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                    .unwrap_or(rest.len());
                if let Some(code) = valid_code(&rest[..end]) {
                    out.insert(code);
                }
            }
        }
    }
}

/// `tpl::area::kind`, with a lowercase final segment and no test fixtures.
fn valid_code(candidate: &str) -> Option<String> {
    let parts: Vec<&str> = candidate.split("::").collect();
    if parts.len() != 3 || parts[1] == "test" {
        return None;
    }
    let kind = parts[2];
    let ok = !kind.is_empty()
        && kind
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    ok.then(|| candidate.to_string())
}

/// The documented set. Sorted, one per line, so a diff reads as one addition.
///
/// When this fails: add the code to `docs/reference/diagnostics.md` with a
/// sentence about what a caller should do when it appears, then add it here.
const DOCUMENTED: &[&str] = &[
    "tpl::answers::parse",
    "tpl::answers::read",
    "tpl::answers::shape",
    "tpl::answers::unknown_key",
    "tpl::config::io",
    "tpl::config::missing",
    "tpl::config::parse",
    "tpl::config::serialise",
    "tpl::data::cancelled",
    "tpl::data::checksum",
    "tpl::data::escapes_root",
    "tpl::data::invalid_git_source",
    "tpl::data::load",
    "tpl::data::needs_project",
    "tpl::data::parse",
    "tpl::data::undeclared_remote",
    "tpl::data::unknown_setting",
    "tpl::data::untrusted",
    "tpl::eval::bad_choices",
    "tpl::eval::cancelled",
    "tpl::eval::expression",
    "tpl::eval::invalid_choice",
    "tpl::eval::pattern_mismatch",
    "tpl::eval::unanswered",
    "tpl::eval::wrong_type",
    "tpl::git::auth",
    "tpl::git::backend",
    "tpl::git::clone",
    "tpl::git::dirty_worktree",
    "tpl::git::diverged",
    "tpl::git::network",
    "tpl::git::no_identity",
    "tpl::git::no_such_revision",
    "tpl::git::not_a_repository",
    "tpl::graph::cycle",
    "tpl::graph::invalid_expression",
    "tpl::graph::unknown_reference",
    "tpl::lint::collision",
    "tpl::lint::conflicting_level",
    "tpl::lint::degenerate_path",
    "tpl::lint::foreign_expression",
    "tpl::lint::syntax",
    "tpl::lint::undeclared",
    "tpl::lint::unknown_code",
    "tpl::manifest::invalid_question",
    "tpl::manifest::missing",
    "tpl::manifest::name_collision",
    "tpl::manifest::parse",
    "tpl::ops::already_initialised",
    "tpl::ops::invalid_argument",
    "tpl::ops::no_rendered_ref",
    "tpl::ops::no_such_path",
    "tpl::ops::write_failed",
    "tpl::refs::invalid",
    "tpl::refs::underivable",
    "tpl::render::collision",
    "tpl::render::content",
    "tpl::render::escapes_tree",
    "tpl::render::partial_not_utf8",
    "tpl::render::path",
    "tpl::resolve::cache",
    "tpl::resolve::dirty_needs_local",
    "tpl::resolve::missing_root",
    "tpl::testing::case_parse",
    "tpl::testing::case_shape",
    "tpl::testing::no_such_case",
    "tpl::testing::no_tests",
    "tpl::testing::snapshot_read",
    "tpl::testing::snapshot_write",
    "tpl::testing::write_needs_local",
    "tpl::userconfig::io",
    "tpl::userconfig::parse",
    "tpl::userconfig::shortcut",
    "tpl::value::parse",
    "tpl::value::type_mismatch",
];

#[test]
fn every_diagnostic_code_is_documented() {
    let defined = defined_codes();
    let documented: BTreeSet<String> = DOCUMENTED.iter().map(|s| s.to_string()).collect();

    let undocumented: Vec<&String> = defined.difference(&documented).collect();
    assert!(
        undocumented.is_empty(),
        "these codes exist but are not documented — add them to \
         docs/reference/diagnostics.md and to DOCUMENTED in this file:\n{undocumented:#?}"
    );

    let stale: Vec<&String> = documented.difference(&defined).collect();
    assert!(
        stale.is_empty(),
        "these codes are documented but no longer exist. Removing a code is a \
         breaking change for anyone branching on it:\n{stale:#?}"
    );
}

/// The docs page has to actually mention each one, or the list above is just a
/// second copy of the source with no reader.
#[test]
fn the_reference_page_lists_every_code() {
    let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/reference/diagnostics.md");
    let text = std::fs::read_to_string(&page).expect("read the diagnostics reference");

    let missing: Vec<&&str> = DOCUMENTED
        .iter()
        .filter(|code| !text.contains(**code))
        .collect();
    assert!(
        missing.is_empty(),
        "not mentioned in docs/reference/diagnostics.md:\n{missing:#?}"
    );
}

/// Sorted, so that adding one is a one-line diff rather than a reshuffle.
#[test]
fn the_documented_list_is_sorted() {
    let mut sorted = DOCUMENTED.to_vec();
    sorted.sort_unstable();
    assert_eq!(DOCUMENTED, sorted.as_slice());
}
