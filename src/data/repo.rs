//! Locating a data file inside another Git repository.
//!
//! The parsing lives here rather than in `mod.rs` so it can be exercised
//! without a repository, and it deliberately declares no diagnostics of its
//! own: a code takes its declaring module's name, and `tpl::git::*` already
//! belongs to [`crate::git::GitError`]. Failures come back as a `String`
//! reason, and `mod.rs` wraps them into `tpl::data::invalid_git_source`.

use std::fmt;

/// The transports a shorthand may name.
///
/// An allow-list rather than "anything before `://`": an unrecognised scheme is
/// far more likely a typo than a transport libgit2 has, and reading it as a
/// repository would turn the typo into a clone attempt.
const SCHEMES: &[&str] = &["https", "http", "ssh", "git", "file"];

/// A data file in another Git repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLocation {
    /// The repository to clone.
    pub repo: String,
    /// The revision to read at — branch, tag or SHA, as written.
    pub reference: String,
    /// The path inside that repository's tree.
    pub path: String,
}

impl fmt::Display for GitLocation {
    /// The canonical `repo@ref:path`.
    ///
    /// The single producer of the string used for the value cache key, the
    /// provenance trailer, the confirmation prompt and every error's
    /// `location:` — the same reason `describe_revision` exists.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}:{}", self.repo, self.reference, self.path)
    }
}

/// Parse `<scheme>://<repo>@<ref>:<path>`.
///
/// `None` means "this is not a shorthand" and the caller should carry on
/// inferring. `Some(Err)` means it was plainly meant to be one and is broken,
/// which earns a diagnostic naming the defect rather than a 404 later.
///
/// An explicit scheme is required, and that requirement is the whole reason
/// this can be parsed at all: `git@github.com:acme/data` is scp-style, has no
/// `://`, and is refused on the first line rather than being misread as a
/// repository called `git` at revision `github.com`.
pub fn parse_shorthand(source: &str) -> Option<Result<GitLocation, String>> {
    let (scheme, rest) = source.split_once("://")?;
    if !SCHEMES.contains(&scheme) {
        return None;
    }

    // The *last* colon: an `ssh://host:22/…` port and a `file:///C:/…` drive
    // letter both sit to its left. Safe because Git forbids `:` in a ref name,
    // so the separator is always the last one — provided the path contains
    // none, which is the documented limit of the shorthand.
    let (left, path) = rest.rsplit_once(':')?;

    // The revision is mandatory. Falling back to the remote's default branch
    // would be a moving ref recorded nowhere, which is the failure this whole
    // feature exists to avoid.
    let (repo, reference) = left.rsplit_once('@')?;

    // A userinfo `@` is not a revision separator. `ssh://git@host/o/r:x.toml`
    // has exactly one `@`, and reading it as one would yield a repository
    // called `ssh://git`. Requiring the `@` to sit past the authority's first
    // `/` distinguishes the two without guessing.
    let authority = scheme.len() + "://".len();
    let host_end = source[authority..].find('/').map(|i| authority + i);
    match host_end {
        Some(end) if authority + repo.len() > end => {}
        // No path component at all, so every `@` is userinfo.
        _ => return None,
    }

    if repo.is_empty() || reference.is_empty() || path.is_empty() {
        return Some(Err(
            "a shorthand is `<repo>@<ref>:<path>`, and no part may be empty".into(),
        ));
    }

    if let Err(reason) = check_reference(reference) {
        return Some(Err(reason));
    }
    if let Err(reason) = check_path(path) {
        return Some(Err(reason));
    }

    Some(Ok(GitLocation {
        repo: format!("{scheme}://{repo}"),
        reference: reference.to_string(),
        path: path.to_string(),
    }))
}

/// The cheap half of `git check-ref-format`.
///
/// The string is handed to `resolve_revision`, and a hostile one should be
/// refused by us with a sentence about what is wrong, not by libgit2 with a
/// sentence about its own internals.
pub fn check_reference(reference: &str) -> Result<(), String> {
    const FORBIDDEN: &[char] = &[' ', '~', '^', '?', '*', '[', '\\', ':'];
    if let Some(bad) = reference
        .chars()
        .find(|c| FORBIDDEN.contains(c) || c.is_control())
    {
        return Err(format!(
            "`{reference}` is not a valid ref name: it contains {bad:?}"
        ));
    }
    Ok(())
}

/// A path must stay inside the repository it came from.
///
/// Rejected rather than resolved. A data repository is untrusted input, and
/// `..` here is a request to read a file the declaration does not name — the
/// same rule a project-local source gets, for the same reason.
pub fn check_path(path: &str) -> Result<(), String> {
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(format!(
            "`{path}` is absolute: a path is relative to the repository root"
        ));
    }
    // A Windows drive prefix is absolute too, and `starts_with('/')` misses it.
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return Err(format!(
            "`{path}` is absolute: a path is relative to the repository root"
        ));
    }
    if path.split(['/', '\\']).any(|part| part == "..") {
        return Err(format!("`{path}` leaves the repository"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(source: &str) -> GitLocation {
        parse_shorthand(source)
            .unwrap_or_else(|| panic!("`{source}` was not recognised as a shorthand"))
            .unwrap_or_else(|reason| panic!("`{source}` was rejected: {reason}"))
    }

    #[test]
    fn an_scp_style_source_is_never_read_as_a_shorthand() {
        // The whole reason a scheme is required. Both of these contain an `@`
        // and a `:`, and a heuristic split would produce a nonsense repository.
        assert!(parse_shorthand("git@github.com:acme/data.git").is_none());
        assert!(parse_shorthand("git@github.com:acme/data@v1:licenses.toml").is_none());
    }

    #[test]
    fn a_shorthand_needs_an_explicit_scheme() {
        assert!(parse_shorthand("github.com/acme/data@v1:licenses.toml").is_none());
        assert!(parse_shorthand("weird://github.com/acme/data@v1:licenses.toml").is_none());
    }

    #[test]
    fn a_shorthand_splits_on_the_last_colon_so_a_port_stays_in_the_repository() {
        let location = ok("ssh://git@host:22/acme/data@main:licenses.toml");
        assert_eq!(location.repo, "ssh://git@host:22/acme/data");
        assert_eq!(location.reference, "main");
        assert_eq!(location.path, "licenses.toml");
    }

    #[test]
    fn a_userinfo_at_sign_is_not_a_revision() {
        // Its only `@` is before the authority's first `/`, so there is no
        // revision here and the string is not a shorthand at all.
        assert!(parse_shorthand("ssh://git@host/acme/data:licenses.toml").is_none());
    }

    #[test]
    fn a_branch_name_containing_a_slash_survives_the_shorthand() {
        let location = ok("https://host/acme/data@release/2.x:licenses.toml");
        assert_eq!(location.reference, "release/2.x");
        assert_eq!(location.path, "licenses.toml");
    }

    #[test]
    fn a_repository_suffix_is_left_alone() {
        // `.git` is part of the URL the user must be able to clone, and
        // stripping it would break a host that requires it.
        let location = ok("https://host/acme/data.git@v2.1.0:data/licenses.toml");
        assert_eq!(location.repo, "https://host/acme/data.git");
        assert_eq!(location.path, "data/licenses.toml");
    }

    #[test]
    fn a_windows_drive_letter_survives_a_file_url() {
        let location = ok("file:///C:/repos/data@main:licenses.toml");
        assert_eq!(location.repo, "file:///C:/repos/data");
        assert_eq!(location.path, "licenses.toml");
    }

    #[test]
    fn a_shorthand_path_that_leaves_the_repository_is_refused() {
        // Not `None`: this is plainly meant to be a shorthand, and reporting it
        // as an unsupported remote scheme would send the author to the wrong page.
        let reason = parse_shorthand("https://host/acme/data@v1:../../etc/passwd")
            .expect("recognised as a shorthand")
            .expect_err("traversal is refused");
        assert!(reason.contains("leaves the repository"), "{reason}");
    }

    #[test]
    fn a_shorthand_with_an_empty_part_names_the_shape_it_wanted() {
        let reason = parse_shorthand("https://host/acme/data@v1:")
            .expect("recognised as a shorthand")
            .expect_err("an empty path is refused");
        assert!(reason.contains("<repo>@<ref>:<path>"), "{reason}");
    }

    #[test]
    fn a_plain_https_url_is_not_a_shorthand() {
        // It must keep inferring as a remote source, or every existing template
        // with remote data breaks.
        assert!(parse_shorthand("https://example.com/licenses.toml").is_none());
    }
}
