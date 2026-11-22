# ADR-051: Cache checksum-pinned HTTP bodies by content

**Status:** Accepted.

## Context

Issue #223 extends the disposable source cache introduced in [ADR-045](045-source-object-cache.md).
Repeated HTTP downloads are unnecessary when a declaration already identifies the exact required bytes by SHA-256.
An unpinned URL has no immutable identity from which to establish freshness or offline integrity.

## Decision

Retain raw HTTP bodies only for declarations with a valid SHA-256 pin.
Normalize the checksum to lowercase and store objects in a separate `v1/http/<digest>` namespace.
The identity is independent of URL and format: each declaration separately authorizes, verifies, parses, and records provenance.
HTTP transport remains in `src/data/`; shared storage and maintenance use the existing explicit `Access` context.
Library callers must configure a cache root for used pinned HTTP sources; unused declarations remain lazy.

Validate consent and the HTTP(S) scheme before cache lookup.
Read cached bodies with the same size bound as downloaded responses and verify their checksum on every acquisition.
A cache hit satisfies immutable content identity, without revalidation, expiration, or network fallback.
Offline pinned misses identify the required source and checksum and explain how to populate them online.
Unpinned HTTP never reads or writes persistent content and remains unavailable offline.

Each HTTP object has stable, separately namespaced lease and mutation locks.
Hold a shared lease and exclusive mutation lock through lookup, verification, and any fetch/publication.
Publish body and disposable metadata together by staging in the same filesystem and atomically renaming into place.
Concurrent successful cold acquisitions perform one download; unrelated digests remain independent.
Once bytes are owned in memory, release the locks before parsing or rendering.
Recover this digest's abandoned staging under mutation exclusion, and never unlink lock files.

Only an absent final entry is a miss.
Corrupt, oversized, or incomplete objects fail online and offline without automatic repair.
The diagnostic gives `git tpl cache clear sha256:<digest>` followed by an online retry as recovery.
Checksum-verified bytes may be published before parsing because interpretation belongs to the declaration, not storage.

Pinned declarations prefer their verified digest object over any invocation-local URL snapshot.
If no object exists online, a previously acquired URL snapshot may populate it after checksum verification.
An existing URL snapshot is not overwritten by a different pinned object.
An unpinned declaration cannot acquire offline semantics from a pinned alias's in-memory bytes.

Extend list, unqualified clear, and age-prune to both storage kinds, preserving busy-entry skips.
Inspection adds `kind` while retaining existing Git IDs and fields; HTTP IDs and labels use `sha256:<digest>`.
The reserved `sha256:` selector targets HTTP content independently of source URLs or aliases.
Selective clear does not require valid body or metadata.
`cache fetch SOURCE` remains a Git branch/tag refresh operation.

## Consequences

Verified HTTP content can be shared across invocations and URLs, and reused offline after population.
Private response bodies may persist until explicitly cleared or pruned.
Clearing removes offline availability but cannot change the meaning of a checksum.
Cache roots, timestamps, metadata, and hit state never affect rendered bytes, project configuration, or provenance.
Trust, deterministic rendering, append-only refs, and update working-state guarantees remain intact.
