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

use tpl::git::{GitError, LibGit2};

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

/// The regression. No wire is involved: the source is a local repository and
/// the *destination* cannot be created, because a regular file sits where the
/// clone wants a directory. Before the class-based split this was reported as
/// "could not reach", which sent the user to check a network that was fine.
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
