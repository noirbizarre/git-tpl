//! `git tpl man` and `git tpl completion` — the generated packaging artefacts.
//!
//! These exist for one reason: Git intercepts `--help` for a subcommand and
//! execs `man git-tpl`. Without an installed page the project's own flagship
//! invocation fails with exit 16, which is what these tests guard.

mod common;

use common::tpl_outside;

/// Every shell `clap_complete` supports out of the box. Nushell and friends
/// would each be another dependency; these five come free with the crate.
const SHELLS: [&str; 5] = ["bash", "zsh", "fish", "elvish", "powershell"];

fn scratch() -> (tempfile::TempDir, tempfile::TempDir) {
    (
        tempfile::tempdir().expect("tempdir"),
        tempfile::tempdir().expect("config dir"),
    )
}

#[test]
fn every_supported_shell_gets_a_completion_script() {
    let (dir, config) = scratch();

    for shell in SHELLS {
        let out = tpl_outside(dir.path(), config.path(), &["completion", shell]);
        assert_eq!(out.code, 0, "{shell}: {}", out.stderr);
        assert!(
            out.stdout.contains("git-tpl"),
            "{shell} script never names the executable:\n{}",
            out.stdout
        );
    }
}

/// The script is keyed to the file on PATH, not to the `git tpl` spelling the
/// help advertises. A completion registered for a name containing a space is
/// one no shell can ever trigger.
#[test]
fn the_completion_script_is_registered_for_the_executable_name() {
    let (dir, config) = scratch();

    let bash = tpl_outside(dir.path(), config.path(), &["completion", "bash"]);
    assert!(
        bash.stdout.contains("git-tpl") && !bash.stdout.contains("complete -F _git tpl"),
        "{}",
        bash.stdout
    );

    let zsh = tpl_outside(dir.path(), config.path(), &["completion", "zsh"]);
    assert!(zsh.stdout.starts_with("#compdef git-tpl"), "{}", zsh.stdout);
}

#[test]
fn an_unknown_shell_is_refused() {
    let (dir, config) = scratch();
    let out = tpl_outside(dir.path(), config.path(), &["completion", "tcsh"]);
    assert_ne!(out.code, 0);
}

/// `man git-tpl` is the literal command Git runs. If the page is titled
/// anything else — `git tpl`, say, which is what `bin_name` would give — it is
/// unreachable and the user is back to "No manual entry for git-tpl".
#[test]
fn the_man_page_is_titled_after_the_executable() {
    let (dir, config) = scratch();
    let out = tpl_outside(dir.path(), config.path(), &["man"]);

    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.contains(".TH git-tpl 1"),
        "unexpected page header:\n{}",
        out.stdout
    );
}

#[test]
fn each_visible_command_gets_its_own_page() {
    let (dir, config) = scratch();
    let pages = dir.path().join("man1");

    let out = tpl_outside(
        dir.path(),
        config.path(),
        &["man", "--out-dir", pages.to_str().expect("utf-8 path")],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);

    for command in [
        "init",
        "update",
        "render",
        "lint",
        "questions",
        "context",
        "status",
        "diff",
        "show",
        "merge",
        "fetch",
        "push",
        "completion",
    ] {
        assert!(
            pages.join(format!("git-tpl-{command}.1")).is_file(),
            "no page for `{command}`"
        );
    }

    assert!(pages.join("git-tpl.1").is_file(), "no top-level page");
}

/// A command hidden from `--help` is hidden everywhere. Shipping a page for
/// `man`, a packaging tool, would undo the decision to hide it.
#[test]
fn a_hidden_command_gets_no_page() {
    let (dir, config) = scratch();
    let pages = dir.path().join("man1");

    tpl_outside(
        dir.path(),
        config.path(),
        &["man", "--out-dir", pages.to_str().expect("utf-8 path")],
    );

    assert!(!pages.join("git-tpl-man.1").exists());
}

/// The directory is created rather than demanded: a packaging script should not
/// have to `mkdir -p` a path it just named.
#[test]
fn the_output_directory_is_created_if_it_is_missing() {
    let (dir, config) = scratch();
    let pages = dir.path().join("deeply/nested/man1");

    let out = tpl_outside(
        dir.path(),
        config.path(),
        &["man", "--out-dir", pages.to_str().expect("utf-8 path")],
    );

    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(pages.join("git-tpl.1").is_file());
}

#[test]
fn an_unwritable_output_directory_reports_where_and_why() {
    let (dir, config) = scratch();
    // A file, not a directory: portable, unlike chmod 0, and it exercises the
    // same failure path with a message the operating system supplies.
    let blocker = dir.path().join("blocked");
    std::fs::write(&blocker, "not a directory").expect("write");

    let out = tpl_outside(
        dir.path(),
        config.path(),
        &[
            "--json",
            "man",
            "--out-dir",
            blocker.to_str().expect("utf-8 path"),
        ],
    );

    assert_ne!(out.code, 0);
    assert_eq!(out.error_code(), "tpl::ops::write_failed");
}
