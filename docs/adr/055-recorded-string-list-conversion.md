# ADR-055: Migrate recorded comma-separated list answers

## Status

Accepted.
Partially supersedes [ADR-041](041-typed-supplied-answers.md)'s prohibition on recorded-string conversion.

## Context

Templates have used string questions for comma-separated lists.
Replacing those questions with formal `list` or `multi_choice` questions currently rejects existing project answers.
The stored string and current question schema provide enough information for a narrow compatibility migration.
Explicit answer files still need strict types so a misspelled or incorrectly quoted value is not silently accepted.

## Decision

Preserve saved-answer origin as `SuppliedAnswer::Recorded(Value)` when merging project answers with overrides.
During update and shared preview evaluation, convert recorded strings for `list` and `multi_choice` questions only.
Split on literal commas, trim surrounding whitespace, and discard empty segments.
Preserve order and duplicates; empty or comma-only strings become empty arrays.
Do not interpret CSV quoting or JSON syntax, inspect historical schemas, or convert other scalar types.

Normal validation applies after conversion, including item constraints, choice membership and count bounds.
Override precedence remains unchanged, and file overrides retain their strict typed origin.
CLI list input remains JSON; CLI multi-choice input remains comma-separated.
Defaults, expressions, composed forwarding and explicit answer files receive no new conversion.

Existing update persistence saves successfully resolved arrays as unstaged configuration edits,
including when rendering is unchanged.
Dry runs and dirty previews perform the same conversion without saving configuration.
Switch candidates keep their compatibility-and-fallback semantics;
historical backport replay and status's best-effort composition probe retain their existing typed inputs.

## Consequences

Existing projects can follow templates from string lists to formal arrays without manually editing saved answers.
Invalid converted values still fail before publication or configuration writeback.
No stored format or serialization changes are needed; resolved answers remain typed `Value` maps.
The additional public Rust enum variant requires downstream exhaustive matches to handle recorded origin.
ADR-041's strict typing contract otherwise remains in force.
