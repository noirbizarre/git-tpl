//! Pull composes real rendering and Git integration, including their failure boundaries.

mod common;

use common::{World, tpl};

#[test]
fn pull_publishes_append_only_history_and_merges_it_in_one_invocation() {
    let world = World::new();
    world.init(&[]).success();
    let previous = world.project.rev_parse(&world.ref_name());
    let config = world.project.read(".config/git.tpl.toml");
    world.move_template();

    let output = tpl(&world.project, &["--json", "pull", "--defaults"]).success();
    let json = output.json();
    assert_eq!(output.stdout.lines().count(), 1);
    assert_eq!(json["update"]["result"], "updated");
    assert_eq!(json["merge"]["result"], "merged");
    let tip = world.project.rev_parse(&world.ref_name());
    assert_eq!(world.project.rev_parse(&format!("{tip}^")), previous);
    assert_eq!(world.project.rev_parse("HEAD^2"), tip);
    assert!(world.project.exists(".github/workflows/release.yml"));
    assert_eq!(world.project.read(".config/git.tpl.toml"), config);
    assert_eq!(world.project.status(), "");

    let before = world.project.working_state();
    let json = tpl(&world.project, &["--json", "pull", "--defaults"])
        .success()
        .json();
    assert_eq!(json["update"]["result"], "upToDate");
    assert_eq!(json["merge"]["result"], "upToDate");
    assert_eq!(world.project.working_state(), before);
}

#[test]
fn an_unchanged_update_still_merges_pending_history() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    let tip = world.project.rev_parse(&world.ref_name());
    let json = tpl(&world.project, &["--json", "pull", "--defaults"])
        .success()
        .json();
    assert_eq!(json["update"]["result"], "upToDate");
    assert_eq!(json["merge"]["result"], "merged");
    assert_eq!(world.project.rev_parse("HEAD^2"), tip);
}

#[test]
fn a_missing_rendered_ref_is_refused_before_answers_or_files_change() {
    let world = World::new();
    world.init(&[]).success();
    world.project.git(&["update-ref", "-d", &world.ref_name()]);
    world.move_template();
    let before = world.project.working_state();
    let json = tpl(
        &world.project,
        &[
            "--json",
            "pull",
            "--defaults",
            "--answer",
            "project_name=changed",
        ],
    )
    .failure()
    .json();
    assert_eq!(json["error"]["code"], "tpl::ops::no_rendered_ref");
    assert!(!world.project.has_ref(&world.ref_name()));
    assert_eq!(world.project.working_state(), before);
}

#[test]
fn update_failure_does_not_attempt_integration() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    let before = world.project.working_state();
    let tip = world.project.rev_parse(&world.ref_name());
    tpl(&world.project, &["pull", "--defaults", "--ref", "absent"]).failure();
    assert_eq!(world.project.working_state(), before);
    assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
}

#[test]
fn a_refused_checkout_preserves_the_completed_update() {
    let world = World::new();
    world.init(&[]).success();
    world
        .project
        .write("README.md", "uncommitted local content\n");
    let before = world.project.working_state();
    let previous = world.project.rev_parse(&world.ref_name());
    world.move_template();
    tpl(&world.project, &["--json", "pull", "--defaults"]).failure();
    assert_eq!(world.project.working_state(), before);
    assert_ne!(world.project.rev_parse(&world.ref_name()), previous);
}

#[test]
fn answer_changes_remain_unstaged_after_the_rendering_is_merged() {
    let world = World::new();
    world.init(&[]).success();
    let json = tpl(
        &world.project,
        &[
            "--json",
            "pull",
            "--defaults",
            "--answer",
            "project_name=renamed",
        ],
    )
    .success()
    .json();
    assert_eq!(json["update"]["answersChanged"], true);
    assert!(world.project.read("Cargo.toml").contains("renamed"));
    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("renamed")
    );
    assert_eq!(world.project.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(
        world.project.git(&["diff", "--name-only"]),
        ".config/git.tpl.toml"
    );
}

#[test]
fn conflicts_keep_normal_git_recovery_state_and_report_one_success_envelope() {
    let world = World::new();
    world.init(&[]).success();
    let readme = world.project.read("README.md");
    world
        .project
        .write("README.md", &format!("{readme}\nLocal ending.\n"));
    world.project.commit_all("docs: append local ending");
    let before = world.project.working_state();
    world.move_template();
    let json = tpl(&world.project, &["--json", "pull", "--defaults"])
        .success()
        .json();
    assert_eq!(json["ok"], true);
    assert_eq!(json["merge"]["result"], "conflicted");
    assert_eq!(json["merge"]["conflicts"], serde_json::json!(["README.md"]));
    assert!(world.project.exists(".git/MERGE_HEAD"));
    assert!(world.project.read("README.md").contains("<<<<<<<"));
    tpl(&world.project, &["merge", "--abort"]).success();
    assert_eq!(world.project.working_state(), before);
}

#[test]
fn no_commit_stages_the_merge_and_a_custom_message_is_used_when_committing() {
    for no_commit in [false, true] {
        let world = World::new();
        world.init(&[]).success();
        let head = world.project.rev_parse("HEAD");
        world.move_template();
        let mut args = vec![
            "--json",
            "pull",
            "--defaults",
            "-m",
            "Apply template changes",
        ];
        if no_commit {
            args.push("--no-commit");
        }
        let json = tpl(&world.project, &args).success().json();
        if no_commit {
            assert_eq!(json["merge"]["result"], "staged");
            assert_eq!(world.project.rev_parse("HEAD"), head);
            assert!(world.project.exists(".git/MERGE_HEAD"));
        } else {
            assert_eq!(json["merge"]["result"], "merged");
            assert_eq!(
                world.project.git(&["log", "-1", "--format=%s"]),
                "Apply template changes"
            );
        }
    }
}

#[test]
fn no_commit_still_allows_fast_forward_integration() {
    let world = World::new();
    world.init(&[]).success();
    // Place the branch at its rendered parent to exercise fast-forward without
    // manufacturing a mock backend or changing the rendering's ancestry.
    let config = world.project.read(".config/git.tpl.toml");
    world.project.git(&["reset", "--hard", &world.ref_name()]);
    world.project.write(".config/git.tpl.toml", &config);
    world.move_template();
    let json = tpl(
        &world.project,
        &["--json", "pull", "--defaults", "--no-commit"],
    )
    .success()
    .json();
    assert_eq!(json["merge"]["result"], "fastForward");
    assert_eq!(
        world.project.rev_parse("HEAD"),
        world.project.rev_parse(&world.ref_name())
    );
    assert_eq!(world.project.read(".config/git.tpl.toml"), config);
}

#[test]
fn pull_rejects_preview_abort_and_incompatible_template_selection_flags() {
    let world = World::new();
    for extra in [
        vec!["--dry-run"],
        vec!["--abort"],
        vec!["--dirty", "--ref", "main"],
    ] {
        let mut args = vec!["pull"];
        args.extend(extra);
        assert_eq!(tpl(&world.project, &args).failure().code, 2);
    }
}
