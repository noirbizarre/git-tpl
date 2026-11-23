# ADR-053: One-shot files retain initialized entries in rendered history

**Status:** Accepted

## Context

Placeholder assets are meant to be replaced by a project's own files.
Regenerating the placeholder creates needless template-side changes and merge conflicts.
Omitting it from later renderings instead creates a deletion, which conflicts with the user's replacement.
The rendered ref must keep the initial entry unchanged so ordinary three-way merge sees no template-side change.

## Decision

Add `[files]`, keyed by final rendered output paths, with declarations such as `{ once = true }`.
Keys may contain expressions evaluated in the owning template's context.
They are relative to output rather than the source root, with no suffix stripping or path normalization.
Reject unsafe paths and evaluated key collisions.
An unmatched declaration waits until its output first appears; an expression evaluating to empty is conditional.

The first successful capture records both bytes and Git mode.
Keep captured entries in every subsequent rendered tree, including after source, declaration or module removal.
Neither `once = false` nor removing a declaration thaws historical capture.
Inheritance merges declarations by key, nearest declaration winning before capture.
Composed children evaluate keys in their own namespaces and contribute capture records to the parent's history.

Record the cumulative sorted set of captured paths in repeated `Template-Once` trailers containing JSON strings.
The corresponding entries remain in the tree, keeping their blobs reachable through ordinary Git history.
This extends ADR-008 without introducing output metadata files or project configuration state.
An unchanged tree still requires a commit when the captured path set changes.

The previous rendered tree and captured path set become explicit deterministic inputs to history-aware rendering.
They are never exposed as template context, and retained contents never come from the user's worktree.
Do not evaluate the current body at a captured path, but continue validating current source path collisions.
Restore absent captured entries before final tree validation.
An absent source root is allowed when historical capture still supplies output.

Use the same baseline for update, previews, forced initialization, and source switching on the same ref.
Project-free renders have no baseline and render current sources normally.
Migration moves touching captured paths are rejected, including directory moves involving their ancestors.
Backport refuses modifications to retained output because it no longer has a reproducible current-source mapping.

## Consequences

Git keeps a user's committed replacement or deletion without a template-side change to the initialized path.
Ordinary files continue updating and disappearing as before.
Removing a composed module can leave its one-shot paths in the rendered baseline deliberately.
Moving a dynamic path creates a different capture identity and preserves the old initialized path as well.

Capture state travels with rendered history, including pushed and fetched refs.
Restore a missing rendered ref before updating if its initialized state must survive.
Commits created by older versions, without these trailers, carry no capture records.
Identical complete inputs, including the retained baseline, still produce identical trees on every platform.
