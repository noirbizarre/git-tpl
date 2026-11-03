# `git tpl cache`

Unused data declarations do not acquire sources, including through unused computed values or untaken conditions.

Git source objects are retained between invocations in bare repositories.
Templates, inherited ancestors, composed children, Git data, and source history share this storage.
Rendered output, answers, contexts, and authoritative template state never live here.
Clearing it changes acquisition cost and offline availability, not the meaning of a full commit ID.

## Location

The first available location wins:

1. `--cache-dir /absolute/path`
2. `GIT_TPL_CACHE_DIR=/absolute/path`
3. `$XDG_CACHE_HOME/git-tpl`, when `XDG_CACHE_HOME` is absolute
4. `$HOME/.cache/git-tpl`, falling back to `USERPROFILE` when `HOME` is unavailable

An empty or relative `XDG_CACHE_HOME` is ignored.
Explicit overrides must be absolute.
If no location can be determined, supply `--cache-dir`; commands using only filesystem repositories need no cache.
The versioned layout is an implementation detail.
Use a local filesystem supporting shared/exclusive file locks and atomic renames.

## Freshness

- A full 40-character commit ID uses its cached commit, trees and blobs without contacting the origin.
  Missing objects are fetched online; unavailable commits fail with the underlying Git diagnostic.
- Branches, tags, abbreviated IDs, revspecs, and the default branch refresh online before resolution.
  Moved/deleted branches and tags, including forced updates, are reflected in the next invocation.
- Successful resolutions of the same source/ref are frozen to an object ID for the rest of that invocation.
  A concurrent refresh cannot change the revision already being rendered.
- A failed refresh is an error, never permission to use an old symbolic-ref mapping.
  `status` keeps its usual best-effort behavior and reports availability as unknown.

There is no TTL and no separate `--refresh` flag.
Normal online resolution refreshes names; explicitly warm or refresh a repository with:

```sh
git tpl cache fetch https://github.com/acme/template
```

A full hexadecimal commit ID names that commit even if an identically named tag exists.
Short IDs must remain unambiguous; retained history can make a previously unique abbreviation ambiguous.
Use full IDs for reproducibility and offline work.
Tags refresh because servers can move them; a full commit ID is the immutable identifier.
Previously fetched commits may remain available after upstream history is rewritten, including for migrations.
Do not depend on the cache to retain otherwise inaccessible source history permanently.

## Offline

```sh
git tpl render https://github.com/acme/template --ref FULL_COMMIT_ID --defaults -o ./generated
git tpl --offline render https://github.com/acme/template --ref FULL_COMMIT_ID --defaults -o ./generated --force
```

`--offline` prevents git-tpl-owned network access.
Every remote template, ancestor, composed child, or Git data reference actually used must name a cached full commit.
Cached branch/tag mappings and short IDs are deliberately insufficient.
Ordinary filesystem repositories still support their local refs and dirty worktrees.
`file://` sources use the clone/cache path and follow the same full-commit rule as other URLs.
Trust checks still apply to cached objects.

HTTP data is unavailable offline, including checksum-pinned HTTP data.
Unused declarations remain lazy and do not fail merely because offline is set.
`fetch`, `push`, `cache fetch`, and updates requesting publication through `--push` or `tpl.autoPush` are refused.
Disable `tpl.autoPush` before an offline update; publication conflicts are checked before advancing the rendered ref.

This flag is not an OS network sandbox.
Explicit test-case commands and native diff drivers remain external programs with their own behavior.
Git credential helpers are used only when acquisition is permitted to contact a source.

## Inspect and maintain

These commands need no project repository, and all accept `--json`:

```sh
git tpl cache list
git tpl --json cache list
git tpl cache clear https://github.com/acme/template
git tpl cache clear
git tpl cache prune --older-than 30
```

`list` reports the cache root in JSON and entries sorted by ID, including redacted source, bytes, and last-use time.
`clear` removes one source or all repositories when the source is omitted.
`prune` removes whole repositories unused for the requested number of days; the default is 30.
Age zero selects all inactive entries.
There is no automatic eviction and no object-level garbage collection.
Maintenance skips entries held by an active operation; JSON reports `removed` and `skipped` ID arrays.
Retry later to remove busy entries.
Stable lock files remain after deletion so concurrent processes cannot lock different inodes for the same entry.

Initialization publishes only complete repositories; abandoned initialization is recovered on the next acquisition.
Corrupt entries can be removed with `cache clear` and fetched again.
Pruning takes an exclusive entry lease and rechecks last use before deletion.
Operations on unrelated repositories use independent locks.

## Repository identity and credentials

Repository keys are conservative hashes of source identity.
Transport, account, port, path case, `.git`, and trailing-slash spellings remain distinct.
SSH and HTTPS URLs, including SSH host aliases, are not automatically merged.
Filesystem paths used for identity are canonicalized when available.
Use the same source spelling to maximize reuse.

Each fetch uses the requested transport and the existing SSH agent/key and HTTPS credential-helper behavior.
Credential-bearing URLs are not stored in remote config, `FETCH_HEAD`, or cache metadata.
Inspection labels omit URL userinfo, query strings, and fragments.
The cache can contain private repository contents; clear entries when you no longer want those objects retained.

See [ADR-045](../adr/045-source-object-cache.md) for the locking and resolution design.
