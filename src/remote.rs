//! A remote URL, taken apart so a prompt can be pre-filled from it.
//!
//! A template asking for a project slug, an owner or a repository name can
//! usually guess the answer from where the project is pushed. This module owns
//! that guess, and nothing else: the result reaches [`crate::seed`], which
//! reaches an interactive prompt, and never the render context.
//!
//! Deliberately *not* [`crate::refs::normalise`]. That one derives
//! `refs/tpl/<id>` and its output is frozen by invariant 3 — a change there
//! renames the template ref of every existing project. This one may be improved
//! freely, because its output only ever pre-fills a prompt a human then
//! confirms. Keeping them apart is what makes that freedom safe.

/// A remote URL split into the parts a template is likely to ask for.
///
/// | url | host | owner | name | slug |
/// |---|---|---|---|---|
/// | `https://github.com/me/git-tpl.git` | `github.com` | `me` | `git-tpl` | `me/git-tpl` |
/// | `git@github.com:me/git-tpl.git` | `github.com` | `me` | `git-tpl` | `me/git-tpl` |
/// | `https://gitlab.com/a/b/c.git` | `gitlab.com` | `a/b` | `c` | `a/b/c` |
/// | `/srv/git/thing.git` | — | — | `thing` | `thing` |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// The URL as configured, with any credentials removed.
    pub url: String,
    /// The host, absent for a filesystem path.
    pub host: Option<String>,
    /// Everything in the path before the final component, joined with `/`.
    ///
    /// A single segment for the usual `owner/repo`, several for a nested
    /// GitLab subgroup. Absent when the path has only one component.
    pub owner: Option<String>,
    /// The final path component, without a trailing `.git`.
    pub name: String,
    /// `owner/name`, or just `name` when there is no owner.
    pub slug: String,
}

/// Take a remote URL apart, or return `None` if there is nothing usable in it.
///
/// Never fails: an unparsable remote means an absent seed, and a question falls
/// back to its declared `default`. A remote we cannot read is not a reason to
/// refuse to render.
pub fn parse(url: &str) -> Option<Remote> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Any scheme, not a fixed list. `refs::normalise` enumerates because its
    // output must never change; here an unknown transport should degrade to a
    // sensible guess rather than to nothing.
    let (had_scheme, after_scheme) = match trimmed.split_once("://") {
        Some((_, rest)) => (true, rest),
        None => (false, trimmed),
    };

    // Strip `user@` or `user:token@`. A token must never reach a prompt, where
    // it would be echoed to the terminal and then recorded as an answer.
    let (had_userinfo, authority) = match after_scheme.split_once('@') {
        // Only when it really is userinfo: a path may legitimately contain `@`
        // (`/srv/git/rust@v2`), and splitting there would lose the directory.
        Some((prefix, rest)) if !prefix.contains('/') && !prefix.contains('\\') => (true, rest),
        _ => (false, after_scheme),
    };

    // scp form — `git@github.com:me/repo`. The colon separates host from path
    // only when no `/` precedes it; `ssh://host:22/path` is a port and is
    // handled as an ordinary URL below.
    let scp = !had_scheme
        && match (authority.find(':'), authority.find('/')) {
            (Some(colon), Some(slash)) => colon < slash,
            (Some(_), None) => true,
            _ => false,
        };

    let (host, path) = if scp {
        let (host, path) = authority.split_once(':').expect("scp form has a colon");
        (Some(host), path)
    } else {
        match authority.split_once('/') {
            Some((host, path)) => (Some(host), path),
            // No slash at all: a bare name, either a host or a relative path.
            None => (None, authority),
        }
    };

    // A bare filesystem path has no host. `file:///srv/x` leaves an empty
    // authority, and `../peer` or `/srv/x` never had one — in both cases the
    // first segment is part of the path, not a machine.
    let looks_local = !had_scheme && !had_userinfo && !scp;
    let (host, path) = match host {
        Some(h) if looks_local => (None, join_path(h, path)),
        // `file:///srv/x` leaves an empty authority: the path was absolute.
        Some("") => (None, path.to_string()),
        Some(h) => (Some(strip_port(h).to_string()), path.to_string()),
        None => (None, path.to_string()),
    };

    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);

    // `.`, `..` and empty segments describe how to walk to the repository, not
    // what it is called. `../../work/thing` is `thing`.
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .collect();
    let (name, owner_parts) = segments.split_last()?;
    let name = (*name).to_string();
    if name.is_empty() {
        return None;
    }

    // With no host there is no owner either. The leading components of
    // `/home/ada/src/thing` are one developer's filesystem, not an
    // organisation, and seeding a prompt with `home/ada/src` would be worse
    // than seeding nothing. The same reasoning as `refs::normalise`, which
    // keeps only the final component of a local path.
    let owner = if host.is_none() || owner_parts.is_empty() {
        None
    } else {
        Some(owner_parts.join("/"))
    };
    let slug = match &owner {
        Some(owner) => format!("{owner}/{name}"),
        None => name.clone(),
    };

    // The URL is reported back without credentials, for the same reason they
    // are stripped above. Everything else is preserved verbatim: an author who
    // wants the exact remote asked for the exact remote.
    let url = if had_userinfo {
        match trimmed.split_once("://") {
            Some((scheme, _)) => format!("{scheme}://{authority}"),
            None => authority.to_string(),
        }
    } else {
        trimmed.to_string()
    };

    Some(Remote {
        url,
        host,
        owner,
        name,
        slug,
    })
}

/// Re-join a first segment that turned out to be part of the path, not a host.
fn join_path(first: &str, rest: &str) -> String {
    if rest.is_empty() {
        first.to_string()
    } else {
        format!("{first}/{rest}")
    }
}

/// Drop `:<port>` from an authority. The port says how to reach the host, not
/// which host it is, so `ssh.github.com:443` and `ssh.github.com` are one host.
fn strip_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(
        "https://github.com/me/git-tpl.git",
        Some("github.com"),
        Some("me"),
        "git-tpl"
    )]
    #[case(
        "https://github.com/me/git-tpl",
        Some("github.com"),
        Some("me"),
        "git-tpl"
    )]
    #[case(
        "git@github.com:me/git-tpl.git",
        Some("github.com"),
        Some("me"),
        "git-tpl"
    )]
    #[case(
        "ssh://git@ssh.github.com:443/me/git-tpl.git",
        Some("ssh.github.com"),
        Some("me"),
        "git-tpl"
    )]
    #[case(
        "git://github.com/me/git-tpl.git",
        Some("github.com"),
        Some("me"),
        "git-tpl"
    )]
    #[case(
        "https://gitlab.com/group/sub/team/thing.git",
        Some("gitlab.com"),
        Some("group/sub/team"),
        "thing"
    )]
    #[case("/srv/git/thing.git", None, None, "thing")]
    #[case("../peer-repo", None, None, "peer-repo")]
    #[case("file:///srv/git/thing.git", None, None, "thing")]
    #[case("thing", None, None, "thing")]
    fn a_remote_url_is_split_into_host_owner_and_name(
        #[case] url: &str,
        #[case] host: Option<&str>,
        #[case] owner: Option<&str>,
        #[case] name: &str,
    ) {
        let remote = parse(url).expect("parsable");
        assert_eq!(remote.host.as_deref(), host, "host of `{url}`");
        assert_eq!(remote.owner.as_deref(), owner, "owner of `{url}`");
        assert_eq!(remote.name, name, "name of `{url}`");
    }

    /// The slug is what a template asking for `owner/repo` wants, and it must
    /// degrade to the bare name rather than to `/name` when there is no owner.
    #[rstest]
    #[case("https://github.com/me/git-tpl.git", "me/git-tpl")]
    #[case("https://gitlab.com/a/b/c.git", "a/b/c")]
    #[case("/srv/git/thing.git", "thing")]
    fn the_slug_joins_the_owner_and_the_name(#[case] url: &str, #[case] slug: &str) {
        assert_eq!(parse(url).unwrap().slug, slug);
    }

    /// A token in a remote URL would be echoed at the prompt and then recorded
    /// as an answer, which is to say committed.
    #[rstest]
    #[case("https://user:ghp_secret@example.com/a/b.git")]
    #[case("https://ghp_secret@example.com/a/b.git")]
    fn a_credential_in_a_remote_url_never_reaches_a_seed(#[case] url: &str) {
        let remote = parse(url).expect("parsable");
        assert_eq!(remote.host.as_deref(), Some("example.com"));
        assert_eq!(remote.slug, "a/b");
        assert!(
            !remote.url.contains("ghp_secret"),
            "credential survived in `{}`",
            remote.url
        );
    }

    /// The scp and HTTPS forms of one repository describe one repository, and a
    /// template seeded from either must offer the same default.
    #[test]
    fn the_scp_and_https_forms_of_a_repository_agree() {
        let scp = parse("git@github.com:me/git-tpl.git").unwrap();
        let https = parse("https://github.com/me/git-tpl.git").unwrap();
        assert_eq!(scp.host, https.host);
        assert_eq!(scp.slug, https.slug);
    }

    /// An `@` inside a path is not userinfo. Splitting there would report the
    /// repository as `v2` and lose the directory it lives in.
    #[test]
    fn an_at_sign_inside_a_path_is_not_a_credential() {
        let remote = parse("/srv/git/rust@v2").unwrap();
        assert_eq!(remote.host, None);
        assert_eq!(remote.name, "rust@v2");
    }

    #[rstest]
    #[case("")]
    #[case("   ")]
    #[case("///")]
    fn a_url_with_nothing_usable_in_it_yields_no_seed(#[case] url: &str) {
        assert_eq!(parse(url), None);
    }
}
