//! The `--json` contract, across commands.
//!
//! Per-command payloads are asserted beside the command they belong to. What
//! lives here is the property that holds for *all* of them, and that issue #53
//! broke: a `--json` invocation that succeeds prints exactly one JSON object on
//! stdout, and nothing else.

mod common;

use common::{World, tpl};

#[test]
fn composed_resolution_keeps_ancestor_trust_diagnostics_and_explicit_consent() {
    let base = common::Repo::init("base");
    base.write("template.toml", "name = 'base'")
        .write("template/base", "base")
        .commit_all("base template");
    let child = common::Repo::init("child");
    child.write("template.toml", &format!("name = 'child'\n[extends]\nsource = '{}'\nrev = '{}'\n[questions.module_name]\ntype = 'string'\n[questions.module_path]\ntype = 'string'", common::file_url(&base.path), base.git(&["rev-parse", "HEAD"])))
        .write("template/child", "child").commit_all("child with remote ancestor");
    let parent = common::Repo::init("parent");
    parent.write("template.toml", &format!("name = 'parent'\n[questions.modules]\ntype = 'list'\ndefault = ['a']\n[[compose]]\nsource = '{}'\nref = 'main'\nfrom = 'modules'", child.path.display()))
        .write("template/parent", "parent").commit_all("composed child");
    // Trust in both the parent and child must not erase an ancestor's own
    // refusal, and that refusal must survive the composed diagnostic wrapper.
    parent.user_config(&format!(
        "[trust]\ntemplates = ['{}', '{}']",
        parent.path.display(),
        child.path.display()
    ));
    let destination = parent.path.join("output");
    let args = [
        "--json",
        "render",
        parent.path.to_str().unwrap(),
        "--defaults",
        "-o",
        destination.to_str().unwrap(),
    ];
    let json = tpl(&parent, &args).failure().json();
    assert_eq!(json["error"]["code"], "tpl::compose::child_failed");
    assert_eq!(
        json["error"]["causes"][0]["code"],
        "tpl::extends::untrusted"
    );
    assert!(
        json["error"]["causes"][0]["help"]
            .as_str()
            .unwrap()
            .contains("--trust")
    );
    assert!(!destination.exists());
    let mut trusted = args.to_vec();
    trusted.push("--trust");
    tpl(&parent, &trusted).success();
    assert_eq!(
        std::fs::read_to_string(destination.join("base")).unwrap(),
        "base"
    );
}

#[test]
fn composed_failures_preserve_nested_codes_help_and_template_test_matching() {
    let child = common::Repo::init("child");
    child.write("template.toml", "name = 'child'\n[questions.module_name]\ntype = 'string'\n[questions.module_path]\ntype = 'string'\n[questions.required]\ntype = 'integer'\n")
        .write("template/out", "child").commit_all("required child question");
    let parent = common::Repo::init("parent");
    parent.write("template.toml", &format!("name = 'parent'\n[questions.modules]\ntype = 'list'\ndefault = ['a']\n[[compose]]\nsource = '{}'\nref = 'main'\nfrom = 'modules'\n", child.path.display()))
        .write("template/keep", "parent")
        .write("tests/inner.toml", "[expect]\nerror = 'tpl::eval::unanswered'\n")
        .commit_all("composed failure expectation");
    let destination = parent.path.join("output");
    let json = tpl(
        &parent,
        &[
            "--json",
            "render",
            parent.path.to_str().unwrap(),
            "--defaults",
            "-o",
            destination.to_str().unwrap(),
        ],
    )
    .failure()
    .json();
    assert_eq!(json["error"]["code"], "tpl::compose::child_failed");
    assert_eq!(json["error"]["causes"][0]["code"], "tpl::eval::unanswered");
    assert!(
        json["error"]["causes"][0]["help"]
            .as_str()
            .unwrap()
            .contains("--answer required=")
    );
    assert!(!destination.exists());
    let json = tpl(
        &parent,
        &[
            "--json",
            "test",
            "--template",
            parent.path.to_str().unwrap(),
        ],
    )
    .success()
    .json();
    assert_eq!(json["summary"]["passed"], 1);
}

/// The guard against the next command reopening the hole.
///
/// `update` used to print nothing at all when there was nothing to do, so a
/// caller doing `git tpl --json update | jq .ok` got a parse error on the most
/// common outcome. A per-command test would not have caught it, because the
/// command it was missing from was the one nobody thought to test.
///
/// `show`, `completion` and `man` are exempt by documented decision: their
/// stdout is already the payload. That is asserted where they live.
#[test]
fn every_command_emits_exactly_one_json_object_on_success() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    let source = world.template.source();
    let commands: Vec<Vec<&str>> = vec![
        vec!["update", "--defaults", "--dry-run"],
        vec!["update", "--defaults"],
        // Twice: the second run has nothing to do, which is the outcome that
        // was silent.
        vec!["update", "--defaults"],
        vec!["merge"],
        vec!["status"],
        vec!["diff"],
        vec!["questions", &source],
        vec!["context", &source, "--defaults"],
        vec!["lint", &source],
    ];

    for args in &commands {
        let mut invocation = vec!["--json"];
        invocation.extend_from_slice(args);
        let output = tpl(&world.project, &invocation);

        assert!(
            !output.stdout.trim().is_empty(),
            "`git tpl {}` printed nothing on stdout\n--- stderr ---\n{}",
            invocation.join(" "),
            output.stderr
        );
        // `json()` panics with both streams when this is not a single object,
        // which is what a stray human line on stdout looks like.
        let json = output.json();
        assert_eq!(
            json["ok"],
            true,
            "`git tpl {}` did not report success: {json}",
            invocation.join(" ")
        );
    }
}

/// Prose on stderr, payload on stdout. The separation is what makes a piped
/// `--json` stream parseable even when the command is chatty.
#[test]
fn human_output_never_reaches_the_json_stream() {
    let world = World::new();
    world.init(&[]).success();

    let output = tpl(&world.project, &["--json", "update", "--defaults"]).success();

    assert_eq!(
        output.stdout.lines().count(),
        1,
        "more than one line on stdout:\n{}",
        output.stdout
    );
    output.json();
}
