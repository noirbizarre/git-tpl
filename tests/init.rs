//! `git tpl init` — attaching a template and the initial merge.

mod common;

use common::{World, tpl};

#[test]
fn init_renders_the_template_and_merges_it_into_the_branch() {
    let world = World::new();

    world.init(&[]).success();

    assert!(world.project.has_ref(&world.ref_name()));
    assert_eq!(
        world.project.read("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\n"
    );
    assert_eq!(
        world.project.read("README.md"),
        "# demo\n\nLicensed under MIT License.\n"
    );
}

/// The load-bearing step. Without it the template commit is not an ancestor of
/// the branch, so the *first* update would have no merge base and would
/// conflict on every line of every file.
#[test]
fn the_template_commit_becomes_an_ancestor_of_the_branch() {
    let world = World::new();
    world.init(&[]).success();

    let template_commit = world.project.rev_parse(&world.ref_name());
    let (is_ancestor, _) =
        world
            .project
            .try_git(&["merge-base", "--is-ancestor", &template_commit, "HEAD"]);

    assert!(
        is_ancestor,
        "the rendered commit must be reachable from HEAD, or the first update conflicts on everything"
    );
}

#[test]
fn the_projects_own_files_survive_the_merge() {
    let world = World::new();
    world.init(&[]).success();

    assert_eq!(
        world.project.read("NOTES.md"),
        "Pre-existing project notes.\n"
    );
}

/// A fresh clone must be understandable from `.config/git.tpl.toml` alone, so
/// leaving it untracked would mean the attachment existed only on the machine
/// that ran `init`.
#[test]
fn the_configuration_is_written_and_committed() {
    let world = World::new();
    world.init(&[]).success();

    let tracked = world.project.git(&["ls-files", ".config/git.tpl.toml"]);
    assert_eq!(tracked, ".config/git.tpl.toml");
    assert_eq!(
        world.project.status(),
        "",
        "the worktree must be left clean"
    );

    let config = world.project.read(".config/git.tpl.toml");
    assert!(config.contains("[template]"), "{config}");
    assert!(config.contains("project_name = \"demo\""), "{config}");
    assert!(config.contains("license = \"MIT\""), "{config}");
}

/// Only answers are recorded. Computed values are a function of them and are
/// recomputed on every render.
#[test]
fn computed_values_are_not_recorded_as_answers() {
    let world = World::new();
    world.init(&[]).success();

    let config = world.project.read(".config/git.tpl.toml");
    assert!(
        !config.contains("package_name"),
        "a computed value must not be recorded:\n{config}"
    );
}

#[test]
fn the_rendered_commit_carries_its_provenance() {
    let world = World::new();
    world.init(&[]).success();

    let message = world.project.commit_message(&world.ref_name());

    assert!(
        message.starts_with("tpl: render rust-library at"),
        "{message}"
    );
    assert!(message.contains("Template-Source:"), "{message}");
    assert!(message.contains("Template-Ref: main"), "{message}");
    assert!(message.contains("Template-Commit:"), "{message}");
    assert!(message.contains("Answers-Digest: sha256:"), "{message}");
    assert!(
        message.contains("Data-Source: licenses = template:data/licenses.toml@"),
        "{message}"
    );
    assert!(message.contains("Tpl-Version:"), "{message}");
}

/// Provenance is in trailers so that plain Git can read it back.
#[test]
fn provenance_is_readable_with_plain_git() {
    let world = World::new();
    world.init(&[]).success();

    let commit = world.project.git(&[
        "log",
        "-1",
        "--format=%(trailers:key=Template-Commit,valueonly)",
        &world.ref_name(),
    ]);

    assert_eq!(commit.trim(), world.template.repo.rev_parse("HEAD"));
}

/// The tree is exactly the rendered files. A provenance file in it would appear
/// in every diff and conflict on every update.
#[test]
fn the_rendered_tree_holds_only_rendered_files() {
    let world = World::new();
    world.init(&[]).success();

    let paths = world.project.tree_paths(&world.ref_name());

    assert_eq!(
        paths,
        ["Cargo.toml", "README.md", "ci.yml", "run.sh", "src/lib.rs"]
    );
}

/// Only the template's `root` subtree is rendered, so a template repository can
/// carry its own README and CI without them reaching every project.
#[test]
fn the_templates_own_files_are_not_rendered_into_the_project() {
    let world = World::new();
    world.init(&[]).success();

    assert_eq!(
        world.project.read("README.md"),
        "# demo\n\nLicensed under MIT License.\n",
        "the template's own README must not overwrite the rendered one"
    );
    assert!(!world.project.exists("template.toml"));
    assert!(!world.project.exists("data/licenses.toml"));
}

/// GitHub Actions files are full of `${{ }}`, and a tool that rendered every
/// file would mangle them.
#[test]
fn a_non_jinja_file_is_copied_byte_for_byte() {
    let world = World::new();
    world.init(&[]).success();

    assert!(
        world.project.read("ci.yml").contains("${{ github.sha }}"),
        "a plain file must be copied verbatim"
    );
}

#[cfg(unix)]
#[test]
fn the_executable_bit_survives_rendering() {
    let world = World::new();
    world.init(&[]).success();

    let mode = world.project.git(&["ls-files", "-s", "run.sh"]);
    assert!(mode.starts_with("100755"), "{mode}");
}

/// The cleanest possible history for a project generated from a template.
#[test]
fn init_in_an_empty_repository_needs_no_merge() {
    let world = World::empty_project();

    world.init(&[]).success();

    assert!(world.project.exists("Cargo.toml"));
    assert!(world.project.has_ref(&world.ref_name()));

    let template_commit = world.project.rev_parse(&world.ref_name());
    let (is_ancestor, _) =
        world
            .project
            .try_git(&["merge-base", "--is-ancestor", &template_commit, "HEAD"]);
    assert!(is_ancestor);
}

#[test]
fn a_supplied_answer_is_used_instead_of_the_default() {
    let world = World::new();

    world
        .init(&[
            "--answer",
            "project_name=My Great Project",
            "--answer",
            "license=Apache-2.0",
        ])
        .success();

    assert!(
        world
            .project
            .read("Cargo.toml")
            .contains("name = \"my-great-project\"")
    );
    assert!(
        world
            .project
            .read("Cargo.toml")
            .contains("license = \"Apache-2.0\"")
    );
    assert!(
        world
            .project
            .read("README.md")
            .contains("Apache License 2.0")
    );
}

#[test]
fn an_answer_outside_the_offered_choices_is_refused() {
    let world = World::new();

    world
        .init(&["--answer", "license=GPL-3.0"])
        .failure()
        .says("GPL-3.0");
}

/// A project has one template. Re-running would silently discard the recorded
/// answers.
#[test]
fn init_refuses_to_run_twice() {
    let world = World::new();
    world.init(&[]).success();

    world
        .init(&[])
        .failure()
        .says("already has a template attached");
}

#[test]
fn no_merge_creates_the_ref_and_leaves_the_branch_alone() {
    let world = World::new();
    let before = world.project.rev_parse("HEAD");

    world.init(&["--no-merge"]).success();

    assert!(world.project.has_ref(&world.ref_name()));
    assert!(
        !world.project.exists("Cargo.toml"),
        "the rendered files must not reach the worktree"
    );

    // The configuration is still committed — that is the record of the
    // attachment, and it is a separate concern from the merge.
    let after = world.project.rev_parse("HEAD");
    assert_ne!(before, after);
    assert_eq!(world.project.status(), "");
}

/// `init` performs a merge, and a merge needs a clean worktree. Refused before
/// the questionnaire rather than after it.
#[test]
fn init_refuses_a_dirty_worktree() {
    let world = World::new();
    world.project.write("NOTES.md", "uncommitted change\n");

    world.init(&[]).failure().says("uncommitted changes");
}

#[test]
fn a_dry_run_creates_nothing() {
    let world = World::new();
    let before = world.project.working_state();

    world
        .init(&["--dry-run"])
        .success()
        .says("Nothing was created");

    assert_eq!(world.project.working_state(), before);
    assert!(!world.project.has_ref(&world.ref_name()));
    assert!(!world.project.exists(".config/git.tpl.toml"));
}

/// The cheapest way to find a cycle or a typo, since both are caught when the
/// graph is built rather than when a question is reached.
#[test]
fn a_dry_run_lists_the_questions_in_resolution_order() {
    let world = World::new();

    let output = world.init(&["--dry-run"]).success();

    let text = output.all();
    let licenses = text.find("licenses").expect("data source listed");
    let project_name = text.find("project_name").expect("question listed");
    let package_name = text.find("package_name").expect("computed value listed");

    assert!(
        licenses < project_name || text.contains("license"),
        "the data source must be listed:\n{text}"
    );
    assert!(
        project_name < package_name,
        "a computed value must follow what it depends on:\n{text}"
    );
}

#[test]
fn an_explicit_id_determines_the_ref_name() {
    let world = World::new();

    world.init(&["--id", "my-template"]).success();

    assert!(world.project.has_ref("refs/tpl/my-template"));
    assert!(!world.project.has_ref("refs/tpl/template"));
}

#[test]
fn a_missing_template_is_reported_clearly() {
    let world = World::new();

    tpl(
        &world.project,
        &["init", "/definitely/not/a/template", "--defaults"],
    )
    .failure();
}

#[test]
fn a_directory_without_a_manifest_is_not_a_template() {
    let world = World::new();
    let bare = common::Repo::init_in(world.dir.path(), "not-a-template");
    bare.write("README.md", "nothing here\n");
    bare.commit_all("chore: initial");

    tpl(
        &world.project,
        &["init", &bare.path.to_string_lossy(), "--defaults"],
    )
    .failure()
    .says("template.toml");
}
