//! One-shot output remains in rendered history so ordinary Git merges preserve user ownership.

mod common;

use common::{World, tpl};

const MANIFEST: &str =
    "name = 'once'\nstrict = true\n[files]\n'assets/logo.svg' = { once = true }\n";

fn world() -> World {
    World::with_template(
        MANIFEST,
        &[
            ("assets/logo.svg.jinja", "initial logo\n"),
            ("ordinary", "first\n"),
        ],
    )
}

fn blob(world: &World, path: &str) -> String {
    world
        .project
        .rev_parse(&format!("{}:{path}", world.ref_name()))
}

#[test]
fn user_replacements_and_deletions_merge_cleanly_after_repeated_template_changes() {
    for deleted in [false, true] {
        let world = world();
        world.init(&[]).success();
        assert_eq!(world.project.read("assets/logo.svg"), "initial logo\n");
        let initial = blob(&world, "assets/logo.svg");
        if deleted {
            world.project.remove("assets/logo.svg");
        } else {
            world
                .project
                .write("assets/logo.svg", "entirely user-owned replacement\n");
        }
        world.project.commit_all("replace initialized asset");
        for iteration in 0..3 {
            if iteration == 0 {
                // Retained bodies must not evaluate even newly invalid strict expressions.
                world
                    .template
                    .repo
                    .write("template/assets/logo.svg.jinja", "{{ missing }}");
            } else if iteration == 1 {
                world.template.repo.remove("template/assets/logo.svg.jinja");
            } else {
                world
                    .template
                    .repo
                    .write("template.toml", "name = 'once'\n");
            }
            world
                .template
                .repo
                .write("template/ordinary", &format!("iteration {iteration}\n"));
            world.template.repo.commit_all("advance ordinary output");
            let before = world.project.working_state();
            tpl(&world.project, &["update", "--defaults"]).success();
            assert_eq!(world.project.working_state(), before);
            assert_eq!(blob(&world, "assets/logo.svg"), initial);
            tpl(&world.project, &["merge"]).success();
            assert_eq!(
                world.project.read("ordinary"),
                format!("iteration {iteration}\n")
            );
            if deleted {
                assert!(!world.project.exists("assets/logo.svg"));
            } else {
                assert_eq!(
                    world.project.read("assets/logo.svg"),
                    "entirely user-owned replacement\n"
                );
            }
        }
    }
}

#[test]
fn removing_or_disabling_the_declaration_never_thaws_a_captured_path() {
    for policy in ["", "[files]\n'assets/logo.svg' = { once = false }\n"] {
        let world = world();
        world.init(&[]).success();
        let initial = blob(&world, "assets/logo.svg");
        world
            .template
            .repo
            .write("template.toml", &format!("name = 'once'\n{policy}"));
        world
            .template
            .repo
            .write("template/assets/logo.svg.jinja", "changed\n");
        world.template.repo.remove("template/ordinary");
        world
            .template
            .repo
            .commit_all("remove policy but keep source");
        tpl(&world.project, &["update", "--defaults"]).success();
        assert_eq!(blob(&world, "assets/logo.svg"), initial);
        assert_eq!(
            world
                .project
                .git(&["ls-tree", &world.ref_name(), "ordinary"]),
            ""
        );
        world.template.repo.remove("template/assets/logo.svg.jinja");
        world
            .template
            .repo
            .write("template/other", "keep render root\n");
        world.template.repo.commit_all("remove source");
        tpl(&world.project, &["update", "--defaults"]).success();
        world
            .template
            .repo
            .write("template/assets/logo.svg.jinja", "reintroduced\n");
        world.template.repo.commit_all("reintroduce source");
        tpl(&world.project, &["update", "--defaults"]).success();
        assert_eq!(blob(&world, "assets/logo.svg"), initial);
    }
}

#[test]
fn adding_a_policy_records_capture_even_when_output_is_identical() {
    let world = World::with_template("name = 'once'\n", &[("assets/logo.svg.jinja", "initial\n")]);
    world.init(&[]).success();
    let initial = world.project.rev_parse(&world.ref_name());
    world.template.repo.write("template.toml", MANIFEST);
    world.template.repo.commit_all("adopt one-shot policy");
    let dry = tpl(
        &world.project,
        &["--json", "update", "--defaults", "--dry-run"],
    )
    .success()
    .json();
    assert_eq!(dry["result"], "wouldUpdate");
    assert_eq!(dry["changes"], serde_json::json!([]));
    let result = tpl(&world.project, &["--json", "update", "--defaults"])
        .success()
        .json();
    assert_eq!(result["result"], "updated");
    assert_ne!(world.project.rev_parse(&world.ref_name()), initial);
    assert_eq!(
        world.project.rev_parse(&format!("{}^", world.ref_name())),
        initial
    );
    let captured = world.project.rev_parse(&world.ref_name());
    tpl(&world.project, &["update", "--defaults"]).success();
    assert_eq!(world.project.rev_parse(&world.ref_name()), captured);
    world
        .template
        .repo
        .write("template.toml", "name = 'once'\n");
    world
        .template
        .repo
        .write("template/assets/logo.svg.jinja", "new\n");
    world
        .template
        .repo
        .commit_all("remove newly adopted policy");
    tpl(&world.project, &["update", "--defaults"]).success();
    assert_eq!(
        world
            .project
            .git(&["show", &format!("{}:assets/logo.svg", world.ref_name())]),
        "initial"
    );
}

#[test]
fn evaluated_output_keys_wait_for_the_first_file_and_ignore_the_source_root() {
    let manifest = "name = 'once'\nroot = 'scaffold'\n[questions.enabled]\ntype = 'boolean'\ndefault = false\n[questions.slug]\ntype = 'string'\ndefault = 'demo'\n[files]\n'{{ slug }}/logo' = { once = true }\n";
    let world = World::with_template("name = 'once'\n", &[("keep", "keep\n")]);
    world.template.repo.write("template.toml", manifest);
    world.template.repo.write("scaffold/keep", "keep\n");
    world.template.repo.write(
        "scaffold/{% if enabled %}{{ slug }}{% endif %}/logo.jinja",
        "{{ slug }}\n",
    );
    world
        .template
        .repo
        .commit_all("add conditional output under custom root");
    world.init(&[]).success();
    assert!(!world.project.exists("demo/logo"));
    tpl(
        &world.project,
        &["update", "--defaults", "--answer", "enabled=true"],
    )
    .success();
    let initial = blob(&world, "demo/logo");
    tpl(
        &world.project,
        &["update", "--defaults", "--answer", "enabled=false"],
    )
    .success();
    assert_eq!(blob(&world, "demo/logo"), initial);
    tpl(
        &world.project,
        &[
            "update",
            "--defaults",
            "--answer",
            "enabled=true",
            "--answer",
            "slug=second",
        ],
    )
    .success();
    assert_eq!(blob(&world, "demo/logo"), initial);
    assert_eq!(
        world
            .project
            .git(&["show", &format!("{}:second/logo", world.ref_name())]),
        "second"
    );
}

#[test]
fn invalid_declarations_and_retained_path_collisions_leave_state_untouched() {
    for path in [
        "",
        "/absolute",
        "../escape",
        "a//b",
        "a/./b",
        "a\\b",
        ".git/config",
        "C:/drive",
    ] {
        let world = world();
        world.init(&[]).success();
        let key = toml::Value::String(path.into()).to_string();
        world.template.repo.write(
            "template.toml",
            &format!("name = 'once'\n[files]\n{key} = {{ once = true }}\n"),
        );
        world.template.repo.commit_all("add unsafe declaration");
        let before = world.project.working_state();
        let tip = world.project.rev_parse(&world.ref_name());
        assert_eq!(
            tpl(&world.project, &["--json", "update", "--defaults"])
                .failure()
                .error_code(),
            "tpl::render::file_path"
        );
        assert_eq!(world.project.working_state(), before);
        assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
    }
    let world = world();
    world.init(&[]).success();
    world.template.repo.remove("template/assets/logo.svg.jinja");
    world
        .template
        .repo
        .write("template/assets/logo.svg/child", "collision\n");
    world.template.repo.commit_all("collide with retained file");
    let before = world.project.working_state();
    let tip = world.project.rev_parse(&world.ref_name());
    assert_eq!(
        tpl(&world.project, &["--json", "update", "--defaults"])
            .failure()
            .error_code(),
        "tpl::render::collision"
    );
    assert_eq!(world.project.working_state(), before);
    assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
}

#[test]
fn previews_keep_captured_paths_and_fresh_renders_use_current_sources() {
    let world = world();
    world.init(&[]).success();
    world
        .template
        .repo
        .write("template/assets/logo.svg.jinja", "new logo\n");
    world
        .template
        .repo
        .commit_all("replace template placeholder");
    let dry = tpl(
        &world.project,
        &["--json", "update", "--defaults", "--dry-run"],
    )
    .success()
    .json();
    assert_eq!(dry["result"], "upToDate");
    let before = world.project.working_state();
    tpl(&world.project, &["diff", "--dirty"]).success();
    assert_eq!(world.project.working_state(), before);
    let output = world.dir.path().join("fresh-output");
    tpl(
        &world.project,
        &[
            "render",
            &world.template.source(),
            "--defaults",
            "-o",
            output.to_str().unwrap(),
        ],
    )
    .success();
    assert_eq!(
        std::fs::read_to_string(output.join("assets/logo.svg")).unwrap(),
        "new logo\n"
    );
}

#[test]
fn binary_entries_keep_their_original_blob_and_executable_mode() {
    let world = World::with_template(
        "name = 'once'\n[files]\n'asset' = { once = true }\n",
        &[("asset", "initial")],
    );
    std::fs::write(
        world.template.repo.path.join("template/asset"),
        b"\0initial\xff",
    )
    .unwrap();
    world.template.repo.make_executable("template/asset");
    world
        .template
        .repo
        .commit_all("initialize executable binary");
    world.init(&[]).success();
    let entry = world.project.git(&["ls-tree", &world.ref_name(), "asset"]);
    assert!(entry.starts_with("100755"));
    world
        .template
        .repo
        .write("template/asset", "new plain file");
    world
        .template
        .repo
        .git(&["update-index", "--chmod=-x", "template/asset"]);
    world
        .template
        .repo
        .commit_all("change asset bytes and mode");
    tpl(&world.project, &["update", "--defaults"]).success();
    assert_eq!(
        world.project.git(&["ls-tree", &world.ref_name(), "asset"]),
        entry
    );
}

#[test]
fn forced_init_and_source_switch_preserve_capture_on_the_same_ref() {
    let world = world();
    world.init(&["--id", "stable"]).success();
    let initial = world.project.rev_parse("refs/tpl/stable:assets/logo.svg");
    world
        .template
        .repo
        .write("template/assets/logo.svg.jinja", "new logo\n");
    world.template.repo.commit_all("change placeholder");
    world.init(&["--force", "--id", "stable"]).success();
    assert_eq!(
        world.project.rev_parse("refs/tpl/stable:assets/logo.svg"),
        initial
    );
    let replacement = common::Repo::init("replacement");
    replacement.write("template.toml", "name = 'replacement'\n");
    replacement.write("template/ordinary", "replacement\n");
    replacement.commit_all("create unrelated template");
    tpl(
        &world.project,
        &["switch", replacement.path.to_str().unwrap(), "--defaults"],
    )
    .success();
    assert_eq!(
        world.project.rev_parse("refs/tpl/stable:assets/logo.svg"),
        initial
    );
}

#[test]
fn migration_moves_cannot_move_initialized_paths_even_during_preflight() {
    let world = world();
    world.init(&[]).success();
    world.add_migration(
        "001-move.toml",
        "[[moves]]\nfrom = 'assets/logo.svg'\nto = 'renamed.svg'\n",
    );
    let before = world.project.working_state();
    let tip = world.project.rev_parse(&world.ref_name());
    for args in [
        vec!["--json", "update", "--defaults", "--dry-run"],
        vec!["--json", "update", "--defaults"],
    ] {
        assert_eq!(
            tpl(&world.project, &args).failure().error_code(),
            "tpl::ops::once_move"
        );
        assert_eq!(world.project.working_state(), before);
        assert_eq!(world.project.rev_parse(&world.ref_name()), tip);
    }
}

#[test]
fn declaration_expressions_cannot_silently_alias_and_jinja_is_a_literal_output_suffix() {
    let world = World::with_template(
        "name = 'once'\n[files]\n'asset' = { once = true }\n\"{{ 'asset' }}\" = { once = true }\n",
        &[("asset", "initial")],
    );
    assert_eq!(
        world.init(&["--json"]).failure().error_code(),
        "tpl::render::collision"
    );
    let world = World::with_template(
        "name = 'once'\n[files]\n'asset.jinja' = { once = true }\n",
        &[("asset.jinja.jinja", "initial")],
    );
    world.init(&[]).success();
    world
        .template
        .repo
        .write("template/asset.jinja.jinja", "new");
    world
        .template
        .repo
        .commit_all("change double-suffixed source");
    tpl(&world.project, &["update", "--defaults"]).success();
    assert_eq!(
        world
            .project
            .git(&["show", &format!("{}:asset.jinja", world.ref_name())]),
        "initial"
    );
}

#[test]
fn retained_output_is_not_mapped_to_a_fabricated_backport_source() {
    let world = world();
    world.init(&[]).success();
    world.template.repo.remove("template/assets/logo.svg.jinja");
    world.template.repo.write("template/ordinary", "second\n");
    world.template.repo.commit_all("remove captured source");
    tpl(&world.project, &["update", "--defaults"]).success();
    tpl(&world.project, &["merge"]).success();
    world.project.write("assets/logo.svg", "user replacement\n");
    assert_eq!(
        tpl(&world.project, &["--json", "backport", "assets/logo.svg"])
            .failure()
            .error_code(),
        "tpl::backport::one_shot"
    );
}

#[cfg(unix)]
#[test]
fn captured_symlinks_keep_their_initial_target_and_git_mode() {
    let world = world();
    world.template.repo.remove("template/assets/logo.svg.jinja");
    std::os::unix::fs::symlink(
        "first-target",
        world.template.repo.path.join("template/assets/logo.svg"),
    )
    .unwrap();
    world.template.repo.commit_all("use symlink placeholder");
    world.init(&[]).success();
    let entry = world
        .project
        .git(&["ls-tree", &world.ref_name(), "assets/logo.svg"]);
    assert!(entry.starts_with("120000"));
    world.template.repo.remove("template/assets/logo.svg");
    world
        .template
        .repo
        .write("template/assets/logo.svg", "now a regular file\n");
    world
        .template
        .repo
        .commit_all("change link kind and target");
    tpl(&world.project, &["update", "--defaults"]).success();
    assert_eq!(
        world
            .project
            .git(&["ls-tree", &world.ref_name(), "assets/logo.svg"]),
        entry
    );
}

#[test]
fn capture_survives_removing_the_entire_source_root_and_declaration() {
    let world = World::with_template(
        "name = 'once'\n[files]\n'asset' = { once = true }\n",
        &[("asset", "initial")],
    );
    world.init(&[]).success();
    let initial = blob(&world, "asset");
    world.template.repo.remove("template/asset");
    world
        .template
        .repo
        .write("template.toml", "name = 'once'\n");
    world.template.repo.commit_all("remove last source");
    tpl(&world.project, &["update", "--defaults"]).success();
    assert_eq!(blob(&world, "asset"), initial);
}
