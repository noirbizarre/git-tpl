# ADR-045: Cache Git source objects with fresh symbolic resolution

**Status:** Accepted.

## Context

Issue #201 makes repeated source acquisition a concrete cost as inheritance, composition and Git data multiply it.
The earlier fresh-clone trade-off avoided stale resolution but discarded reusable objects after every command.
Neither rendered output nor resolved context needs a new source of truth.

The manual `measure_source_resolution_costs` test compares the existing bare-clone constructor with acquisition.
On Linux, using the debug test binary, `file://`, 40 history updates of about 130 KB each, and ten iterations:

| Operation | Mean milliseconds per invocation |
|---|---:|
| Existing fresh bare clone and revision lookup | 1644.95 |
| Cold cache acquisition | 1731.94 |
| Warm full commit acquisition | 3.23 |
| Warm branch refresh | 5.03 |
| Four acquisitions of one source/ref in one invocation | 13.57 |

One additional commit refreshed in 6.44 ms; the resulting source cache occupied 470756 bytes.
These are local object-acquisition measurements, not internet latency or end-to-end rendering benchmarks.
They demonstrate that object reuse removes repeated transfer/packing cost; cold acquisition is not faster.
Tests assert object identity, freshness, and successful warm use without an origin, never timing thresholds.
Reproduce with:

```sh
cargo test --test cache measure_source_resolution_costs -- --ignored --nocapture
```

## Decision

Use versioned, machine-local bare repositories, shared by template and data acquisition below `ops`.
Pass an explicit `Access` context from commands through operations; resolved templates retain it for data and children.
Its successful name-to-OID resolutions are invocation-local and never become render context.

Full commit IDs may reuse complete cached commit/tree/blob objects without network access.
Other names refresh online with explicit heads/tags refspecs, deletion handling, and advertised default HEAD.
Failed refreshes never fall back to cached names.
A full commit ID is literal even when a tag has that hexadecimal spelling.
Ancestor pin restrictions and declaration-specific consent remain separate from object availability.

Identity preserves transport/account/port/path distinctions, including SSH versus HTTPS.
Do not reuse lossy template-ref slugging or trust normalization for cache keys.
Anonymous remotes use each request's credentials without retaining its URL in configuration or `FETCH_HEAD`.

Each entry has two stable external file locks:

1. A shared lifetime lease prevents removal while handles can read its objects.
2. An exclusive mutation lock serializes initialization, refresh, and name resolution.

Resolve to an OID under mutation exclusion and release that lock before prompting/rendering.
Fetches can add objects while other leases read older objects; no concurrent object GC or repacking is introduced.
Different entries do not share a global lock, and repeated-source recursive acquisition uses compatible read leases.
Repository handles drop before their leases, including on Windows.

Initialize in a unique staging directory, write disposable metadata, close handles, then rename into place.
The next initializer removes abandoned staging for that identity while holding its mutation lock.
Do not unlink lock files: waiters must continue to share the same inode after clear/recreation.
Clear/prune attempt exclusive leases and report busy skips; prune rechecks age under that lease.
Pruning removes whole entries rather than rewriting their object databases.

`--offline` blocks git-tpl-owned Git/HTTP network access and publication requests.
It permits cached full-commit requests and ordinary local repository access.
It does not sandbox explicitly authorized external test commands or native diff presentation.

## Consequences

Persistent objects are disposable optimization state, including any private source contents previously fetched.
Users can inspect, refresh, clear, and age-prune them explicitly.
Clearing removes offline availability and can expose historical commits that upstream no longer serves as unavailable.
Accumulated objects may make short IDs ambiguous; users must expand them rather than receive an arbitrary match.
Tags remain useful named pins but are refreshed because Git permits their movement.

The cache root, timestamps, locks, metadata and credentials never affect rendered bytes or project configuration.
The rendered-ref model, update working-state guarantees, Git backend isolation, and trust model remain intact.
