//! The seed context: machine facts a prompt may be pre-filled from.
//!
//! A template asking for a project slug or a repository owner can usually guess
//! the answer from the project it is being applied to. This module gathers the
//! facts that guess is made of — the Git configuration, the working directory
//! name, the remote URL, the current UTC datetime, the OS login name — and exposes them to a
//! `default_from` expression.
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
//! | `now` | UTC calendar/time components and ISO date, time and datetime strings |
//! | `user` | `user.name`, the current OS account's login name |
//!
//! The set is closed. See ADR-018 and its extensions, ADR-056 and ADR-057.

use std::collections::BTreeMap;
use time::{OffsetDateTime, UtcOffset};

use crate::git::{GitBackend, GitError};
use crate::remote;
use crate::template::Value;

/// The closed namespaces a `default_from` expression may read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeedContext {
    roots: BTreeMap<String, Value>,
}

/// The namespaces a `default_from` expression may name.
///
/// Public so that manifest validation can reject an unknown root at load time,
/// where the author sees it, rather than at prompt time on someone else's
/// machine.
pub const NAMESPACES: [&str; 5] = ["dir", "git", "remote", "now", "user"];

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
    // Capture once in the lazy prompt-only context: separate prompts must not
    // disagree across midnight, and replaying recorded answers never reads time.
    roots.insert(
        "now".to_string(),
        Value::Table(now_table(OffsetDateTime::now_utc())),
    );
    // Like the clock, account identity is captured only for interactive seed
    // suggestions and reused until this render's prompts have finished.
    roots.insert(
        "user".to_string(),
        Value::Table(user_table(whoami::username_os().ok())),
    );

    Ok(SeedContext { roots })
}

/// An unavailable account leaves an undefined leaf so template fallbacks work.
fn user_table(name: Option<std::ffi::OsString>) -> BTreeMap<String, Value> {
    let mut table = BTreeMap::new();
    // Lossy conversion would suggest a different login; lookup failure must
    // never prevent an otherwise usable interactive prompt from appearing.
    if let Some(name) = name.and_then(|name| name.into_string().ok())
        && !name.trim().is_empty()
    {
        table.insert("name".into(), Value::String(name));
    }
    table
}

/// Derive every representation from one UTC instant, including at date boundaries.
fn now_table(instant: OffsetDateTime) -> BTreeMap<String, Value> {
    let instant = instant.to_offset(UtcOffset::UTC);
    let (year, month, day) = instant.to_calendar_date();
    let month = u8::from(month);
    let (hour, minute, second) = instant.to_hms();
    // Explicit whole-second ISO forms avoid locale, offset and fractional-second
    // differences in suggestions that may later be recorded as answers.
    let date = format!("{year:04}-{month:02}-{day:02}");
    let time = format!("{hour:02}:{minute:02}:{second:02}Z");
    let datetime = format!("{date}T{time}");
    BTreeMap::from([
        ("year".into(), Value::Integer(i64::from(year))),
        ("month".into(), Value::Integer(i64::from(month))),
        ("day".into(), Value::Integer(i64::from(day))),
        ("hour".into(), Value::Integer(i64::from(hour))),
        ("minute".into(), Value::Integer(i64::from(minute))),
        ("second".into(), Value::Integer(i64::from(second))),
        ("date".into(), Value::String(date)),
        ("time".into(), Value::String(time)),
        ("datetime".into(), Value::String(datetime)),
    ])
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
    /// All roots are always present, possibly empty. One defined level of
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

    #[test]
    fn a_login_name_is_preserved_verbatim() {
        assert_eq!(
            user_table(Some(" ada ".into())).get("name"),
            Some(&Value::String(" ada ".into()))
        );
    }

    #[test]
    fn unavailable_or_blank_login_names_allow_expression_fallbacks() {
        for name in [None, Some("".into()), Some(" \t".into())] {
            let user = user_table(name);
            assert!(user.is_empty());
            let seeds =
                SeedContext::from_roots(BTreeMap::from([("user".into(), Value::Table(user))]));
            assert_eq!(
                crate::eval::render_seed("{{ user.name | default('developer') }}", &seeds, "test")
                    .unwrap(),
                "developer"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_login_is_absent_rather_than_mangled() {
        use std::os::unix::ffi::OsStringExt;

        assert!(user_table(Some(std::ffi::OsString::from_vec(vec![0xff]))).is_empty());
    }

    #[test]
    fn datetime_seeds_share_one_utc_instant_and_use_whole_second_iso_strings() {
        let instant = time::Date::from_calendar_date(2024, time::Month::February, 29)
            .unwrap()
            .with_hms_nano(3, 4, 5, 123_456_789)
            .unwrap()
            .assume_utc();
        let now = now_table(instant);
        assert_eq!(now.len(), 9);
        for (field, value) in [
            ("year", 2024),
            ("month", 2),
            ("day", 29),
            ("hour", 3),
            ("minute", 4),
            ("second", 5),
        ] {
            assert_eq!(now[field], Value::Integer(value));
        }
        for (field, value) in [
            ("date", "2024-02-29"),
            ("time", "03:04:05Z"),
            ("datetime", "2024-02-29T03:04:05Z"),
        ] {
            assert_eq!(now[field], Value::String(value.into()));
        }
        let seeds = SeedContext::from_roots(BTreeMap::from([("now".into(), Value::Table(now))]));
        assert_eq!(
            crate::eval::render_seed("{{ now.year }}", &seeds, "test").unwrap(),
            "2024"
        );
    }

    #[test]
    fn datetime_seeds_normalize_to_utc_before_deriving_the_year() {
        let local = time::Date::from_calendar_date(2027, time::Month::January, 1)
            .unwrap()
            .with_hms(0, 30, 0)
            .unwrap()
            .assume_offset(UtcOffset::from_hms(1, 0, 0).unwrap());
        let now = now_table(local);
        assert_eq!(now["year"], Value::Integer(2026));
        assert_eq!(
            now["datetime"],
            Value::String("2026-12-31T23:30:00Z".into())
        );
        let next_year = now_table(local + time::Duration::minutes(30));
        assert_eq!(next_year["year"], Value::Integer(2027));
        assert_eq!(
            next_year["datetime"],
            Value::String("2027-01-01T00:00:00Z".into())
        );
    }

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
