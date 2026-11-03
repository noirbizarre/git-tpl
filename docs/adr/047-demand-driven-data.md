# 047 — Data acquisition follows actual expression access

Status: accepted.
Extends [ADR-007](007-static-dependency-graph.md) and [ADR-045](045-source-object-cache.md).

## Context

The static dependency graph describes every possible manifest dependency.
Executing all its nodes eagerly fetched unused data, even behind false question gates or short-circuited expressions.
This contradicted the documented lazy-loading and offline contracts.
Static reachability alone cannot distinguish which branch MiniJinja actually executes.

## Decision

Retain upfront graph validation and its deterministic question order.
Resolve data and computed declarations only when an executed expression accesses them.
Skipped questions do not evaluate their choices or ordinary defaults;
an explicit `default_when_skipped` still requests its default.
Supplied answers do not evaluate defaults they replace, but still validate against applicable choices.

A scoped evaluation session owns the materialized context and borrows the existing data loader.
MiniJinja context objects hold owned snapshots and a synchronized first-demand slot, never repository handles.
An unresolved lookup records its declaration and returns a temporary undefined value.
After the pure evaluation attempt, the session resolves that first demand and retries.
The attempt's result or error is discarded whenever a demand was recorded,
including demands raised during typed value conversion.
Only the first unresolved lookup counts: subsequent execution may have followed a speculative branch.
Resolved declarations are memoized, and an active-node set detects hidden runtime cycles.

The retry boundary is one expression, path segment or file body.
Prompts, source acquisition, child rendering, ref publication and worktree edits are never replayed.
Paths still short-circuit before later segments and bodies, and binary and symlink bodies remain opaque.
The renderer receives an evaluator, keeping acquisition in the evaluation/data layers.

Operation-specific metadata is prepared while the session is alive and before publication:
init requests notes and remote URLs, update requests newly crossed migration messages,
and composition prepares its parent source expressions before acquiring children.
`context --eval` adds its expression as a demand root.
A context dump explicitly requests all computed values, then serializes materialized data without fetching other sources.
Final contexts are owned snapshots with no acquisition capability.

Network consent remains batched over declarations before evaluation.
The loader enforces those decisions when a source is actually requested.
Unused declined sources do not fail; requested declined sources keep the existing actionable diagnostics.
Byte caching, per-declaration validation and Git resolution freezing remain loader responsibilities.
Provenance records successfully accessed declarations in deterministic demand order.

## Consequences

Untaken conditions and unused declarations perform no acquisition and cannot cause offline or missing-file errors.
Dependency cycles and unknown manifest references remain statically rejected, including in unused declarations.
This does not broaden the existing contract for dynamically unknowable manifest dependency names.
Pure MiniJinja evaluation can repeat when a new dependency is discovered,
but each declaration is loaded at most once and no external effects occur during speculative evaluation.
