//! `--answers-from`: answers read from a file.
//!
//! One flag serving four unrelated jobs — a migration from another generator,
//! shared house values, a render with no terminal, and a template's own
//! fixtures. Each is a test below.

mod common;

use common::{World, tpl};

/// Write an answers file *outside* the project, so that supplying answers never
/// dirties the worktree `init` requires to be clean.
fn answers_file(world: &World, name: &str, content: &str) -> String {
    let path = world.dir.path().join(name);
    std::fs::write(&path, content).expect("write answers file");
    path.to_string_lossy().into_owned()
}

#[test]
fn answers_from_a_file_are_used_instead_of_the_defaults() {
    let world = World::new();
    let path = answers_file(
        &world,
        "answers.toml",
        "project_name = \"My Great Project\"\nlicense = \"Apache-2.0\"\n",
    );

    world.init(&["--answers-from", &path]).success();

    let manifest = world.project.read("Cargo.toml");
    assert!(
        manifest.contains("name = \"my-great-project\""),
        "{manifest}"
    );
    assert!(manifest.contains("license = \"Apache-2.0\""), "{manifest}");
}

/// The file is a set of values, not a decision. A flag typed on the same
/// command line is more specific and wins.
#[test]
fn an_answer_flag_beats_the_same_key_in_a_file() {
    let world = World::new();
    let path = answers_file(&world, "answers.toml", "project_name = \"from-file\"\n");

    world
        .init(&[
            "--answers-from",
            &path,
            "--answer",
            "project_name=from-flag",
        ])
        .success();

    assert!(
        world.project.read("README.md").contains("# from-flag"),
        "{}",
        world.project.read("README.md")
    );
}

/// House defaults first, then the project's own file on top of them.
#[test]
fn a_later_answers_file_beats_an_earlier_one() {
    let world = World::new();
    let house = answers_file(
        &world,
        "house.toml",
        "project_name = \"house\"\nlicense = \"Apache-2.0\"\n",
    );
    let project = answers_file(&world, "project.toml", "project_name = \"specific\"\n");

    world
        .init(&["--answers-from", &house, "--answers-from", &project])
        .success();

    let manifest = world.project.read("Cargo.toml");
    assert!(manifest.contains("name = \"specific\""), "{manifest}");
    // Untouched by the second file, so the first still supplies it.
    assert!(manifest.contains("license = \"Apache-2.0\""), "{manifest}");
}

/// Ignored rather than fatal, because a real answers file carries keys this
/// template never had. Reported rather than silent, because otherwise a typo
/// looks exactly like an answer that had no effect.
#[test]
fn a_key_naming_no_question_is_ignored_and_reported() {
    let world = World::new();
    let path = answers_file(
        &world,
        "answers.toml",
        "project_name = \"thing\"\nlegacy_option = \"dropped\"\n",
    );

    world
        .init(&["--answers-from", &path])
        .success()
        .says("answers ignored")
        .says("legacy_option");

    // The recorded answers hold what the template asked for, and nothing else.
    let config = world.project.read(".config/git.tpl.toml");
    assert!(!config.contains("legacy_option"), "{config}");
}

/// The case that motivated the flag: a `.copier-answers.yml` works unedited,
/// `_src_path` and all.
#[test]
fn a_copier_answers_file_renders_without_editing() {
    let world = World::new();
    let path = answers_file(
        &world,
        ".copier-answers.yml",
        "# Changes here will be overwritten by Copier\n\
         _commit: v1.2.0\n\
         _src_path: https://github.com/example/rust-library\n\
         project_name: ported\n\
         license: MIT\n",
    );

    world
        .init(&["--answers-from", &path])
        .success()
        .says("_src_path");

    assert!(world.project.read("README.md").contains("# ported"));
}

/// A file carries types where a flag carries only text, which is half the point
/// of having it.
#[test]
fn types_are_preserved_from_a_json_file() {
    let world = World::with_template(
        r#"
name = "typed"

[questions.port]
type = "integer"
default = 80

[questions.ci]
type = "boolean"
default = false
"#,
        &[("app.conf.jinja", "port={{ port }}\nci={{ ci }}\n")],
    );
    let path = answers_file(&world, "answers.json", r#"{"port": 8080, "ci": true}"#);

    world.init(&["--answers-from", &path]).success();

    // `True` is how MiniJinja renders a boolean; what matters here is that it
    // is a boolean at all rather than the string `"true"`.
    assert_eq!(world.project.read("app.conf"), "port=8080\nci=True\n");
}

/// The existing rule for `--answer`, applied to the file: a mismatch is an
/// error, never a coercion. Asserted by diagnostic code, never by prose.
#[test]
fn a_file_value_of_the_wrong_type_is_an_error_not_a_coercion() {
    let world = World::with_template(
        r#"
name = "typed"

[questions.port]
type = "integer"
default = 80
"#,
        &[("app.conf.jinja", "port={{ port }}\n")],
    );
    let path = answers_file(&world, "answers.toml", "port = \"eighty\"\n");

    world
        .init(&["--answers-from", &path])
        .failure()
        .says("tpl::eval::wrong_type");
}

#[test]
fn a_missing_answers_file_is_an_error_naming_it() {
    let world = World::new();
    let missing = world.dir.path().join("absent.toml");

    world
        .init(&["--answers-from", &missing.to_string_lossy()])
        .failure()
        .says("tpl::answers::read")
        .says("absent.toml");
}

#[test]
fn a_malformed_answers_file_is_reported_with_its_path() {
    let world = World::new();
    let path = answers_file(&world, "answers.toml", "project_name = \n");

    world
        .init(&["--answers-from", &path])
        .failure()
        .says("tpl::answers::parse");
}

/// A document that is neither shape is refused up front, rather than silently
/// supplying nothing.
#[test]
fn an_answers_file_that_is_not_a_table_is_refused() {
    let world = World::new();
    let path = answers_file(&world, "answers.json", "[\"a\", \"b\"]");

    world
        .init(&["--answers-from", &path])
        .failure()
        .says("tpl::answers::shape");
}

/// The nested shape, which is what a template's own fixtures will use.
#[test]
fn a_top_level_answers_table_supplies_the_answers() {
    let world = World::new();
    let path = answers_file(
        &world,
        "case.toml",
        "[answers]\nproject_name = \"fixture\"\n\n[expect]\nfiles = [\"README.md\"]\n",
    );

    world
        .init(&["--answers-from", &path])
        .success()
        // `expect` is not a key of the answers table, so it is not an answer,
        // and there is nothing to report.
        .silent_about("answers ignored");

    assert!(world.project.read("README.md").contains("# fixture"));
}

/// The CI case: no terminal, every question answered, nothing prompted.
#[test]
fn an_answers_file_supplies_a_question_that_has_no_default() {
    let world = World::with_template(
        r#"
name = "required"

[questions.owner]
type = "string"
"#,
        &[("OWNER.jinja", "{{ owner }}\n")],
    );
    let path = answers_file(&world, "answers.toml", "owner = \"axel\"\n");

    // Without the file this fails as `tpl::eval::unanswered`: `--defaults` has
    // no default to take.
    world.init(&["--answers-from", &path]).success();

    assert_eq!(world.project.read("OWNER"), "axel\n");
}

/// `update` takes the same flag, and taking it changes nothing about the ref
/// being append-only.
#[test]
fn answers_from_a_file_change_the_rendering_and_stay_append_only() {
    let world = World::new();
    world.init(&[]).success();
    let before = world.project.rev_parse(&world.ref_name());

    let path = answers_file(&world, "answers.toml", "license = \"Apache-2.0\"\n");
    tpl(
        &world.project,
        &["update", "--defaults", "--answers-from", &path],
    )
    .success();

    let after = world.project.rev_parse(&world.ref_name());
    assert_ne!(after, before);
    assert_eq!(
        world.project.rev_parse(&format!("{}^", world.ref_name())),
        before,
        "an answer change is still append-only"
    );
}

/// `--dry-run` exists to find mistakes before anything is written, so it must
/// report the same ignored keys the real run would.
#[test]
fn a_dry_run_reports_ignored_keys_too() {
    let world = World::new();
    let path = answers_file(&world, "answers.toml", "nonexistent = \"x\"\n");
    let source = world.template.source();

    tpl(
        &world.project,
        &[
            "init",
            source.as_str(),
            "--defaults",
            "--dry-run",
            "--answers-from",
            &path,
        ],
    )
    .success()
    .says("nonexistent");
}
