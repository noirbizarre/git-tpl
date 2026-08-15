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
