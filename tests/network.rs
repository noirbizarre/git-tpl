//! The transports libgit2 was built with.
//!
//! git-tpl once shipped with `default-features = false` on `git2` and nothing
//! putting `https` and `ssh` back, so the vendored libgit2 had neither a TLS
//! backend nor libssh2 and *no* remote could be cloned. The failure surfaced as
//! a plain `tpl::git::network`, indistinguishable from an unplugged cable.
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
