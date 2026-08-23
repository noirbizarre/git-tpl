//! Migrations: a message and path moves declared across a version boundary.
//!
//! No template ever declares a version, and no project ever records one — see
//! `docs/adr/024-template-migrations.md`. Instead, a migration is a file in
//! the template repository's [`MIGRATIONS_DIR`], and `git tpl update`
//! discovers which ones are *new* by diffing the template's own tree between
//! the previously recorded `Template-Commit` (already read back from
//! provenance, for display, before this existed) and the revision it just
//! resolved. A migration is "new" exactly once, at whichever `update` first
//! crosses it — which is also what keeps the noise down without any state of
//! its own.
//!
//! Two independent things a migration may declare:
//!
//! - `message`/`message_file`: shown once, through the same
//!   [`crate::note`] sanitiser and attributed block a template's `init`-time
//!   note uses. Resolving one against a project's answers needs a
//!   [`crate::context::Context`] and the template's own repository, both of
//!   which belong to `ops` — so that half lives in `ops::update`, mirroring
//!   `ops::template_note` exactly. This module only parses and validates.
//! - `moves`: applied to the rendered ref itself, ahead of the ordinary
//!   rendered commit, so that plain `git merge` — which git-tpl never runs
//!   and never configures — sees a content-identical rename rather than an
//!   unrelated delete and add.

use std::collections::{BTreeMap, BTreeSet};

use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::Deserialize;
use thiserror::Error;

use crate::git::{ChangeKind, FileMode, GitBackend, GitError, Oid, TreeEntry};

/// The directory, at the template repository root, that holds migrations.
///
/// Sibling to `template.toml`, in the same path namespace as `note_file` —
/// outside `root`, so nothing in it is ever itself rendered into a project.
pub const MIGRATIONS_DIR: &str = "migrations";

/// One path move a migration declares.
///
/// `from`/`to` are rendered *output* paths — the same namespace `git tpl
/// diff` reports — not template source paths, and are literal strings: a
/// migration cannot yet express a move whose destination depends on an
/// answer. See docs/adr/024, "explicitly out of scope".
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Move {
    /// Where the file was, in the previously rendered tree.
    pub from: String,
    /// Where it belongs now.
    pub to: String,
}

/// One migration, parsed from a file in [`MIGRATIONS_DIR`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    /// The migration file's path, repository-root-relative.
    ///
    /// Used in error messages and `--json`, and its sort order is the
    /// application order when several migrations are discovered together —
    /// see `ops::migration::discover_new`.
    pub path: String,
    /// A message shown once, the update that first discovers this migration.
    ///
    /// A literal, evaluated as an expression exactly like `Manifest::note`.
    pub message: Option<String>,
    /// A path to a message, instead of `message`.
    ///
    /// Mutually exclusive with it, exactly like `Manifest::note`/`note_file`,
    /// and for the same reason: a `message` moved into a file and left behind
    /// would otherwise show the stale half.
    pub message_file: Option<String>,
    /// Paths to move, applied in declaration order.
    pub moves: Vec<Move>,
}

/// The TOML shape a migration file is deserialised from.
///
/// Not [`Migration`] itself: the two error checks in [`parse`] need to run
/// after deserialisation succeeds, and keeping the wire shape separate is
/// what makes that ordering obvious rather than incidental.
#[derive(Debug, Deserialize)]
struct RawMigration {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    message_file: Option<String>,
    #[serde(default)]
    moves: Vec<Move>,
}

/// Errors from parsing or applying a migration.
#[derive(Debug, Error, Diagnostic)]
pub enum MigrationError {
    /// The file is not valid TOML, or does not match the schema.
    #[error("invalid migration in `{path}`: {message}")]
    #[diagnostic(code(tpl::migration::parse))]
    Parse {
        /// The migration file's path.
        path: String,
        /// The parser's message.
        message: String,
        #[source_code]
        /// The file, for the diagnostic snippet.
        src: NamedSource<String>,
        #[label("here")]
        /// Where in it the parser gave up.
        span: SourceSpan,
    },

    /// Both `message` and `message_file` are set.
    #[error("`{path}` sets both `message` and `message_file`")]
    #[diagnostic(
        code(tpl::migration::conflicting_message),
        help("a migration says its piece one way or the other, not both")
    )]
    ConflictingMessage {
        /// The migration file's path.
        path: String,
    },

    /// A move's `from` and `to` are the same, or either is empty.
    #[error("`{path}` declares an invalid move: `{from}` -> `{to}`")]
    #[diagnostic(
        code(tpl::migration::invalid_move),
        help("`from` and `to` must both be non-empty, and different from each other")
    )]
    InvalidMove {
        /// The migration file's path.
        path: String,
        /// The declared source.
        from: String,
        /// The declared destination.
        to: String,
    },

    /// Two moves declared in the same file land on the same destination.
    #[error("`{path}` moves more than one file to `{to}`")]
    #[diagnostic(
        code(tpl::migration::duplicate_target),
        help("each destination path may be the target of at most one move per file")
    )]
    DuplicateTarget {
        /// The migration file that declared the conflicting moves.
        path: String,
        /// The destination two moves share.
        to: String,
    },

    /// A declared `from` does not exist in the previous rendered tree.
    #[error(
        "migration move `{from}` -> `{to}` cannot be applied: `{from}` does not exist in the previous rendering"
    )]
    #[diagnostic(
        code(tpl::migration::move_source_missing),
        help(
            "a move's `from` must name a path the template's previous rendering \
             actually produced — check it against `git tpl diff --name-only`"
        )
    )]
    MoveSourceMissing {
        /// The declared source.
        from: String,
        /// The declared destination.
        to: String,
    },

    /// A declared `to` collides with something already there.
    #[error("migration move `{from}` -> `{to}` cannot be applied: `{to}` already exists")]
    #[diagnostic(
        code(tpl::migration::move_target_exists),
        help(
            "`to` must name a path the previous rendering did not already have — \
             check for an earlier move onto the same destination"
        )
    )]
    MoveTargetExists {
        /// The declared source.
        from: String,
        /// The declared destination, already occupied.
        to: String,
    },

    /// A Git operation failed while discovering or applying migrations.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Git(#[from] GitError),
}

/// Whether `path` is a migration file: under [`MIGRATIONS_DIR`], and `.toml`.
///
/// Anything else there — a `README.md` explaining the convention to template
/// authors, say — is left alone rather than treated as a broken migration.
pub fn is_migration_path(path: &str) -> bool {
    path.strip_prefix(&format!("{MIGRATIONS_DIR}/"))
        .is_some_and(|rest| rest.ends_with(".toml"))
}

/// Parse one migration file, and validate what does not need a project.
///
/// Structural only — no expression is evaluated here, so this is exactly what
/// `lint` runs, with no repository and no answers. Resolving `message`/
/// `message_file` against a project's answers is `ops::update`'s job, done
/// the same way `ops::template_note` resolves `note`/`note_file`.
pub fn parse(text: &str, path: &str) -> Result<Migration, MigrationError> {
    let raw: RawMigration = toml::from_str(text).map_err(|error| {
        let span = error
            .span()
            .map(|s| SourceSpan::from(s.start..s.end))
            .unwrap_or_else(|| SourceSpan::from(0..0));
        MigrationError::Parse {
            path: path.to_string(),
            message: error.message().to_string(),
            src: NamedSource::new(path, text.to_string()),
            span,
        }
    })?;

    // One message, or none — the same rule `Manifest::note`/`note_file`
    // enforces, and for the same reason.
    if raw.message.is_some() && raw.message_file.is_some() {
        return Err(MigrationError::ConflictingMessage {
            path: path.to_string(),
        });
    }

    let mut targets = BTreeSet::new();
    for mv in &raw.moves {
        if mv.from.trim().is_empty() || mv.to.trim().is_empty() || mv.from == mv.to {
            return Err(MigrationError::InvalidMove {
                path: path.to_string(),
                from: mv.from.clone(),
                to: mv.to.clone(),
            });
        }
        // Checked within this one file only: a target two *different*
        // migration files both claim is a `move_target_exists` at apply
        // time, once their order relative to each other is known.
        if !targets.insert(mv.to.clone()) {
            return Err(MigrationError::DuplicateTarget {
                path: path.to_string(),
                to: mv.to.clone(),
            });
        }
    }

    Ok(Migration {
        path: path.to_string(),
        message: raw.message,
        message_file: raw.message_file,
        moves: raw.moves,
    })
}

/// Migration files newly present in the template's tree since the project's
/// previous rendering, in path order.
///
/// "New" means present in `new_tree` and absent from `old_tree` — a plain
/// tree diff, scoped to [`MIGRATIONS_DIR`]. No version and no history walk: a
/// migration added and then edited several template commits later is still
/// discovered exactly once, at whichever `update` first crosses it, because
/// that is the only property a two-tree diff can see.
///
/// Consequence, recorded here because it is easy to violate by accident: a
/// migration file must never be deleted from the template repository once
/// shipped. Doing so removes it from every future diff too, and a project
/// that skipped both its addition and its removal would never discover it.
/// See docs/adr/024.
pub fn discover_new(
    template: &dyn GitBackend,
    old_tree: Oid,
    new_tree: Oid,
) -> Result<Vec<(String, Vec<u8>)>, GitError> {
    let changes = template.diff_trees(Some(old_tree), new_tree, &[MIGRATIONS_DIR.to_string()])?;

    let mut discovered = Vec::new();
    for change in changes {
        if change.kind != ChangeKind::Added || !is_migration_path(&change.path) {
            continue;
        }
        // `Added` at `new_tree` — reading it there cannot legitimately come
        // back empty, but a defensive `if let` beats an `expect` two calls
        // away from what it is proving.
        if let Some(bytes) = template.read_path(new_tree, &change.path)? {
            discovered.push((change.path, bytes));
        }
    }

    // `diff_trees` already returns changes in path order, and filtering
    // preserves it.
    Ok(discovered)
}

/// Apply a batch of moves to `base_tree`, returning the resulting tree.
///
/// Returns `Ok(None)` when `moves` is empty: no intermediate tree is needed,
/// and `update` creates no extra commit for it.
///
/// Moves are applied in order, each against the running result of the ones
/// before it — so a later migration's `from` may legally be an earlier one's
/// `to`, chaining two migrations that touch the same path within the same
/// `update`.
///
/// Every moved entry keeps its `oid` and `mode` untouched. That is the whole
/// mechanism: the blob at the new path is byte-identical to the blob that was
/// at the old one, which is what makes plain `git merge`'s own
/// similarity-based rename detection reliably attribute the move to the
/// user's own edits rather than losing them to an unrelated delete and add.
/// See docs/adr/024.
pub fn apply_moves(
    project: &dyn GitBackend,
    base_tree: Oid,
    moves: &[Move],
) -> Result<Option<Oid>, MigrationError> {
    if moves.is_empty() {
        return Ok(None);
    }

    let mut entries: BTreeMap<String, (Oid, FileMode)> = project
        .list_tree(base_tree)?
        .into_iter()
        .map(|entry| (entry.path, (entry.oid, entry.mode)))
        .collect();

    for mv in moves {
        let Some(moved) = entries.remove(&mv.from) else {
            return Err(MigrationError::MoveSourceMissing {
                from: mv.from.clone(),
                to: mv.to.clone(),
            });
        };
        if entries.contains_key(&mv.to) {
            return Err(MigrationError::MoveTargetExists {
                from: mv.from.clone(),
                to: mv.to.clone(),
            });
        }
        entries.insert(mv.to.clone(), moved);
    }

    let tree_entries: Vec<TreeEntry> = entries
        .into_iter()
        .map(|(path, (oid, mode))| TreeEntry { path, oid, mode })
        .collect();

    Ok(Some(project.build_tree(&tree_entries)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::libgit2::LibGit2;

    fn scratch() -> (tempfile::TempDir, LibGit2) {
        let dir = tempfile::tempdir().unwrap();
        let repo = LibGit2::init(dir.path()).unwrap();
        repo.set_config_str("user.name", "Test").unwrap();
        repo.set_config_str("user.email", "test@example.invalid")
            .unwrap();
        (dir, repo)
    }

    #[test]
    fn a_migration_with_neither_message_nor_moves_parses() {
        let migration = parse("", "migrations/000-empty.toml").unwrap();
        assert_eq!(migration.message, None);
        assert!(migration.moves.is_empty());
    }

    #[test]
    fn a_message_and_a_message_file_together_are_rejected() {
        let text = "message = \"hi\"\nmessage_file = \"NOTE.md\"\n";
        let error = parse(text, "migrations/x.toml").unwrap_err();
        assert!(matches!(error, MigrationError::ConflictingMessage { .. }));
    }

    #[test]
    fn a_move_to_itself_is_rejected() {
        let text = "[[moves]]\nfrom = \"a\"\nto = \"a\"\n";
        let error = parse(text, "migrations/x.toml").unwrap_err();
        assert!(matches!(error, MigrationError::InvalidMove { .. }));
    }

    #[test]
    fn two_moves_onto_the_same_target_are_rejected() {
        let text = "[[moves]]\nfrom = \"a\"\nto = \"c\"\n[[moves]]\nfrom = \"b\"\nto = \"c\"\n";
        let error = parse(text, "migrations/x.toml").unwrap_err();
        assert!(matches!(error, MigrationError::DuplicateTarget { .. }));
    }

    #[test]
    fn moves_are_applied_by_dropping_the_source_and_keeping_the_blob() {
        let (_dir, repo) = scratch();
        let blob = repo.write_blob(b"content\n").unwrap();
        let base = repo
            .build_tree(&[TreeEntry {
                path: "src/config.rs".into(),
                oid: blob,
                mode: FileMode::Blob,
            }])
            .unwrap();

        let moved = apply_moves(
            &repo,
            base,
            &[Move {
                from: "src/config.rs".into(),
                to: "src/config/mod.rs".into(),
            }],
        )
        .unwrap()
        .expect("a tree, since there is a move");

        let entries = repo.list_tree(moved).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src/config/mod.rs");
        // Byte-identical: the whole point is that Git sees the same blob at
        // the new path, not a new one that merely looks similar.
        assert_eq!(entries[0].oid, blob);
    }

    #[test]
    fn no_moves_means_no_intermediate_tree() {
        let (_dir, repo) = scratch();
        let base = repo.build_tree(&[]).unwrap();
        assert_eq!(apply_moves(&repo, base, &[]).unwrap(), None);
    }

    #[test]
    fn a_chained_move_may_target_what_an_earlier_move_just_vacated() {
        let (_dir, repo) = scratch();
        let blob = repo.write_blob(b"content\n").unwrap();
        let base = repo
            .build_tree(&[TreeEntry {
                path: "a".into(),
                oid: blob,
                mode: FileMode::Blob,
            }])
            .unwrap();

        let moved = apply_moves(
            &repo,
            base,
            &[
                Move {
                    from: "a".into(),
                    to: "b".into(),
                },
                Move {
                    from: "b".into(),
                    to: "c".into(),
                },
            ],
        )
        .unwrap()
        .unwrap();

        let entries = repo.list_tree(moved).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "c");
    }

    #[test]
    fn a_move_whose_source_is_absent_is_refused() {
        let (_dir, repo) = scratch();
        let base = repo.build_tree(&[]).unwrap();
        let error = apply_moves(
            &repo,
            base,
            &[Move {
                from: "missing".into(),
                to: "somewhere".into(),
            }],
        )
        .unwrap_err();
        assert!(matches!(error, MigrationError::MoveSourceMissing { .. }));
    }

    #[test]
    fn a_move_onto_an_occupied_path_is_refused() {
        let (_dir, repo) = scratch();
        let a = repo.write_blob(b"a\n").unwrap();
        let b = repo.write_blob(b"b\n").unwrap();
        let base = repo
            .build_tree(&[
                TreeEntry {
                    path: "a".into(),
                    oid: a,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "b".into(),
                    oid: b,
                    mode: FileMode::Blob,
                },
            ])
            .unwrap();

        let error = apply_moves(
            &repo,
            base,
            &[Move {
                from: "a".into(),
                to: "b".into(),
            }],
        )
        .unwrap_err();
        assert!(matches!(error, MigrationError::MoveTargetExists { .. }));
    }

    #[test]
    fn only_added_toml_files_under_the_migrations_directory_are_discovered() {
        let (_dir, repo) = scratch();
        let old = repo.build_tree(&[]).unwrap();

        let migration_blob = repo.write_blob(b"message = \"hi\"\n").unwrap();
        let readme_blob = repo.write_blob(b"how migrations work here\n").unwrap();
        let unrelated_blob = repo.write_blob(b"fn main() {}\n").unwrap();
        let new = repo
            .build_tree(&[
                TreeEntry {
                    path: "migrations/000-x.toml".into(),
                    oid: migration_blob,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "migrations/README.md".into(),
                    oid: readme_blob,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "template/main.rs".into(),
                    oid: unrelated_blob,
                    mode: FileMode::Blob,
                },
            ])
            .unwrap();

        let discovered = discover_new(&repo, old, new).unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].0, "migrations/000-x.toml");
    }

    /// The property the whole discovery mechanism depends on: a migration
    /// found once must not resurface once the project's baseline has moved
    /// past it.
    #[test]
    fn a_migration_already_present_in_the_old_tree_is_not_rediscovered() {
        let (_dir, repo) = scratch();
        let migration_blob = repo.write_blob(b"message = \"hi\"\n").unwrap();
        let old = repo
            .build_tree(&[TreeEntry {
                path: "migrations/000-x.toml".into(),
                oid: migration_blob,
                mode: FileMode::Blob,
            }])
            .unwrap();

        let other_blob = repo.write_blob(b"fn main() {}\n").unwrap();
        let new = repo
            .build_tree(&[
                TreeEntry {
                    path: "migrations/000-x.toml".into(),
                    oid: migration_blob,
                    mode: FileMode::Blob,
                },
                TreeEntry {
                    path: "template/main.rs".into(),
                    oid: other_blob,
                    mode: FileMode::Blob,
                },
            ])
            .unwrap();

        assert_eq!(discover_new(&repo, old, new).unwrap(), Vec::new());
    }
}
