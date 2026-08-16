//! The transports libgit2 was built with, and what a clone failure is blamed on.
//!
//! git-tpl once shipped with `default-features = false` on `git2` and nothing
//! putting `https` and `ssh` back, so the vendored libgit2 had neither a TLS
//! backend nor libssh2 and *no* remote could be cloned. The failure surfaced as
//! a plain `tpl::git::network`, indistinguishable from an unplugged cable.
//!
//! The rest of the file guards the converse mistake: `clone` writes a
//! repository as well as reading a remote, and every failure from it used to be
//! reported as unreachability. A full `$TMPDIR` therefore told the user to
//! check their proxy, and looked intermittent because it tracked free disk
//! space rather than connectivity.
//!
//! These tests never leave the machine: port 1 on the loopback refuses
//! immediately, which is enough to prove the protocol was understood in the
//! first place.

mod common;

use common::file_url;
use tpl::git::{GitBackend, GitError, LibGit2};

/// Reach for a remote that will refuse the connection, and report why.
fn failure(url: &str) -> String {
    let dir = tempfile::tempdir().expect("temporary directory");
    let into = dir.path().join("clone.git");

    match LibGit2::clone_bare(url, &into) {
        // A refused connection is the expected outcome. Anything succeeding
        // here would mean something is listening on port 1, not that the test
        // passed.
        Err(GitError::Network { reason, .. }) => reason,
        Err(other) => panic!("expected a network failure, got: {other}"),
        Ok(_) => panic!("nothing should answer on port 1"),
    }
}

/// Without git2's `https` feature libgit2 has no TLS backend, and every HTTPS
/// template fails with "there is no TLS stream available".
#[test]
fn an_https_remote_has_a_tls_backend() {
    let reason = failure("https://127.0.0.1:1/template.git");
    assert!(
        !reason.to_lowercase().contains("tls stream"),
        "libgit2 was built without a TLS backend: {reason}"
    );
}

/// Without git2's `ssh` feature libgit2 has no libssh2, and rejects the scheme
/// outright with "unsupported URL protocol".
#[test]
fn an_ssh_remote_has_a_transport() {
    let reason = failure("ssh://git@127.0.0.1:1/template.git");
    assert!(
        !reason.to_lowercase().contains("unsupported url protocol"),
        "libgit2 was built without an SSH transport: {reason}"
    );
}

/// The wire really did fail, so `tpl::git::network` is the right answer and the
/// remedy it offers — check the URL, the network, the proxy — is the right one.
#[test]
fn a_refused_connection_is_a_network_failure() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let into = dir.path().join("clone.git");

    match LibGit2::clone_bare("https://127.0.0.1:1/template.git", &into) {
        Err(GitError::Network { .. }) => {}
        Err(other) => panic!("a refused connection must be a network failure, got: {other}"),
        Ok(_) => panic!("nothing should answer on port 1"),
    }
}

/// The stage recording must not get in the way of the thing working. A clone
/// that succeeds exercises the callbacks the attribution hangs off, which
/// nothing else here reaches — every other case fails before objects arrive.
///
/// The `file://` form is deliberate. Given a plain path libgit2 takes a
/// shortcut and hardlinks the object files, so no pack is negotiated and
/// `transfer_progress` never fires; `file://` forces the real download path,
/// which is what a template source actually takes.
#[test]
fn a_clone_that_succeeds_still_succeeds() {
    let dir = tempfile::tempdir().expect("temporary directory");

    let source = dir.path().join("source");
    let origin = LibGit2::init(&source).expect("a source to clone from");
    origin
        .set_config_str("user.name", "Test")
        .expect("an identity");
    origin
        .set_config_str("user.email", "test@example.invalid")
        .expect("an identity");
    let tree = origin.build_tree(&[]).expect("an empty tree");
    let commit = origin
        .create_commit(tree, &[], "seed")
        .expect("something to clone");
    origin
        .set_ref("refs/heads/main", commit, "seed")
        .expect("a branch to clone");

    let into = dir.path().join("clone.git");
    let clone = LibGit2::clone_bare(&file_url(&source), &into)
        .expect("a local repository clones without a network");

    assert_eq!(
        clone.resolve_ref("refs/heads/main").expect("the branch"),
        Some(commit),
        "the clone did not bring the branch across"
    );
}

/// The regression. No wire is involved: the source is a local repository and
/// the *destination* cannot be created, because a regular file sits where the
/// clone wants a directory. Before the stage-based attribution this was
/// reported as "could not reach", which sent the user to check a network that
/// was fine.
#[test]
fn an_unwritable_destination_is_not_a_network_failure() {
    let dir = tempfile::tempdir().expect("temporary directory");

    // A real repository, so that nothing about the *source* can be at fault.
    let source = dir.path().join("source");
    LibGit2::init(&source).expect("a source to clone from");

    // A regular file, so every path underneath it is ENOTDIR.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"").expect("write the blocker");
    let into = blocker.join("clone.git");

    match LibGit2::clone_bare(&source.to_string_lossy(), &into) {
        Err(GitError::Clone { .. }) => {}
        Err(GitError::Network { reason, .. }) => {
            panic!("a local write failure was blamed on the network: {reason}")
        }
        Err(other) => panic!("expected a clone failure, got: {other}"),
        Ok(_) => panic!("a clone under a regular file cannot succeed"),
    }
}
