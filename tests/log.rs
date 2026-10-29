//! Source history uses real Git reachability, not the most recent rendering.

mod common;

use common::{World, file_url, tpl, tpl_colored};

const DATE: &str = "2026-10-29T14:35:42+00:00";

fn entries(world: &World, args: &[&str]) -> Vec<serde_json::Value> {
    let mut command = vec!["--json", "log"];
    command.extend_from_slice(args);
    tpl(&world.project, &command).success().json()["entries"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn an_unmerged_rendering_does_not_advance_the_applied_log() {
    let world = World::new();
    world.init(&[]).success();
    let applied = world.template.repo.rev_parse("HEAD");
    world.move_template();
    tpl(&world.project, &["update", "--defaults"]).success();
    world
        .template
        .repo
        .commit_all_at("docs: a later source-only change", DATE);

    let json = tpl(&world.project, &["log", "--json"]).success().json();
    assert_eq!(json["applied"]["commit"], applied);
    assert_eq!(json["target"]["commit"], applied);
    assert_eq!(json["entries"][0]["commit"], applied);
    assert_eq!(json["entries"].as_array().unwrap().len(), 1);

    tpl(&world.project, &["merge"]).success();
    assert_eq!(
        entries(&world, &[])[0]["subject"],
        "feat: add a release workflow"
    );
}

#[test]
fn explicit_template_branches_tags_and_shas_select_the_same_range() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    world
        .template
        .repo
        .git(&["tag", "-a", "next", "-m", "next release"]);
    let target = world.template.repo.rev_parse("HEAD");
    // Project main has a different history: resolving there would produce a wrong range.
    for reference in ["main", "next", target.as_str()] {
        let result = entries(&world, &[reference]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["commit"], target);
    }
    let output = tpl(&world.project, &["--json", "log", "missing-ref"])
        .code(1)
        .json();
    assert_eq!(output["error"]["code"], "tpl::git::no_such_revision");
}

#[test]
fn equal_older_and_divergent_targets_follow_two_dot_semantics() {
    let world = World::new();
    let older = world.template.repo.rev_parse("HEAD");
    world.move_template();
    world.init(&[]).success();
    assert!(entries(&world, &["main"]).is_empty());
    assert!(entries(&world, &[&older]).is_empty());
    world
        .template
        .repo
        .git(&["checkout", "-q", "-b", "other", &older]);
    world
        .template
        .repo
        .commit_all_at("fix: another line of development", DATE);
    let output = tpl(&world.project, &["--json", "log", "other"])
        .success()
        .json();
    assert_eq!(output["targetDescendsFromApplied"], false);
    assert_eq!(output["entries"].as_array().unwrap().len(), 1);
    assert!(
        tpl(&world.project, &["log", "other"])
            .success()
            .stderr
            .contains("target-only")
    );
}

#[test]
fn source_merge_history_includes_both_parents_and_excludes_applied_ancestry() {
    let world = World::new();
    world.init(&[]).success();
    world
        .template
        .repo
        .git(&["checkout", "-q", "-b", "feature"]);
    world.template.repo.commit_all_at("feat: side branch", DATE);
    let side = world.template.repo.rev_parse("HEAD");
    world.template.repo.git(&["checkout", "-q", "main"]);
    world
        .template
        .repo
        .commit_all_at("fix: main branch", "2026-10-28T14:35:00Z");
    world
        .template
        .repo
        .git(&["merge", "--no-ff", "feature", "-m", "Merge feature"]);
    let result = entries(&world, &["main", "--all"]);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0]["subject"], "Merge feature");
    assert!(result.iter().any(|entry| entry["commit"] == side));
    assert_eq!(entries(&world, &["main", "-n", "1"])[0], result[0]);
}

#[test]
fn history_limits_default_to_twenty_and_can_be_overridden_or_removed() {
    let world = World::new();
    for n in 0..23 {
        world
            .template
            .repo
            .commit_all_at(&format!("chore: change {n}"), DATE);
    }
    world.init(&[]).success();
    assert_eq!(entries(&world, &[]).len(), 20);
    assert_eq!(entries(&world, &["--max-count", "3"]).len(), 3);
    assert_eq!(entries(&world, &["--all"]).len(), 24);
    assert!(entries(&world, &["-n", "0"]).is_empty());
    tpl(&world.project, &["log", "--all", "-n", "1"]).code(2);
    tpl(&world.project, &["log", "-n", "-1"]).code(2);
}

#[test]
fn log_reads_git_history_even_when_the_target_manifest_is_invalid() {
    let world = World::new();
    world.init(&[]).success();
    world
        .template
        .repo
        .write("template.toml", "not valid TOML")
        .commit_all_at("fix: work in progress", DATE);
    assert_eq!(
        entries(&world, &["main"])[0]["subject"],
        "fix: work in progress"
    );
    assert_eq!(entries(&world, &[]).len(), 1);
}

#[test]
fn inspection_preserves_dirty_project_and_source_state_and_all_refs() {
    let world = World::new();
    world.init(&[]).success();
    world.move_template();
    world.project.write("NOTES.md", "staged changes\n");
    world.project.git(&["add", "NOTES.md"]);
    world.project.write("untracked", "leave this alone");
    world.template.repo.write("README.md", "dirty source");
    let project = world.project.working_state();
    let source = world.template.repo.working_state();
    let refs = world.project.git(&["show-ref"]);
    let source_refs = world.template.repo.git(&["show-ref"]);
    let config = world.project.read(".git/config");
    tpl(&world.project, &["log", "main"]).success();
    assert_eq!(world.project.working_state(), project);
    assert_eq!(world.template.repo.working_state(), source);
    assert_eq!(world.project.git(&["show-ref"]), refs);
    assert_eq!(world.template.repo.git(&["show-ref"]), source_refs);
    assert_eq!(world.project.read(".git/config"), config);
}

#[test]
fn remote_style_source_access_uses_an_ephemeral_clone() {
    let world = World::new();
    tpl(
        &world.project,
        &["init", &file_url(&world.template.repo.path), "--defaults"],
    )
    .success();
    world.move_template();
    assert_eq!(entries(&world, &["main"]).len(), 1);
    assert_eq!(entries(&world, &[]).len(), 1);
}

#[test]
fn no_applied_rendering_or_missing_ref_is_actionable() {
    let world = World::new();
    world.init(&["--no-merge"]).success();
    let output = tpl(&world.project, &["--json", "log"]).code(1).json();
    assert_eq!(output["error"]["code"], "tpl::log::not_applied");
    world.project.git(&["update-ref", "-d", &world.ref_name()]);
    let output = tpl(&world.project, &["--json", "log"]).code(1).json();
    assert_eq!(output["error"]["code"], "tpl::ops::no_rendered_ref");
}

#[test]
fn a_dirty_applied_template_has_no_exact_source_history_boundary() {
    let world = World::new();
    world
        .template
        .repo
        .write("template/extra.txt", "uncommitted");
    world.init(&["--dirty"]).success();
    let output = tpl(&world.project, &["--json", "log"]).code(1).json();
    assert_eq!(output["error"]["code"], "tpl::log::dirty");
}

/// Hand-authored/migration-only rendered commits must not silently select an older boundary.
fn apply_message(world: &World, message: &str) {
    let parent = world.project.rev_parse(&world.ref_name());
    let tree = world
        .project
        .rev_parse(&format!("{}^{{tree}}", world.ref_name()));
    let commit = world
        .project
        .git(&["commit-tree", &tree, "-p", &parent, "-m", message]);
    world
        .project
        .git(&["update-ref", &world.ref_name(), &commit]);
    world
        .project
        .git(&["merge", "--no-edit", &world.ref_name()]);
}

#[test]
fn an_applied_commit_without_provenance_is_not_replaced_by_an_older_rendering() {
    let world = World::new();
    world.init(&[]).success();
    apply_message(&world, "tpl: move files before rendering");
    let output = tpl(&world.project, &["--json", "log"]).code(1).json();
    assert_eq!(output["error"]["code"], "tpl::log::provenance");
    apply_message(
        &world,
        "tpl: incomplete provenance\n\nTemplate-Source: nowhere",
    );
    let output = tpl(&world.project, &["--json", "log"]).code(1).json();
    assert_eq!(output["error"]["code"], "tpl::log::provenance");
}

#[test]
fn missing_historical_source_objects_do_not_expand_the_range() {
    let world = World::new();
    world.init(&[]).success();
    apply_message(
        &world,
        &format!(
            "tpl: missing source commit\n\nTemplate-Source: {}\nTemplate-Commit: {}",
            world.template.source(),
            "a".repeat(40)
        ),
    );
    let output = tpl(&world.project, &["--json", "log", "main"])
        .code(1)
        .json();
    assert_eq!(output["error"]["code"], "tpl::log::missing_commit");
}

#[test]
fn date_preferences_change_presentation_but_never_the_json_payload() {
    let world = World::new();
    world
        .template
        .repo
        .commit_all_at("feat(cli)!: inspect history", DATE);
    world.init(&[]).success();
    let json = tpl(&world.project, &["--json", "log", "-n", "1"])
        .success()
        .json();
    assert_eq!(
        json["entries"][0]["conventional"],
        serde_json::json!({"type": "feat", "scope": "cli", "breaking": true})
    );
    assert!(json["entries"][0]["committedAt"].is_i64());
    let absolute = tpl(&world.project, &["log", "-n", "1", "--no-relative-date"]).success();
    assert!(absolute.stderr.contains("2026-10-29 14:35 UTC"));
    world
        .project
        .git(&["config", "tpl.log.relativeDate", "false"]);
    assert_eq!(
        tpl(&world.project, &["log", "-n", "1"]).success().stderr,
        absolute.stderr
    );
    assert_eq!(
        tpl(
            &world.project,
            &["--json", "log", "-n", "1", "--no-relative-date"]
        )
        .success()
        .json(),
        json
    );
    let quiet = tpl(&world.project, &["log", "--quiet"]).success();
    assert!(quiet.stdout.is_empty() && quiet.stderr.is_empty());
}

#[test]
fn untrusted_subjects_are_plain_text_before_our_own_highlighting() {
    let world = World::new();
    world
        .template
        .repo
        .commit_all_at("feat(cli): \x1b[2Jdon't erase\x1b]52;c;clipboard\x07", DATE);
    world.init(&[]).success();
    let plain = tpl(&world.project, &["log", "-n", "1"]).success();
    assert!(!plain.stderr.contains('\x1b'));
    assert!(plain.stderr.contains("feat(cli): don't erase"));
    let colored = tpl_colored(&world.project, &["log", "-n", "1"]).success();
    assert!(colored.stderr.contains('\x1b'));
    assert!(!colored.stderr.contains("\x1b[2J"));
    assert!(!colored.stderr.contains("clipboard"));
    assert!(
        entries(&world, &["-n", "1"])[0]["subject"]
            .as_str()
            .unwrap()
            .contains('\x1b')
    );
}

#[test]
fn edited_attachment_sources_do_not_replace_the_applied_source_repository() {
    let world = World::new();
    world.init(&[]).success();
    let config = world.project.read(".config/git.tpl.toml");
    let changed = config
        .replace(
            &world.template.declared_source(),
            "https://invalid.example/future-template",
        )
        .replace("[template]", "[template]\nid = \"template\"");
    assert_ne!(config, changed);
    world.project.write(".config/git.tpl.toml", &changed);
    assert_eq!(entries(&world, &[])[0]["subject"], "feat: initial template");
}

#[test]
fn history_before_the_manifest_and_non_utf8_subjects_remain_visible() {
    let world = World::new();
    world.init(&[]).success();
    let repo = &world.template.repo;
    let parent = repo.rev_parse("HEAD");
    let tree = repo.rev_parse("HEAD^{tree}");
    // Write a real commit object because `git commit -m` receives UTF-8 arguments from Rust.
    let mut bytes = format!("tree {tree}\nparent {parent}\nauthor Test <test@example.invalid> 1793284542 +0000\ncommitter Test <test@example.invalid> 1793284542 +0000\n\nfix: invalid ").into_bytes();
    bytes.extend_from_slice(b"\xff subject\n");
    std::fs::write(repo.path.join("raw-commit"), bytes).unwrap();
    let commit = repo.git(&["hash-object", "-t", "commit", "-w", "raw-commit"]);
    repo.git(&["update-ref", "refs/heads/main", &commit]);
    assert_eq!(
        entries(&world, &["main"])[0]["subject"],
        "fix: invalid � subject"
    );

    // A target before any manifest is still legitimate Git history.
    repo.git(&["checkout", "-q", "--orphan", "pre-template"]);
    repo.git(&["rm", "-r", "-q", "-f", "."]);
    repo.write("README.md", "before template.toml existed")
        .commit_all_at("Start repository", DATE);
    assert_eq!(
        entries(&world, &["pre-template"])[0]["subject"],
        "Start repository"
    );
}
