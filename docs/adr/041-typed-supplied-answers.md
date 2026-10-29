# ADR-041: Preserve supplied-answer origins

## Status

Accepted.

## Context

Structured answers already carry types, but treating every string as CLI text silently accepted wrong-type files
and recorded configuration.
The same spelling must not make a typed string into an integer, boolean or list.

## Decision

Public evaluation and operation inputs use `SuppliedAnswer::Typed(Value)` or `SuppliedAnswer::Cli(String)`.
Only explicit CLI text receives question-type parsing.
Files, configuration, composed forwarding and test cases supply typed values;
choice, pattern and list validation then applies equally to both origins.
Precedence remains CLI, later files, earlier files, recorded answers, defaults.

## Consequences

This is an acceptance and Rust API breaking change.
Callers wrap existing typed maps with `answers::typed` and use `Cli` only for actual CLI text.
`Config.answers`, case answers, resolved context, digests and TOML serialization remain typed `Value` maps.
Existing wrong-type recorded strings fail with `tpl::eval::wrong_type`;
users must explicitly correct their configuration or override the answer, rather than receiving a silent migration.
