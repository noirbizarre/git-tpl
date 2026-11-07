# ADR-049: Snapshot manifests record paths and modes, not content metadata

## Status

Accepted.
Supersedes the snapshot serialization and checksum/size assertions in
[ADR-016](016-template-tests-are-data.md) and the manifest format in [ADR-044](044-symlink-artifacts.md).

## Context

Each snapshot already stores expected bytes verbatim in `files/`.
Recording those bytes' checksums and sizes again in a shared `MANIFEST` creates unnecessary merge conflicts
when independent PRs update different rendered files.
It also prevents expectation-first testing: editing expected contents produces a metadata-integrity error
instead of the ordinary comparison failure that should guide the template change.

Paths and modes remain useful shared metadata.
They distinguish expected output kinds across platforms, including Windows checkouts that lose executable bits,
and allow link targets to remain safe, ordinary payload files.

## Decision

Keep the snapshot directory and `files/` payload layout.
Replace the line-based manifest with `manifest.toml`:

```toml
version = 2

[files]
"README.md" = "100644"
"bin/run.sh" = "100755"
"current" = "120000"
```

Sort entries by path and serialize paths as TOML keys with appropriate escaping.
Only the version, path inventory and modes are stored.
No checksum, size or binary marker is recorded.
The payload bytes are the expected contents, and comparison detects binary files directly from those bytes.
An empty `[files]` table records an empty output, surviving Git's omission of empty directories.

Preserve the inventory agreement check and mode comparison.
Link payloads remain ordinary files containing opaque target bytes.
Keep revision-aware reads, ignore-independent working-tree reads, write-side link protections,
and unstaged, uncommitted snapshot recording.
[ADR-032](032-write-only-writes.md) still governs `--write`: recording replaces expectations with actual output.

## Compatibility

This changes the on-disk snapshot contract; older readers cannot read new snapshots.
Read legacy `MANIFEST` snapshots using their paths and modes while ignoring checksums and sizes as content assertions.
Continue validating their structure and inventory.
Plain comparison never migrates files.

On explicit `--write`, record TOML and remove any legacy `MANIFEST`, including when output is unchanged.
If both manifests exist, TOML is authoritative; invalid TOML must not fall back to stale legacy data.
A format-only update uses existing outcome `updated` with an empty semantic change list.
Subsequent equal writes report `unchanged` and do not rewrite files.

## Consequences

Content-only changes modify only the payload files in Git diffs.
Authors can edit expected contents first and change the template until a plain test run passes.
Additions, deletions and mode changes still edit the shared inventory and can still conflict there.
No per-file metadata mechanism or new snapshot layout is needed.
