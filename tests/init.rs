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
/// the branch, so the *first* update would have no merge base — and without one
/// Git cannot tell the user's edits from the template's, so everything that
/// differs conflicts. Demonstrated by
/// `without_a_merge_base_a_customisation_conflicts_the_template_never_touched`.
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
        "the rendered commit must be reachable from HEAD, or the first update has no merge base"
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
fn a_non_jinja_file_reaches_the_project_byte_for_byte() {
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

// --- adopting a project that already exists ---------------------------------
//
// `init` on a populated repository is how a project generated by another tool,
// or written by hand and since grown to resemble a template, is brought under
// git-tpl. There is no separate command, because there is no separate
// behaviour: the orphan commit is merged, and Git reconciles the two sides.
//
// These tests exist because that path had none, and its absence let
// ADR-009 claim for a long time that the merge "conflicts on every line of
// every file". It does not, and `a_file_that_differs_conflicts_only_on_the
// _differing_lines` is the test that says so.

/// The project as someone else's generator left it: the same files the template
/// renders, one of them edited since.
fn populated_project() -> World {
    let world = World::new();
    // Byte-identical to what the template renders with the default answers.
    world.project.write("src/lib.rs", "//! demo\n");
    // Differs from the rendered version by exactly one line.
    world.project.write(
        "README.md",
        "# demo\n\nLicensed under Apache License 2.0.\n",
    );
    world
        .project
        .commit_all("chore: the project as another tool left it");
    world
}

#[test]
fn a_file_identical_on_both_sides_does_not_conflict() {
    let world = populated_project();
    world.init(&[]).success();

    // No merge base, and still no conflict: Git compares content, and identical
    // content has nothing to reconcile.
    assert!(
        !world.project.read("src/lib.rs").contains("<<<<<<<"),
        "an identical file was reported as a conflict"
    );
    assert!(
        !world.project.status().contains("src/lib.rs"),
        "an identical file was left unresolved"
    );
}

#[test]
fn a_file_that_differs_conflicts_only_on_the_differing_lines() {
    let world = populated_project();
    world.init(&[]).success();

    let readme = world.project.read("README.md");
    assert!(readme.contains("<<<<<<<"), "expected a conflict");

    // The load-bearing assertion. An unrelated-histories merge does a real
    // line-level diff, so the heading the two sides agree on stays outside the
    // markers. If this ever fails, the whole "adopt an existing project" story
    // fails with it, because a user would face whole-file conflicts instead of
    // a three-line one.
    let conflict = readme
        .split("<<<<<<<")
        .nth(1)
        .expect("a conflict block")
        .split(">>>>>>>")
        .next()
        .expect("a closing marker");

    assert!(
        !conflict.contains("# demo"),
        "the agreed heading was swallowed by the conflict:\n{readme}"
    );
    assert!(conflict.contains("Apache License 2.0"), "ours is missing");
    assert!(conflict.contains("MIT License"), "theirs is missing");
}

#[test]
fn a_template_file_the_project_lacks_is_added_and_staged() {
    let world = populated_project();
    world.init(&[]).success();

    // Added by the merge itself, with no flag and no prompt. This is why
    // bringing the template's content in needs no machinery of our own.
    assert!(world.project.exists("ci.yml"));
    assert!(
        world.project.status().contains("ci.yml"),
        "the added file was not staged: {}",
        world.project.status()
    );
}

#[test]
fn the_projects_own_files_are_untouched_when_the_merge_conflicts() {
    let world = populated_project();
    world.init(&[]).success();

    // The template does not render this, so nothing may happen to it — least of
    // all in a merge the user is midway through resolving.
    assert_eq!(
        world.project.read("NOTES.md"),
        "Pre-existing project notes.\n"
    );
}

#[test]
fn resolving_the_first_merge_leaves_the_next_update_clean() {
    let world = populated_project();
    world.init(&[]).success();

    // Resolve exactly as the user would: plain Git, no git-tpl involvement.
    world.project.write(
        "README.md",
        "# demo\n\nLicensed under Apache License 2.0.\n",
    );
    world.project.commit_all("chore: adopt the template");

    // The template moves on.
    world.template.repo.write(
        "template/src/lib.rs.jinja",
        "//! {{ project_name }}\n\npub fn added() {}\n",
    );
    world.template.repo.commit_all("feat: add a function");

    tpl(&world.project, &["update"]).success();
    tpl(&world.project, &["merge"]).success();

    // The whole point of merging the orphan commit: `G0` is now a merge base,
    // so the second merge is a two-line diff rather than a repeat of the first.
    assert_eq!(world.project.status(), "", "the second merge conflicted");
    assert!(world.project.read("src/lib.rs").contains("pub fn added()"));
    // The resolution survived: the template did not win the licence back.
    assert!(
        world
            .project
            .read("README.md")
            .contains("Apache License 2.0")
    );
}

/// The claim ADR-009 rests on, demonstrated rather than asserted.
///
/// Without the merge, `G0` is not an ancestor of the branch, so Git cannot tell
/// the user's edits from the template's. A file the user customised and the
/// template never touched still conflicts — which is the cost the merge buys
/// off, and the reason `init` performs one.
///
/// The other half of the pair is
/// `resolving_the_first_merge_leaves_the_next_update_clean`, where the same
/// customisation survives untouched because the merge base exists.
#[test]
fn without_a_merge_base_a_customisation_conflicts_the_template_never_touched() {
    let world = World::new();
    world.init(&["--no-merge"]).success();

    // What a generator does: the files arrive on the branch with no
    // relationship to `G0`.
    let ref_name = world.ref_name();
    world.project.git(&["checkout", &ref_name, "--", "."]);
    world.project.commit_all("chore: take the rendered files");

    // A deliberate customisation, in a file the template will not change.
    world
        .project
        .write("README.md", "# demo\n\nOur own README.\n");
    world.project.commit_all("docs: our own README");

    // The template changes something else entirely.
    world.template.repo.write(
        "template/src/lib.rs.jinja",
        "//! {{ project_name }}\n\npub fn added() {}\n",
    );
    world.template.repo.commit_all("feat: add a function");

    tpl(&world.project, &["update"]).success();
    let out = tpl(&world.project, &["merge"]);

    let status = world.project.status();
    assert!(
        status.contains("README.md"),
        "expected the untouched customisation to conflict; status was:\n{status}\n{}",
        out.stderr
    );
}

/// A data source in the *project*, not the template — `./` is the marker.
///
/// This is the case where the template asks a question whose choices the
/// project owns: a house list of licences or environments that the template
/// must not carry. The file is read relative to the project root, and never
/// relative to the process's working directory — which would make the same
/// template, answers and revision render differently depending on where the
/// command was run from.
#[test]
fn a_project_local_data_source_is_read_relative_to_the_project_root() {
    let world = common::World::with_template(
        r#"
name = "house-rules"

[data.house]
source = "./house.toml"

[questions.env]
type = "choice"
choices_from = "data.house.envs"
default = "staging"
"#,
        &[("env.txt.jinja", "{{ env }}\n")],
    );

    world
        .project
        .write("house.toml", "envs = [\"staging\", \"production\"]\n");
    world.project.commit_all("chore: declare the house envs");

    world.init(&[]).success();

    assert_eq!(world.project.read("env.txt"), "staging\n");
}

/// `../../../etc/passwd` in a template repository is untrusted input asking to
/// read a file outside the project. It is rejected rather than resolved: a
/// canonicalising fix would still read the file when the path stayed inside
/// after resolution, which is not the property wanted.
#[test]
fn a_project_local_data_source_may_not_escape_the_project_root() {
    let world = common::World::with_template(
        r#"
name = "nosy"

[data.secrets]
source = "../outside.toml"

[questions.leak]
type = "choice"
choices_from = "data.secrets.values"
default = "a"
"#,
        &[("out.txt.jinja", "{{ leak }}\n")],
    );

    // A real file just outside the project, so the test fails for the right
    // reason: refused, not merely absent.
    std::fs::write(
        world.dir.path().join("outside.toml"),
        "values = [\"a\", \"b\"]\n",
    )
    .expect("write the file outside the project");

    let output = tpl(
        &world.project,
        &["init", &world.template.source(), "--defaults"],
    )
    .failure();

    output.says("tpl::data::escapes_root");
}

/// Re-attaching was refused outright, so changing an answer meant editing
/// `.config/git.tpl.toml` by hand or starting the repository over. The ref is
/// append-only, so another rendering on it is exactly what `update` writes —
/// the only thing `--force` adds is asking the questions again.
#[test]
fn force_re_renders_over_an_existing_attachment() {
    let world = World::new();
    world.init(&["--answer", "project_name=first"]).success();

    world
        .init(&[])
        .failure()
        .says("already has a template attached");

    tpl(
        &world.project,
        &[
            "init",
            &world.template.source(),
            "--defaults",
            "--force",
            "--answer",
            "project_name=second",
        ],
    )
    .success();

    assert!(world.project.read("README.md").contains("second"));
}
