# ADR-056: UTC datetime values may seed an interactive prompt

**Status:** accepted

**Supersedes:** [ADR-018](018-seed-context.md)'s exclusion of `now` from the seed context

**Relates to:** [ADR-006](006-no-runtime-context.md)

## Context

A copyright year is an ordinary project answer, but manually typing the current year adds no useful judgment.
ADR-018 already allows machine-derived suggestions that reach rendered content only after human confirmation.
A clock-derived suggestion can follow the same boundary without making later renders depend on the clock.

## Decision

Add `now` to the closed seed context, available only to `default_from` expressions:

```toml
[questions.copyright_year]
type = "string"
default_from = "{{ now.year }}"
```

`now.year`, `month`, `day`, `hour`, `minute`, and `second` are integers.
`now.date`, `time`, and `datetime` are ISO strings, respectively `2026-11-27`, `14:05:09Z`,
and `2026-11-27T14:05:09Z`.
UTC avoids a dependency on host timezone configuration; strings use whole-second precision.
Capture one instant when the lazy seed context is first needed and reuse it for all subsequent prompts in that render.
This prevents questions on either side of midnight from receiving inconsistent date components.

All other ADR-018 boundaries remain:

- Seeds require an interactive run with a project repository.
- Supplied or reused answers and compatible personal defaults take precedence without acquiring a seed.
- `--defaults` does not acquire seeds; an ordinary `default` applies, or an unanswered question fails.
- The seed context is a separate type, its environment has no partials, and only string questions accept seeds.
- A suggestion becomes an ordinary recorded answer only after human confirmation.
- No clock value is injected into rendering, computed values, conditions, or ordinary defaults.

The namespace set is now `git`, `dir`, `remote`, and `now`; other additions still require an ADR.

## Consequences

A copyright year can be suggested once and retained indefinitely.
It does not automatically advance on updates: changing a recorded year remains an explicit answer change.
The same template, recorded answers, data, and retained one-shot baseline still render the same tree.

This is an additive manifest capability, with no persisted configuration or CLI format changes.
Older versions reject the new seed namespace during manifest validation.
Templates that need non-interactive use must provide stable ordinary defaults or supply answers explicitly.
Standalone rendering without a project repository retains its existing seed behavior.
