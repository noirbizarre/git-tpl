# ADR-044: Rendered artifacts preserve symbolic links

## Status

Accepted.

## Context

An executable-only artifact loses Git's `120000` kind and can turn a link target into a rendered regular file.
Dirty traversal previously omitted links, so the same committed and working-tree inputs disagreed.

## Decision

Carry `FileMode` on every rendered artifact and through materialisation and snapshot comparison.
Preserve regular (`100644`), executable (`100755`) and symbolic-link (`120000`) entries in Git trees.
Treat link targets as opaque bytes, even when a link is named `.jinja`.
Path rendering, including suffix removal, follows the ordinary output naming rules.
Dirty traversal reads link targets without following them, including dangling and directory links.

Reject output prefix collisions before building trees or materialising files.
Reject existing symlinks at destination paths and their ancestors before cleanup or writes.
No output may be written through a rendered link.
Materialisation creates Unix links from target bytes and explicitly fails on unsupported platforms.
Git-only rendering and snapshots do not require filesystem link creation.
Backport rejects link changes and regular/link conversions because its text patch cannot carry link modes.

## Compatibility

The public Rust artifact replaces its `executable` field with `mode`, and `materialise` accepts modes instead of booleans.
This is a breaking library API change.
Render JSON retains the boolean `executable` and adds an octal string `mode`.
Snapshot format 1 gains `120000` as an accepted manifest mode;
older readers reject it rather than silently interpreting it as an ordinary file.
Snapshot `files/` payloads remain ordinary files containing the exact target bytes, never live links.
Existing regular and executable snapshots retain their format.
