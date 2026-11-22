# `git tpl cache`

Unused data declarations do not acquire sources, including through unused computed values or untaken conditions.

Git source objects are retained between invocations in bare repositories.
Templates, inherited ancestors, composed children, Git data, and source history share this storage.
Checksum-pinned HTTP data is retained as raw content addressed by its declared SHA-256.
Rendered output, answers, contexts, and authoritative template state never live here.
Clearing it changes acquisition cost and offline availability, not the meaning of a full commit ID or checksum.

## Location

The first available location wins:

1. `--cache-dir /absolute/path`
2. `GIT_TPL_CACHE_DIR=/absolute/path`
3. `$XDG_CACHE_HOME/git-tpl`, when `XDG_CACHE_HOME` is absolute
4. `$HOME/.cache/git-tpl`, falling back to `USERPROFILE` when `HOME` is unavailable

An empty or relative `XDG_CACHE_HOME` is ignored.
Explicit overrides must be absolute.
If no location can be determined, supply `--cache-dir`.
Commands using only filesystem repositories and unpinned HTTP data need no cache.
Library callers loading pinned HTTP data must supply an explicit root through `Loader::with_access` and `Access::new`.
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

HTTP data with a declared `sha256` can be used offline after its verified content has been cached online.
Unpinned HTTP data is unavailable offline, even if another declaration cached the same URL's content.
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
git tpl cache clear sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
git tpl cache clear
git tpl cache prune --older-than 30
```

`list` reports the cache root in JSON and entries sorted by ID, including `kind`, source label, bytes, and last-use time.
Git entries have `kind = "git"` and retain their opaque repository IDs and redacted source labels.
HTTP entries have `kind = "http"` and use `sha256:<digest>` as both ID and source label.
`clear SOURCE` removes one Git source; `clear sha256:<digest>` removes one HTTP object shared by any URLs using that pin.
The `sha256:` selector is reserved and accepts exactly 64 hexadecimal characters, case-insensitively.
`clear` removes all inactive entries when the selector is omitted.
`prune` removes whole entries unused for the requested number of days; the default is 30.
Age zero selects all inactive entries.
There is no automatic eviction and no object-level garbage collection.
Maintenance skips entries held by an active operation; JSON reports `removed` and `skipped` ID arrays.
Retry later to remove busy entries.
Stable lock files remain after deletion so concurrent processes cannot lock different inodes for the same entry.

Initialization publishes only complete entries; abandoned initialization is recovered on the next publication.
Corrupt entries can be removed with `cache clear` and fetched again.
Pruning takes an exclusive entry lease and rechecks last use before deletion.
Operations on unrelated entries use independent locks.

## HTTP content identity and integrity

Only HTTP sources with a declared `sha256` use persistent storage.
The normalized SHA-256 identifies raw response bytes, independently of URL and parsing format.
Every acquisition verifies the stored body against the declared checksum before parsing it.
Trust checks and declaration-specific format validation still apply on a hit.
Changing a URL while retaining its checksum can reuse the same content without contacting either server.

On a miss, an online render downloads and verifies the body before atomically publishing the complete object.
Concurrent renders serialize acquisition of the same digest; no reader observes a partially written body.
There is no expiration, revalidation, stale fallback, or automatic corruption repair.
A corrupt entry fails online and offline; clear its reported `sha256:<digest>` selector, then retry online.
An offline miss identifies the required checksum and asks you to populate it online first.
`cache fetch` remains a Git operation; warm HTTP content by rendering a template that uses the pin.

## Repository identity and credentials

Repository keys are conservative hashes of source identity.
Transport, account, port, path case, `.git`, and trailing-slash spellings remain distinct.
SSH and HTTPS URLs, including SSH host aliases, are not automatically merged.
Filesystem paths used for identity are canonicalized when available.
Use the same source spelling to maximize reuse.

Each fetch uses the requested transport and the existing SSH agent/key and HTTPS credential-helper behavior.
Credential-bearing URLs are not stored in remote config, `FETCH_HEAD`, or cache metadata.
Inspection labels omit URL userinfo, query strings, and fragments.
The cache can contain private repository contents and HTTP response bodies.
Clear entries when you no longer want those objects retained.

See [ADR-045](../adr/045-source-object-cache.md) for the locking and resolution design.
The HTTP extension is described in [ADR-051](../adr/051-checksum-http-cache.md).
