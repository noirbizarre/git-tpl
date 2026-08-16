//! `git tpl show` — one path, as the template renders it.

mod common;

use common::{World, tpl};

#[test]
fn show_prints_the_template_version_of_a_file() {
    let world = World::new();
    world.init(&[]).success();

    let out = tpl(&world.project, &["show", "Cargo.toml"]).success();

    // Plain Git is the oracle: `git tpl show` is documented as looking up the
    // ref name and nothing more, so anything else it did would be a surprise.
    let expected = world
        .project
        .git(&["show", &format!("{}:Cargo.toml", world.ref_name())]);
    assert_eq!(out.stdout.trim_end(), expected);
}

/// The whole point of the command. During a conflicted merge the worktree
/// holds the merged mess; the ref holds what the template actually said.
#[test]
fn show_reads_the_ref_not_the_worktree() {
    let world = World::new();
    world.init(&[]).success();

    world
        .project
        .write("Cargo.toml", "this is what the user wrote\n");
    world.project.commit_all("chore: local edit");

    let out = tpl(&world.project, &["show", "Cargo.toml"]).success();

    assert!(
        !out.stdout.contains("this is what the user wrote"),
        "read the worktree instead of the ref:\n{}",
        out.stdout
    );
    assert!(out.stdout.contains("[package]"), "{}", out.stdout);
}

#[test]
fn show_lists_a_directory() {
    let world = World::new();
    world.init(&[]).success();

    let out = tpl(&world.project, &["show", "src"]).success();

    // Root-relative, like `git tpl diff --name-only`, so the output is usable
    // as input to anything else.
    assert_eq!(out.stdout, "src/lib.rs\n");
}

#[test]
fn show_of_the_root_lists_the_whole_rendering() {
    let world = World::new();
    world.init(&[]).success();

    let out = tpl(&world.project, &["show", "."]).success();
    let listed: Vec<&str> = out.stdout.lines().collect();

    assert!(listed.contains(&"Cargo.toml"), "{listed:?}");
    assert!(listed.contains(&"src/lib.rs"), "{listed:?}");
    // The rendered ref holds the rendering and nothing else — the project's own
    // files are not in it.
    assert!(!listed.contains(&"NOTES.md"), "{listed:?}");
}

#[test]
fn show_without_a_rendered_ref_says_which_ref_is_missing() {
    let world = World::new();
    world.init(&[]).success();
    // The case a clone reproduces: the configuration is committed, the ref is
    // not, because template refs are never pushed automatically.
    world.project.git(&["update-ref", "-d", &world.ref_name()]);

    tpl(&world.project, &["show", "Cargo.toml"])
        .failure()
        .says("tpl::ops::no_rendered_ref");
}

#[test]
fn show_without_a_template_says_the_repository_has_none() {
    let world = World::new();

    tpl(&world.project, &["show", "Cargo.toml"])
        .failure()
        .says("tpl::config::missing");
}

/// Asserted by diagnostic code, not by prose: the code is the stable surface,
/// and pinning the message would make every wording improvement a breaking
/// change.
#[test]
fn an_absent_path_names_the_ref_and_suggests_diff() {
    let world = World::new();
    world.init(&[]).success();

    tpl(&world.project, &["show", "nope.txt"])
        .code(1)
        .says("tpl::ops::no_such_path")
        .says(&world.ref_name());
}

#[test]
fn a_path_leaving_the_rendering_is_refused_rather_than_resolved() {
    let world = World::new();
    world.init(&[]).success();

    tpl(&world.project, &["show", "../Cargo.toml"])
        .failure()
        .says("tpl::ops::invalid_argument");
}

/// Stdout is data. A decorated heading would break `git tpl show x > x` and
/// every pipe anyone writes.
#[test]
fn show_writes_the_content_and_nothing_else() {
    let world = World::new();
    world.init(&[]).success();

    let out = tpl(&world.project, &["show", "Cargo.toml"]).success();

    assert_eq!(out.stderr, "", "stderr should be empty on success");
    assert!(out.stdout.starts_with("[package]"), "{}", out.stdout);
}

/// `show` reads. It must be as inert as `diff`.
#[test]
fn show_does_not_touch_head_the_index_or_the_worktree() {
    let world = World::new();
    world.init(&[]).success();

    let before = world.project.working_state();
    tpl(&world.project, &["show", "Cargo.toml"]).success();
    let after = world.project.working_state();

    assert_eq!(before.head, after.head, "HEAD moved");
    assert_eq!(before.index, after.index, "the index changed");
    assert_eq!(before.worktree, after.worktree, "the worktree changed");
}

/// The documented exemption from `--json`: this command's stdout *is* the
/// payload — the file's bytes — and wrapping it in an envelope would mean
/// nothing could read it.
#[test]
fn show_writes_raw_bytes_even_under_json() {
    let world = World::new();
    world.init(&[]).success();

    let plain = tpl(&world.project, &["show", "Cargo.toml"]).success();
    let json = tpl(&world.project, &["--json", "show", "Cargo.toml"]).success();

    assert_eq!(plain.stdout, json.stdout);
    assert!(
        !json.stdout.contains("\"ok\""),
        "show wrapped its output:\n{}",
        json.stdout
    );
}
