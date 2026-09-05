//! `[extends]` and the trust gate.
//!
//! A remote ancestor's `source` is chosen by the *template author*, exactly
//! like a `[data]` `kind = "git"` source, so cloning it needs the same
//! confirmation — see `docs/adr/034-template-inheritance.md`. A `file://` URL
//! is a real clone as far as libgit2 is concerned (the same code path,
//! `tests/data_git.rs`'s own rationale), so nothing here needs to leave the
//! machine to be honest about what it proves.

mod common;

use common::{Repo, file_url, local_toml_path, tpl};

/// A parent template with one file, tagged `v1.0.0`.
fn parent(dir: &std::path::Path) -> Repo {
    let parent = Repo::init_in(dir, "parent");
    parent.write("template.toml", "name = \"base\"\n");
    parent.write("template/README.md", "from the base\n");
    parent.commit_all("feat: initial");
    parent.git(&["tag", "v1.0.0"]);
    parent
}

/// A child extending `parent` at `source`, with a `template/dummy.txt` of its
/// own so its own root is never the reason a resolve fails.
fn child(dir: &std::path::Path, source: &str) -> Repo {
    let child = Repo::init_in(dir, "child");
    child.write(
        "template.toml",
        &format!("name = \"child\"\n\n[extends]\nsource = \"{source}\"\nrev = \"v1.0.0\"\n"),
    );
    child.write("template/dummy.txt", "x");
    child.commit_all("feat: initial");
    child
}

fn project(dir: &std::path::Path) -> Repo {
    let project = Repo::init_in(dir, "project");
    project.write("NOTES.md", "notes\n");
    project.commit_all("chore: initial");
    project
}

fn init(project: &Repo, child: &Repo, extra: &[&str]) -> common::Output {
    let source = child.path.to_string_lossy().to_string();
    let mut args = vec!["init", &source, "--defaults"];
    args.extend_from_slice(extra);
    tpl(project, &args)
}

/// The acceptance case: a remote ancestor is refused, exactly like a `[data]`
/// `kind = "git"` source would be.
#[test]
fn a_remote_ancestor_is_refused_when_the_template_is_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    let child = child(dir.path(), &file_url(&parent.path));
    let project = project(dir.path());

    // A clone carries the user's credentials to whatever host the ancestor
    // names. Treating it as less than a `[data]` git clone would make the
    // gate avoidable by choosing a different spelling.
    let output = init(&project, &child, &[]).failure();
    output.says("tpl::extends::untrusted");
}

/// `[trust]` covering the *ancestor's own* source authorises the clone
/// without a prompt.
#[test]
fn a_trust_entry_for_the_ancestor_authorises_the_clone() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    let child = child(dir.path(), &file_url(&parent.path));
    let project = project(dir.path());
    project.user_config(&format!(
        "[trust]\ntemplates = [\"{}\"]\n",
        file_url(&parent.path)
    ));

    init(&project, &child, &[]).success();
    assert!(project.read("README.md").starts_with("from the base"));
}

/// `--trust` authorises it too.
#[test]
fn the_trust_flag_authorises_the_ancestor_clone() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    let child = child(dir.path(), &file_url(&parent.path));
    let project = project(dir.path());

    init(&project, &child, &["--trust"]).success();
    assert!(project.read("README.md").starts_with("from the base"));
}

/// Trusting the *leaf* template does not extend to what it `[extends]` —
/// each ancestor's own source is checked independently (ADR-034).
#[test]
fn trusting_the_leaf_does_not_trust_a_remote_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    let child = child(dir.path(), &file_url(&parent.path));
    let project = project(dir.path());
    // Covers the *child's* own source, not the parent's.
    project.user_config(&format!(
        "[trust]\ntemplates = [\"{}\"]\n",
        local_toml_path(&child.path)
    ));

    let output = init(&project, &child, &[]).failure();
    output.says("tpl::extends::untrusted");
}

/// A local ancestor needs no trust at all — the entire rest of
/// `tests/extends.rs` already exercises this implicitly (every fixture there
/// uses a local parent and passes with no `--trust`); this pins it
/// explicitly, once, as a regression guard for the local exemption itself.
#[test]
fn a_local_ancestor_needs_no_trust() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    let child = child(dir.path(), &local_toml_path(&parent.path));
    let project = project(dir.path());

    init(&project, &child, &[]).success();
    assert!(project.read("README.md").starts_with("from the base"));
}

/// `git tpl lint` has no `--trust` of its own and refuses outright.
#[test]
fn lint_refuses_an_untrusted_remote_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    let child = child(dir.path(), &file_url(&parent.path));
    let config = tempfile::tempdir().unwrap();

    let output = common::tpl_outside(
        dir.path(),
        config.path(),
        &["--json", "lint", &child.path.to_string_lossy()],
    )
    .failure();
    assert_eq!(output.error_code(), "tpl::extends::untrusted");
}

/// `git tpl questions` has no `--trust` of its own and refuses outright.
#[test]
fn questions_refuses_an_untrusted_remote_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    let child = child(dir.path(), &file_url(&parent.path));
    let config = tempfile::tempdir().unwrap();

    let output = common::tpl_outside(
        dir.path(),
        config.path(),
        &["--json", "questions", &child.path.to_string_lossy()],
    )
    .failure();
    assert_eq!(output.error_code(), "tpl::extends::untrusted");
}

/// `git tpl status` degrades exactly like any other resolve failure on its
/// best-effort "available" side — it does not surface a distinct error.
#[test]
fn status_degrades_silently_for_an_untrusted_remote_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    // Trust the ancestor just long enough to attach, then withdraw it, so
    // `status` still has something recorded to compare against.
    let child = child(dir.path(), &file_url(&parent.path));
    let project = project(dir.path());
    init(&project, &child, &["--trust"]).success();

    let status = tpl(&project, &["--json", "status"]).success().json();
    assert_eq!(status["availableCommit"], serde_json::Value::Null);
    assert_eq!(
        status["availableReferenceDescription"],
        serde_json::Value::Null
    );
}

/// `git tpl test` resolves the chain once, before any case is read, so it
/// checks `[trust]` alone -- a case's own `trust = true` does not bypass an
/// untrusted `[extends]` ancestor (ADR-028 scopes a case's `trust` to
/// declared `[data]` sources, not the manifest chain that produced them).
#[test]
fn test_refuses_an_untrusted_remote_ancestor_regardless_of_case_trust() {
    let dir = tempfile::tempdir().unwrap();
    let parent = parent(dir.path());
    let child = child(dir.path(), &file_url(&parent.path));
    child.write("tests/a.toml", "trust = true\n[answers]\n");
    child.commit_all("test: add a case");

    let output = tpl(&child, &["--json", "test"]).failure();
    assert_eq!(output.error_code(), "tpl::extends::untrusted");
}
