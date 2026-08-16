//! The `--json` contract, across commands.
//!
//! Per-command payloads are asserted beside the command they belong to. What
//! lives here is the property that holds for *all* of them, and that issue #53
//! broke: a `--json` invocation that succeeds prints exactly one JSON object on
//! stdout, and nothing else.

mod common;

use common::{World, tpl};

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
