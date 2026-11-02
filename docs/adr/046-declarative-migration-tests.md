# 046 — Migration cases assert a real updated and merged project

Status: accepted.
Extends [ADR-016](016-template-tests-are-data.md)
and supersedes the migration-testing deferral in [ADR-024](024-template-migrations.md).

## Context

A successful rendering cannot prove that a migration applies to an old project
or that Git preserves its customisations when paths move.
Migration tests need two resolved revisions and a genuine merge base.
Template authors should be able to describe that history without executable setup code.

## Decision

Existing case files may declare `[migration]`, with a required `from` reference
and an optional `to` reference in the same local template repository.
Without `to`, the destination is the suite's captured template, including its dirty tree.
Cases and snapshots are always selected from the suite target.

The harness creates an isolated real Git project through the production initialization path.
`migration.answers` supplies the old answers; omission copies the case's top-level answers.
An explicitly empty old-answer table remains empty.
Literal `migration.files` writes and `migration.absent` deletions become committed project customisations.
They cannot address Git metadata or the harness's attachment configuration.
Tree edits replace symbolic links without following them and retain executable modes on existing regular files.

Production update merges recorded old answers with top-level target overrides, discovers migrations,
applies their moves, publishes append-only history, and records changed answers and child locks.
The harness uses the same orchestration with captured resolved input, avoiding a second migration engine
or a second read of a dirty destination.
Old supplied answers and target overrides are strict; obsolete recorded answers retain update's ordinary semantics.

The harness then merges the updated ref into the customised project using the Git backend.
Conflicts fail the case with the paths reported by Git.
Existing path, substring and snapshot assertions inspect the merged project tree,
including project-owned files but excluding `.config/git.tpl.toml` and Git metadata.
The attachment is harness bookkeeping and would otherwise put machine-specific source paths into snapshots.

`expect.migrations` asserts the exact ordered discovery list; omission makes no assertion.
`expect.messages` asserts substrings in template-authored evaluated messages, keyed by migration path.
Destination update failures may be asserted by diagnostic code using `expect.error`.
Preparation failures cannot satisfy that assertion.
Failures retain endpoints, phase and crossed migrations; failed moves retain their declaring migration path
without changing existing diagnostic codes.

A direct `from` → `to` case performs one update, not a series of historical upgrades.
Discovery compares endpoint trees; newly added migration files run in path order,
and moves run in declaration order against the running old rendered tree.
Messages use the destination context.
Intermediate template commits are not individually rendered.
Authors use separate cases with explicit destinations to verify intermediate upgrades as well as skipped-version jumps.

Migration execution is data-only and does not depend on command enablement.
Existing optional command lists keep their lifecycle: `before` precedes initialization,
`rendered` sees the completed merged project, `after` follows assertions, and `finally` always runs.
The migration repository also serves as the command sandbox, with optional `git` identity overrides.
Commands do not mutate the immutable artifact checked by expectations.

Snapshot recording follows [ADR-032](032-write-only-writes.md):
`--write` skips nonsnapshot cases, executes declarative preparation to obtain the merged artifact,
and checks no expectations or commands.
An initialization, update or merge failure prevents recording.
Scratch commits never stage or commit anything in the user's template checkout.

## Consequences

Ordinary render cases remain valid.
Migration cases test the same history that users update and merge, including Git's actual rename and conflict behavior.
They do not promise that every move plus arbitrary rewrite merges cleanly.
Literal text edits cover committed project changes; binary fixture overlays and expected-conflict assertions
can be designed separately if needed.
