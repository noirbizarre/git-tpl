//! `git tpl status`, and in particular its machine-readable form.
//!
//! The text output is asserted incidentally from `tests/diff.rs`,
//! `tests/merge.rs` and `tests/remote_refs.rs`, which use `status` for its exit
//! code. The JSON payload has no such incidental coverage, and it is the half
//! that CI consumes: every key here is a name a script has hard-coded, so a
//! rename is breaking and wants a test that fails when it happens.

mod common;

use common::{World, tpl};

/// The shape before anything has been rendered.
///
/// Every optional field is null rather than absent, so that a caller can index
/// without checking whether the key exists — which is the difference between
/// `.tip // empty` and a `jq` error.
#[test]
fn status_json_reports_nothing_rendered_before_init() {
    let world = World::new();
    world.project.write(
        ".config/git.tpl.toml",
        &format!(
            "[template]\nsource = \"{}\"\n",
            world.template.source().replace('\\', "/")
        ),
    );
    world.project.commit_all("chore: declare the template");

    let json = tpl(&world.project, &["--json", "status"]).json();

    assert_eq!(json["ok"], true);
    assert_eq!(json["ref"], "refs/tpl/template");
    assert_eq!(json["tip"], serde_json::Value::Null);
    assert_eq!(json["renderedRevision"], serde_json::Value::Null);
    assert_eq!(json["renderedCommit"], serde_json::Value::Null);
    assert_eq!(json["dirty"], false);
    assert_eq!(json["renderingCount"], 0);
}

#[test]
fn status_json_reports_the_ref_the_tip_and_the_rendering_count() {
    let world = World::new();
    world.init(&[]).success();

    let json = tpl(&world.project, &["--json", "status"]).success().json();

    assert_eq!(json["ok"], true);
    assert_eq!(json["id"], "template");
    assert_eq!(json["ref"], "refs/tpl/template");
    assert_eq!(
        json["tip"],
        world.project.rev_parse(&world.ref_name()),
        "the tip is the ref it names"
    );
    assert_eq!(json["renderingCount"], 1);
    assert_eq!(json["dirty"], false);
    assert_eq!(json["templateMoved"], false);
    assert_eq!(json["merged"], true);
    assert_eq!(json["pending"], false);
    assert_eq!(json["worktreeClean"], true);
    assert_eq!(json["remote"], serde_json::Value::Null);
    assert!(
        json["renderedCommit"].is_string(),
        "the revision rendered from is recorded: {json}"
    );
}

/// `pending` is the field a CI job branches on, and it must agree with the
/// exit code rather than being computed a second way.
#[test]
fn status_json_says_pending_when_the_template_has_moved() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();

    let output = tpl(&world.project, &["--json", "status"]).code(2);
    let json = output.json();

    assert_eq!(json["templateMoved"], true);
    assert_eq!(json["pending"], true);
    assert!(
        json["availableRevision"].is_string(),
        "the revision that could be rendered is named: {json}"
    );
}

#[test]
fn status_json_says_pending_when_a_rendering_has_not_been_merged() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();

    let json = tpl(&world.project, &["--json", "status"]).code(2).json();

    assert_eq!(json["merged"], false);
    assert_eq!(json["pending"], true);
    assert_eq!(json["renderingCount"], 2);
}

#[test]
fn status_json_reports_a_dirty_worktree() {
    let world = World::new();
    world.init(&[]).success();
    world.project.write("NOTES.md", "edited, not committed\n");

    let json = tpl(&world.project, &["--json", "status"]).json();

    assert_eq!(json["worktreeClean"], false);
}

/// The flag `--json` replaced is gone, as ADR-015 said it would be in 0.7.
///
/// The assertion is on behaviour, not on clap's wording: a caller still pinned
/// to the old spelling must fail loudly rather than get a report it cannot
/// distinguish from a JSON one.
#[test]
fn the_removed_format_flag_is_rejected() {
    let world = World::new();
    world.init(&[]).success();

    let output = tpl(&world.project, &["status", "--format", "json"]).failure();

    assert!(
        output.stdout.trim().is_empty(),
        "a rejected invocation produces no payload: {:?}",
        output.stdout
    );
}
