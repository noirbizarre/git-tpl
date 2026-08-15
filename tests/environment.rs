//! The harness's own isolation from the environment it is run in.
//!
//! Not a test of `git-tpl`: a test of `tests/common`. The suite is routinely
//! run from inside a Git process — `git rebase --exec 'cargo test'`,
//! `git bisect run cargo test`, a `pre-push` hook — and Git exports `GIT_DIR`,
//! `GIT_WORK_TREE` and `GIT_INDEX_FILE` to whatever it invokes. Those override
//! `current_dir`, so a harness that inherits them builds its "temporary"
//! repository on top of the git-tpl repository, writes `user.name` into the
//! developer's config and clears `core.bare` from a worktree's. That is #14,
//! and it survives the run that caused it.

mod common;

use std::process::Command;

use common::{World, tpl};

/// Re-run the canary below with the environment Git would have given us, and
/// require it to pass. This is the reproduction from #14 reduced to one
/// process: before `scrub_git_env` existed it failed, and it fails again the
/// day a new call site builds a `Command` without the helper.
#[test]
fn the_harness_ignores_an_ambient_git_environment() {
    let dir = tempfile::tempdir().expect("temp dir");
    // A path that does not exist, so any leak is a hard error rather than a
    // quiet write into a real repository — including this one.
    let absent = dir.path().join("absent.git");

    let status = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", "a_repository_is_built_and_committed_to"])
        .arg("--nocapture")
        .env("GIT_DIR", &absent)
        .env("GIT_WORK_TREE", &absent)
        .env("GIT_INDEX_FILE", absent.join("index"))
        .env("GIT_AUTHOR_NAME", "Ambient")
        .env("GIT_AUTHOR_EMAIL", "ambient@example.invalid")
        .env("GIT_COMMITTER_NAME", "Ambient")
        .env("GIT_COMMITTER_EMAIL", "ambient@example.invalid")
        .status()
        .expect("re-run this test binary");

    assert!(
        status.success(),
        "the harness followed the ambient Git environment instead of its own \
         temporary repository"
    );
}

/// The canary the test above re-runs, exercising every part of the harness
/// that touches Git: `init`, `configure`, a commit, and one binary invocation.
///
/// It also runs normally, in a clean environment, as the control.
#[test]
fn a_repository_is_built_and_committed_to() {
    let world = World::new();
    world.init(&[]).success();

    world.project.write("note.txt", "content");
    world.project.commit_all("a commit");

    // The identity is the repository's, not the one an ambient `GIT_AUTHOR_*`
    // would impose.
    assert_eq!(world.project.git(&["log", "-1", "--format=%an"]), "Test");
    assert_eq!(
        world.project.git(&["log", "-1", "--format=%ae"]),
        "test@example.invalid"
    );
    // A worktree, at the path the harness chose.
    assert_eq!(
        world.project.git(&["rev-parse", "--is-bare-repository"]),
        "false"
    );

    tpl(&world.project, &["status"]).success();
}
