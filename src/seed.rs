//! The seed context: machine facts a prompt may be pre-filled from.
//!
//! A template asking for a project slug or a repository owner can usually guess
//! the answer from the project it is being applied to. This module gathers the
//! facts that guess is made of — the Git configuration, the working directory
//! name, the remote URL — and exposes them to a `default_from` expression.
//!
//! # What this is not
//!
//! It is deliberately **not** [`crate::Context`]. That type is the render
//! context, and ADR-006 says a machine-varying value must never reach it: if it
//! did, two developers would render two different trees from the same answers
//! and the template ref would grow a commit every time anyone looked at it.
//!
//! A seed reaches exactly one place — the pre-filled text of an interactive
//! prompt, which a human then confirms and which is then recorded as an
//! ordinary answer. A run that asks nobody anything never builds one of these
//! at all. Being a separate type is what makes that a compile-time fact rather
//! than a convention someone will eventually break.
//!
//! # Namespaces
//!
//! | name | contents |
//! |---|---|
//! | `git` | the Git configuration, dotted keys nested: `git.user.name` |
//! | `dir` | `dir.name`, the working directory's own name |
//! | `remote` | `url`, `host`, `owner`, `name`, `slug` — see [`crate::remote`] |
//!
//! The set is closed. See `docs/adr/018-seed-context.md`.

use std::collections::BTreeMap;

use crate::git::{GitBackend, GitError};
use crate::remote;
use crate::template::Value;

/// The three namespaces a `default_from` expression may read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeedContext {
    roots: BTreeMap<String, Value>,
}

/// The namespaces a `default_from` expression may name.
///
/// Public so that manifest validation can reject an unknown root at load time,
/// where the author sees it, rather than at prompt time on someone else's
/// machine.
pub const NAMESPACES: [&str; 3] = ["dir", "git", "remote"];

/// Gather the seed context from a project repository.
///
/// `remote` is the remote to describe — `tpl.remote`, which defaults to
/// `origin`. Passed in rather than read here so that this module stays a
/// gatherer with no opinion about preferences.
pub fn collect(repo: &dyn GitBackend, remote_name: &str) -> Result<SeedContext, GitError> {
    let mut roots = BTreeMap::new();

    roots.insert("git".to_string(), Value::Table(git_table(repo)?));
    roots.insert("dir".to_string(), Value::Table(dir_table(repo)?));
    roots.insert(
        "remote".to_string(),
        Value::Table(remote_table(repo, remote_name)?),
    );

    Ok(SeedContext { roots })
}

/// The Git configuration as a tree, so `git.user.name` is an ordinary lookup.
fn git_table(repo: &dyn GitBackend) -> Result<BTreeMap<String, Value>, GitError> {
    let mut table = BTreeMap::new();
    for (key, value) in repo.config_entries()? {
        insert_nested(&mut table, &key, value);
    }
    Ok(table)
}

/// Insert `a.b.c = value` as nested tables.
///
/// Two rules worth knowing, both forced by Git rather than chosen:
///
/// - A repeated key wins on its last occurrence. `config_entries` yields
///   entries in Git's precedence order — system, then user, then repository —
///   so last-wins is what `git config --get` would report.
/// - When a key is both a leaf and a branch (`a.b` and `a.b.c`), the branch
///   wins and the scalar is dropped. Dropping the branch would lose more, and
///   this shape only arises from a configuration Git itself treats oddly.
fn insert_nested(table: &mut BTreeMap<String, Value>, key: &str, value: String) {
    let mut segments = key.split('.').filter(|s| !s.is_empty()).peekable();
    let mut current = table;

    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            // A leaf never overwrites a branch.
            if !matches!(current.get(segment), Some(Value::Table(_))) {
                current.insert(segment.to_string(), Value::String(value));
            }
            return;
        }

        let entry = current
            .entry(segment.to_string())
            .or_insert_with(|| Value::Table(BTreeMap::new()));
        // A branch does overwrite a leaf, for the reason above.
        if !matches!(entry, Value::Table(_)) {
            *entry = Value::Table(BTreeMap::new());
        }
        let Value::Table(next) = entry else {
            unreachable!("just ensured a table")
        };
        current = next;
    }
}

/// The working directory's own name — and only that.
///
/// There is no `dir.path`. An absolute path is the value most likely to be
/// pasted into a rendered file, and then the tree differs by machine, which
/// ends invariant 2. It also leaks the user's home directory into a prompt that
/// may be on a screen recording or in a CI log. `dir.name` is already
/// sluggable, which is what anybody actually wants it for.
fn dir_table(repo: &dyn GitBackend) -> Result<BTreeMap<String, Value>, GitError> {
    let mut table = BTreeMap::new();
    let workdir = repo.workdir()?;
    // A non-UTF-8 directory name leaves `dir.name` undefined rather than
    // mangled, so `| default(...)` still has something to do.
    if let Some(name) = workdir.file_name().and_then(|n| n.to_str())
        && !name.is_empty()
    {
        table.insert("name".to_string(), Value::String(name.to_string()));
    }
    Ok(table)
}

/// The remote, taken apart — or an empty table when there is no remote.
///
/// Empty rather than absent: `remote` must always be a table so that
/// `{{ remote.name | default(dir.name) }}` on a repository that has never been
/// pushed is an undefined lookup and not an error.
fn remote_table(
    repo: &dyn GitBackend,
    remote_name: &str,
) -> Result<BTreeMap<String, Value>, GitError> {
    let mut table = BTreeMap::new();
    let Some(url) = repo.remote_url(remote_name)? else {
        return Ok(table);
    };
    let Some(parsed) = remote::parse(&url) else {
        return Ok(table);
    };

    table.insert("url".to_string(), Value::String(parsed.url));
    table.insert("name".to_string(), Value::String(parsed.name));
    table.insert("slug".to_string(), Value::String(parsed.slug));
    if let Some(host) = parsed.host {
        table.insert("host".to_string(), Value::String(host));
    }
    if let Some(owner) = parsed.owner {
        table.insert("owner".to_string(), Value::String(owner));
    }
    Ok(table)
}

impl SeedContext {
    /// Build a context from its roots directly, for tests.
    ///
    /// Not public: outside tests the only way to obtain one is [`collect`],
    /// which is what keeps the namespace set closed in practice and not just in
    /// documentation.
    #[cfg(test)]
    pub(crate) fn from_roots(roots: BTreeMap<String, Value>) -> Self {
        Self { roots }
    }

    /// The seed context as MiniJinja sees it.
    ///
    /// All three roots are always present, possibly empty. One defined level of
    /// nesting is what lets an absent value fall through `| default(...)`
    /// instead of raising.
    pub fn to_minijinja(&self) -> minijinja::Value {
        minijinja::Value::from_iter(
            self.roots
                .iter()
                .map(|(key, value)| (key.clone(), minijinja::Value::from(value.clone()))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(entries: &[(&str, &str)]) -> BTreeMap<String, Value> {
        let mut table = BTreeMap::new();
        for (key, value) in entries {
            insert_nested(&mut table, key, (*value).to_string());
        }
        table
    }

    #[test]
    fn a_dotted_config_key_becomes_a_nested_lookup() {
        let git = table(&[("user.name", "Ada Lovelace")]);
        let Some(Value::Table(user)) = git.get("user") else {
            panic!("expected a `user` table, got {git:?}");
        };
        assert_eq!(
            user.get("name"),
            Some(&Value::String("Ada Lovelace".into()))
        );
    }

    #[test]
    fn a_three_segment_key_nests_twice() {
        let git = table(&[("remote.origin.url", "git@example.com:a/b.git")]);
        let Some(Value::Table(remote)) = git.get("remote") else {
            panic!("expected a `remote` table");
        };
        let Some(Value::Table(origin)) = remote.get("origin") else {
            panic!("expected an `origin` table");
        };
        assert_eq!(
            origin.get("url"),
            Some(&Value::String("git@example.com:a/b.git".into()))
        );
    }

    /// `config_entries` yields Git's precedence order, so the repository value
    /// arrives after the user one and must be the one that survives.
    #[test]
    fn a_repeated_key_keeps_the_last_value() {
        let git = table(&[
            ("user.email", "home@example.com"),
            ("user.email", "work@example.com"),
        ]);
        let Some(Value::Table(user)) = git.get("user") else {
            panic!("expected a `user` table");
        };
        assert_eq!(
            user.get("email"),
            Some(&Value::String("work@example.com".into()))
        );
    }

    #[test]
    fn a_branch_wins_over_a_leaf_of_the_same_name() {
        let both = table(&[("a.b", "leaf"), ("a.b.c", "branch")]);
        let Some(Value::Table(a)) = both.get("a") else {
            panic!("expected an `a` table");
        };
        std::assert_matches!(
            a.get("b"),
            Some(Value::Table(_)),
            "the branch should have won, got {:?}",
            a.get("b")
        );

        // And in the other order, so the result does not depend on which key
        // Git happened to yield first.
        let reversed = table(&[("a.b.c", "branch"), ("a.b", "leaf")]);
        let Some(Value::Table(a)) = reversed.get("a") else {
            panic!("expected an `a` table");
        };
        std::assert_matches!(a.get("b"), Some(Value::Table(_)));
    }

    /// The whole point of the eager tree: a miss must be undefined, or
    /// `| default(...)` never fires and every seed silently becomes empty.
    #[test]
    fn a_missing_key_is_undefined_not_empty() {
        let context = SeedContext {
            roots: BTreeMap::from([(
                "git".to_string(),
                Value::Table(table(&[("user.name", "Ada")])),
            )]),
        };
        let value = context.to_minijinja();
        let git = value.get_attr("git").unwrap();
        assert!(git.get_attr("nope").unwrap().is_undefined());
    }

    mod with_a_repository {
        use super::*;
        use crate::git::libgit2::LibGit2;

        fn repo(name: &str) -> (tempfile::TempDir, LibGit2) {
            let parent = tempfile::tempdir().unwrap();
            let path = parent.path().join(name);
            std::fs::create_dir(&path).unwrap();
            let repo = LibGit2::init(&path).unwrap();
            (parent, repo)
        }

        #[test]
        fn the_directory_name_is_the_worktree_name() {
            let (_dir, repo) = repo("my-project");
            let seeds = collect(&repo, "origin").unwrap();
            let Some(Value::Table(dir)) = seeds.roots.get("dir") else {
                panic!("expected a `dir` table");
            };
            assert_eq!(dir.get("name"), Some(&Value::String("my-project".into())));
        }

        /// A project that has never been pushed is an ordinary state, and a
        /// template seeded from the remote must still render there. `remote`
        /// stays a table so the lookup is undefined rather than an error.
        #[test]
        fn a_repository_with_no_remote_still_defines_the_remote_namespace() {
            let (_dir, repo) = repo("lonely");
            let seeds = collect(&repo, "origin").unwrap();
            assert_eq!(
                seeds.roots.get("remote"),
                Some(&Value::Table(BTreeMap::new()))
            );

            let value = seeds.to_minijinja();
            let remote = value.get_attr("remote").unwrap();
            assert!(remote.get_attr("name").unwrap().is_undefined());
        }

        #[test]
        fn a_remote_is_taken_apart_into_host_owner_and_name() {
            let (_dir, repo) = repo("checkout-name");
            repo.set_config_str("remote.origin.url", "git@github.com:me/git-tpl.git")
                .unwrap();

            let seeds = collect(&repo, "origin").unwrap();
            let Some(Value::Table(remote)) = seeds.roots.get("remote") else {
                panic!("expected a `remote` table");
            };
            assert_eq!(
                remote.get("host"),
                Some(&Value::String("github.com".into()))
            );
            assert_eq!(remote.get("owner"), Some(&Value::String("me".into())));
            assert_eq!(remote.get("name"), Some(&Value::String("git-tpl".into())));
            assert_eq!(
                remote.get("slug"),
                Some(&Value::String("me/git-tpl".into()))
            );
        }

        /// `tpl.remote` names which remote git-tpl works with, and a seed must
        /// describe that one — someone working from a fork would otherwise be
        /// offered their fork's owner when they meant upstream's.
        #[test]
        fn the_named_remote_is_the_one_described() {
            let (_dir, repo) = repo("forked");
            repo.set_config_str("remote.origin.url", "git@github.com:me/fork.git")
                .unwrap();
            repo.set_config_str("remote.upstream.url", "git@github.com:them/real.git")
                .unwrap();

            let seeds = collect(&repo, "upstream").unwrap();
            let Some(Value::Table(remote)) = seeds.roots.get("remote") else {
                panic!("expected a `remote` table");
            };
            assert_eq!(remote.get("name"), Some(&Value::String("real".into())));
        }

        #[test]
        fn a_configured_identity_is_reachable_under_git() {
            let (_dir, repo) = repo("identified");
            repo.set_config_str("user.name", "Ada Lovelace").unwrap();

            let seeds = collect(&repo, "origin").unwrap();
            let value = seeds.to_minijinja();
            let name = value.get_attr("git").unwrap().get_attr("user").unwrap();
            assert_eq!(
                name.get_attr("name").unwrap().as_str(),
                Some("Ada Lovelace")
            );
        }
    }
}
