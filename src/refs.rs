//! Template identity, and the ref name derived from it.
//!
//! Every project/template relationship gets a dedicated ref,
//! `refs/tpl/<template-id>`. This module owns turning a template source into
//! that id. See `docs/adr/001-rendered-ref-model.md`.

use std::fmt;

use miette::Diagnostic;
use thiserror::Error;

/// The ref namespace holding rendered template state.
///
/// Deliberately not `refs/heads/`: this is not a branch you check out, it must
/// not appear in `git branch`, and a bare `git push` must not send it.
pub const REF_PREFIX: &str = "refs/tpl/";

/// Errors from constructing a [`TemplateId`].
#[derive(Debug, Error, Diagnostic)]
pub enum TemplateIdError {
    /// The source normalised to nothing usable.
    #[error("cannot derive a template id from source `{origin}`")]
    #[diagnostic(
        code(tpl::refs::underivable),
        help("set `id` under `[template]` in .config/git.tpl.toml to name it explicitly")
    )]
    Underivable {
        /// The source that could not be normalised.
        // Not named `source`: thiserror treats that name as `#[source]` and
        // requires it to implement `Error`.
        origin: String,
    },

    /// An explicitly configured id is not usable as a ref component.
    #[error("`{id}` is not a valid template id")]
    #[diagnostic(
        code(tpl::refs::invalid),
        help(
            "a template id may contain only letters, digits, `-`, `_` and `.`, and must not start or end with `-`"
        )
    )]
    Invalid {
        /// The rejected id.
        id: String,
    },
}

/// A normalised template identity.
///
/// Constructed from an explicit `[template] id` when there is one, and derived
/// from the source URL otherwise. The derivation collapses the SSH and HTTPS
/// forms of the same repository to the same id, so switching between them does
/// not orphan the ref.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TemplateId(String);

impl TemplateId {
    /// Derive an id from a template source.
    ///
    /// | source | id |
    /// |---|---|
    /// | `https://github.com/noirbizarre/rust-library` | `github-com-noirbizarre-rust-library` |
    /// | `git@github.com:noirbizarre/rust-library.git` | `github-com-noirbizarre-rust-library` |
    /// | `../rust-library-template` | `rust-library-template` |
    pub fn derive(source: &str) -> Result<Self, TemplateIdError> {
        let normalised = normalise(source);
        if normalised.is_empty() {
            return Err(TemplateIdError::Underivable {
                origin: source.to_string(),
            });
        }
        Ok(Self(normalised))
    }

    /// Use an explicitly configured id, validating it.
    pub fn explicit(id: &str) -> Result<Self, TemplateIdError> {
        if !is_valid(id) {
            return Err(TemplateIdError::Invalid { id: id.to_string() });
        }
        Ok(Self(id.to_string()))
    }

    /// Derive from a source unless an explicit id is configured.
    pub fn resolve(source: &str, explicit: Option<&str>) -> Result<Self, TemplateIdError> {
        match explicit {
            Some(id) => Self::explicit(id),
            None => Self::derive(source),
        }
    }

    /// The full ref name, `refs/tpl/<id>`.
    pub fn ref_name(&self) -> String {
        format!("{REF_PREFIX}{}", self.0)
    }

    /// The remote-tracking ref name, `refs/remotes/<remote>/tpl/<id>`.
    ///
    /// Template refs are fetched into the remote's namespace like any other
    /// remote ref, so that comparing local and remote copies is an ordinary
    /// ahead/behind calculation.
    pub fn remote_ref_name(&self, remote: &str) -> String {
        format!("refs/remotes/{remote}/tpl/{}", self.0)
    }

    /// The id as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TemplateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a string is usable as the `<id>` component of a ref name.
///
/// Stricter than Git's own ref rules on purpose. Git would accept far more, but
/// an id also appears in output, in error messages and in shell commands we
/// suggest, and an id needing quoting there is an id we do not want.
fn is_valid(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.starts_with('.')
        && !id.ends_with('.')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Turn a template source into a ref-safe slug.
fn normalise(source: &str) -> String {
    let mut s = source.trim();

    // Strip the transport. The scheme says how to reach the repository, not
    // which repository it is, so `https://` and `ssh://` forms of one address
    // must not produce different ids.
    for scheme in [
        "https://",
        "http://",
        "ssh://",
        "git://",
        "git+ssh://",
        "file://",
    ] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest;
            break;
        }
    }

    // Strip `user@` from scp-style and ssh URLs, for the same reason: the
    // account used to authenticate is not part of the repository's identity.
    if let Some((prefix, rest)) = s.split_once('@') {
        // Only when it really is a userinfo prefix. A path may legitimately
        // contain `@` (`../templates/rust@v2`), and stripping there would
        // silently merge two distinct local templates onto one id.
        if !prefix.contains('/') && !prefix.contains('\\') {
            s = rest;
        }
    }

    // `git@github.com:noirbizarre/x` — the scp-style separator is a path
    // separator, not a port. Ports are stripped below in any case, so
    // normalising it to `/` here is enough to make the scp and URL forms agree.
    let s = s.replace(':', "/");

    // Drop a trailing `.git`, which is a convention of the URL rather than part
    // of the repository name, and is present in some forms and not others.
    let s = s.strip_suffix(".git").unwrap_or(&s).to_string();

    // A local path's identity is its final component. Keeping the whole path
    // would put every developer's home directory in the ref name, so the same
    // template checked out to two places would produce two refs.
    let s = if is_local_path(source) {
        s.trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .find(|c| !c.is_empty() && *c != "." && *c != "..")
            .unwrap_or("")
            .to_string()
    } else {
        s
    };

    slugify(&s)
}

/// Whether a source refers to a path on this machine rather than a remote.
fn is_local_path(source: &str) -> bool {
    !source.contains("://")
        && !source.contains('@')
        && (source.starts_with('.')
            || source.starts_with('/')
            || source.starts_with('~')
            || source.contains('\\')
            // A bare relative path such as `my-template`. Anything containing a
            // dot before a slash is more likely a hostname (`github.com/x`).
            || !source.split('/').next().unwrap_or("").contains('.'))
}

/// Lowercase, collapse every run of non-alphanumerics to a single `-`, trim.
///
/// ASCII-only and lossy on purpose. This derives `refs/tpl/<id>` from a URL,
/// where the input is a host and a path and the output must never change:
/// a different slug is a different ref, and invariant 3 says refs are
/// append-only. `eval::slugify` — the template filter — transliterates instead,
/// and the two are kept separate so that improving the filter cannot rename
/// anybody's template ref.
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_sep = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('-');
            }
            pending_sep = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending_sep = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(
        "https://github.com/noirbizarre/rust-library",
        "github-com-noirbizarre-rust-library"
    )]
    #[case(
        "http://github.com/noirbizarre/rust-library",
        "github-com-noirbizarre-rust-library"
    )]
    #[case(
        "https://github.com/noirbizarre/rust-library.git",
        "github-com-noirbizarre-rust-library"
    )]
    #[case(
        "git@github.com:noirbizarre/rust-library.git",
        "github-com-noirbizarre-rust-library"
    )]
    #[case(
        "ssh://git@github.com/noirbizarre/rust-library",
        "github-com-noirbizarre-rust-library"
    )]
    #[case(
        "https://gitlab.example.com/team/sub/tpl",
        "gitlab-example-com-team-sub-tpl"
    )]
    fn a_remote_source_becomes_a_host_and_path_slug(#[case] source: &str, #[case] expected: &str) {
        assert_eq!(TemplateId::derive(source).unwrap().as_str(), expected);
    }

    /// The SSH and HTTPS forms of one repository must agree, or a user who
    /// switches between them silently orphans the rendered ref and the next
    /// merge, having no common ancestor, conflicts on everything that differs.
    #[test]
    fn the_ssh_and_https_forms_of_a_repository_derive_the_same_id() {
        let https = TemplateId::derive("https://github.com/noirbizarre/rust-library").unwrap();
        let ssh = TemplateId::derive("git@github.com:noirbizarre/rust-library.git").unwrap();
        let ssh_url = TemplateId::derive("ssh://git@github.com/noirbizarre/rust-library").unwrap();
        assert_eq!(https, ssh);
        assert_eq!(https, ssh_url);
    }

    #[rstest]
    #[case("../rust-library-template", "rust-library-template")]
    #[case("./my-template", "my-template")]
    #[case("/home/someone/src/my-template", "my-template")]
    #[case("../my-template/", "my-template")]
    #[case("my-template", "my-template")]
    fn a_local_path_becomes_its_final_component(#[case] source: &str, #[case] expected: &str) {
        assert_eq!(TemplateId::derive(source).unwrap().as_str(), expected);
    }

    /// Keeping the whole path would embed the developer's home directory in the
    /// ref name, so one template cloned to two places would render to two refs.
    #[test]
    fn the_same_local_template_at_two_paths_derives_the_same_id() {
        let a = TemplateId::derive("/home/alice/src/my-template").unwrap();
        let b = TemplateId::derive("../../work/my-template").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn an_explicit_id_overrides_the_derivation() {
        let id = TemplateId::resolve(
            "https://github.com/noirbizarre/rust-library",
            Some("legacy-name"),
        )
        .unwrap();
        assert_eq!(id.as_str(), "legacy-name");
    }

    #[rstest]
    #[case("has spaces")]
    #[case("-leading")]
    #[case("trailing-")]
    #[case("has/slash")]
    #[case("..")]
    #[case("a..b")]
    #[case("")]
    fn an_unusable_explicit_id_is_rejected(#[case] id: &str) {
        std::assert_matches!(
            TemplateId::explicit(id),
            Err(TemplateIdError::Invalid { .. })
        );
    }

    #[test]
    fn a_source_with_nothing_sluggable_is_rejected() {
        std::assert_matches!(
            TemplateId::derive("///"),
            Err(TemplateIdError::Underivable { .. })
        );
    }

    #[test]
    fn the_ref_name_is_under_refs_tpl() {
        let id = TemplateId::explicit("rust-library").unwrap();
        assert_eq!(id.ref_name(), "refs/tpl/rust-library");
        assert_eq!(
            id.remote_ref_name("origin"),
            "refs/remotes/origin/tpl/rust-library"
        );
    }

    /// Every derived id must survive `TemplateId::explicit`, or a source could
    /// produce an id we would refuse to accept if it were written down.
    #[rstest]
    #[case("https://github.com/noirbizarre/rust-library")]
    #[case("git@github.com:a/b.git")]
    #[case("../my-template")]
    #[case("https://example.com/~weird/name!!")]
    fn every_derived_id_is_itself_valid(#[case] source: &str) {
        let derived = TemplateId::derive(source).unwrap();
        assert_eq!(
            TemplateId::explicit(derived.as_str()).unwrap(),
            derived,
            "derived `{derived}` would be rejected if written explicitly"
        );
    }
}
