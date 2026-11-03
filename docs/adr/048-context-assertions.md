# 048 — Template tests assert typed context, including null

Status: accepted.
Extends [ADR-016](016-template-tests-are-data.md) and [ADR-047](047-demand-driven-data.md).

## Context

File expectations and snapshots cannot detect a wrongly applicable question when output references are guarded.
Context resolution is observable template behavior and needs native tests using the case's answers.
Normalizing null to empty text also prevents assertions from distinguishing present null from an empty string.

## Decision

Add `expect.context` with `present`, `absent`, `equals` and `expressions`.
Presence means membership, not truthiness; equality compares literal typed values recursively.
Paths follow existing dotted table lookup, while expressions provide indexing and richer checks.
Boolean expressions use whole `{{ ... }}` syntax and pass only on boolean true.
Mismatches accumulate through existing human and JSON test reports, with absence explicitly distinguished from null.

Assertions add selective demand roots to the same scoped session that renders the case.
An absence check must resolve a declared computed/data root before inspecting it.
Unrelated declarations remain lazy, and expression short-circuiting retains the normal acquisition semantics.
Operational evaluation errors retain existing failure classification.
Assertions need no generated files or commands and run under `--skip-commands`.
Snapshot recording neither checks assertions nor evaluates their dependencies.
Migration cases inspect the destination update context, including replayed answers and overrides;
file expectations continue to inspect the merged project.
Composed child contexts remain separate.

Introduce first-class `Value::Null` throughout JSON/YAML, MiniJinja and context/report serialization.
Null is defined and falsy, with its own canonical encoding; existing non-null encodings are unchanged.
Undefined expression results keep their existing lenient empty-string conversion and do not become null.
Question types remain non-nullable, so typed null answers/defaults fail normal validation.
TOML has no null literal: use JSON/YAML equality or a boolean defined-and-none expression in TOML tests.
Persisting null-containing answers to project TOML fails with an actionable diagnostic rather than losing data.

## Consequences

Context tests detect changes that leave rendered output unchanged, without duplicating evaluator semantics.
The new expectations are additive, but preserving null is a breaking semantic and public Rust enum change.
Templates relying on null-to-empty-string normalization must explicitly choose their textual fallback.
Typed null values now survive nested arrays and objects, and reports preserve expected and actual types.
