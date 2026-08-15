//! `~/.config/git-tpl/config.toml`, end to end.
//!
//! The user configuration is the one input that is never versioned and never
//! shared, so most of what these tests defend is what it must *not* be able to
//! do: reach a rendered tree, or leak a machine-local URL into a project.

mod common;

use common::{World, tpl};

#[test]
fn an_absent_user_configuration_is_not_an_error() {
    let world = World::new();
    // Nothing has written one, and the harness points `$XDG_CONFIG_HOME` at an
    // empty directory. The overwhelming majority of runs look like this.
    tpl(
        &world.project,
        &["init", &world.template.source(), "--defaults"],
    )
    .success();
}

#[test]
fn a_malformed_user_configuration_names_the_file_and_the_line() {
    let world = World::new();
    world.project.user_config("[shortcuts\ngh = \"x\"\n");

    tpl(
        &world.project,
        &["init", &world.template.source(), "--defaults"],
    )
    .failure()
    .says("tpl::userconfig::parse")
    .says("config.toml");
}

#[test]
fn an_unknown_section_in_the_user_configuration_is_refused() {
    let world = World::new();
    // Nothing generates this file, so an unrecognised key is a typo — and a
    // silently ignored `[defualts]` is an afternoon wasted.
    world.project.user_config("[defualts]\nauthor = \"x\"\n");

    tpl(
        &world.project,
        &["init", &world.template.source(), "--defaults"],
    )
    .failure()
    .says("tpl::userconfig::parse");
}

#[test]
fn a_shortcut_named_like_a_scheme_is_refused_when_the_file_is_read() {
    let world = World::new();
    world
        .project
        .user_config("[shortcuts]\nhttps = \"https://example.invalid/\"\n");

    // Refused on any command that reads the file, not only on one that would
    // have expanded a URL: the day it is written is when it should fail.
    tpl(
        &world.project,
        &["init", &world.template.source(), "--defaults"],
    )
    .failure()
    .says("tpl::userconfig::shortcut")
    .says("https");
}

// --- [defaults] -------------------------------------------------------------

/// A template whose only question has an answer the user would rather state
/// once than retype in every project.
const SEEDED: &str = r#"
name = "seeded"

[questions.author]
type = "string"
default = "anonymous"
default_from = "git:user.name"
"#;

/// The claim the whole design rests on, and the reason `[defaults]` is a seed
/// rather than an answer.
///
/// Under `--defaults` nobody confirms anything, so a value from this machine
/// must not reach the tree. If it did, the same template revision with the same
/// recorded answers would render two different trees on two machines, and
/// "an unchanged template produces no commit" would stop being true.
#[test]
fn user_defaults_do_not_apply_when_questions_are_not_asked() {
    let world = World::with_template(SEEDED, &[("AUTHORS.jinja", "{{ author }}\n")]);
    world
        .project
        .user_config("[defaults]\nauthor = \"Axel Haustant\"\n");

    let output = world.init(&[]).success();

    assert_eq!(world.project.read("AUTHORS"), "anonymous\n");
    assert!(
        world
            .project
            .read(".config/git.tpl.toml")
            .contains("author = \"anonymous\""),
        "the recorded answer must be the template's own default"
    );
    output.silent_about("Axel Haustant");
}

/// Two machines, two `[defaults]` files, one tree. The observable form of the
/// same claim.
#[test]
fn two_machines_with_different_user_defaults_render_the_same_tree() {
    let one = World::with_template(SEEDED, &[("AUTHORS.jinja", "{{ author }}\n")]);
    let two = World::with_template(SEEDED, &[("AUTHORS.jinja", "{{ author }}\n")]);
    one.project
        .user_config("[defaults]\nauthor = \"Ada Lovelace\"\n");
    two.project
        .user_config("[defaults]\nauthor = \"Grace Hopper\"\n");

    one.init(&[]).success();
    two.init(&[]).success();

    assert_eq!(
        one.project
            .git(&["rev-parse", &format!("{}^{{tree}}", one.ref_name())]),
        two.project
            .git(&["rev-parse", &format!("{}^{{tree}}", two.ref_name())]),
    );
}

/// This file is written once for every template the user will ever generate, so
/// it is *expected* to name questions a given template does not have. Reporting
/// that on every run would be noise — unlike an `--answers-from` key, which was
/// supplied for this template and is therefore a typo.
#[test]
fn a_user_default_naming_no_question_is_neither_fatal_nor_reported() {
    let world = World::with_template(SEEDED, &[("AUTHORS.jinja", "{{ author }}\n")]);
    world
        .project
        .user_config("[defaults]\nauthor = \"Axel\"\nlicence = \"MIT\"\n");

    world.init(&[]).success().silent_about("licence");
}

// --- [shortcuts] ------------------------------------------------------------

/// The rule that makes shortcuts safe: what is recorded is the expanded URL.
///
/// If the shortcut were recorded instead, a project created by someone with a
/// `mine:` shortcut would be unusable by everyone else, and `refs/tpl/<id>`
/// would differ per contributor for the same template — template refs are
/// append-only, so two contributors deriving two ids is not a cosmetic problem.
#[test]
fn an_expanded_shortcut_is_what_gets_recorded_in_the_project() {
    let world = World::new();
    let source = world.template.source();
    // A local path, so the test needs no network; the substitution is the same
    // one a `https://github.com/` prefix gets. Forward slashes throughout —
    // Git accepts them on every platform, and a Windows path written into TOML
    // raw would be a string full of invalid escape sequences.
    let path = std::path::Path::new(&source);
    let parent = path
        .parent()
        .expect("a path with a parent")
        .to_string_lossy()
        .replace('\\', "/");
    let name = path
        .file_name()
        .expect("a path with a final component")
        .to_string_lossy()
        .into_owned();
    world
        .project
        .user_config(&format!("[shortcuts]\nt = \"{parent}/\"\n"));

    tpl(
        &world.project,
        &["init", &format!("t:{name}"), "--defaults"],
    )
    .success();

    let recorded = world.project.read(".config/git.tpl.toml");
    assert!(
        recorded.contains(&format!("source = \"{parent}/{name}\"")),
        "the expanded URL must be recorded, not the shortcut:\n{recorded}"
    );
    assert!(
        !recorded.contains("t:"),
        "the shortcut must never leave this machine:\n{recorded}"
    );
    // Derived from the expanded URL, so every contributor gets the same ref.
    world.project.git(&["rev-parse", &world.ref_name()]);
}

/// It may be a real scheme, and there is no list of every one of them.
#[test]
fn an_unknown_prefix_is_left_alone() {
    let world = World::new();
    world
        .project
        .user_config("[shortcuts]\ngh = \"https://github.com/\"\n");

    // The failure must be about resolving `nope://x`, not about an expansion.
    tpl(&world.project, &["init", "nope://x", "--defaults"])
        .failure()
        .silent_about("https://github.com/");
}
