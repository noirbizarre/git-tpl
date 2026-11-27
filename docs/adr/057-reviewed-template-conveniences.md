# ADR-057: Selected template conveniences are admitted by review

**Status:** accepted

**Supersedes:** [ADR-003](003-minijinja-only.md)'s requirement that an added filter cannot reasonably be
expressed with built-ins.
Its engine, isolation and no-plugin decisions remain in force.

## Context

Templates frequently need to spell a boolean-like value as `yes` or `no`.
`{{ 'yes' if value else 'no' }}` already works, but `{{ value | yesno }}` states the formatting intent
directly and composes naturally with other filters.
Rejecting all syntactic sugar rules out useful, small conveniences solely because a longer expression exists.

Every extra filter or global also becomes a compatibility surface that templates may depend on indefinitely.
Accepting one convenience should not commit the project to an ever-growing catalogue.

## Decision

Selected syntactic sugar may be added to git-tpl's deliberately small set of filters and globals.
Contributions are reviewed case by case and are not guaranteed acceptance, even when they satisfy the
technical constraints.
Review weighs readability and usefulness against the lasting compatibility cost.

Filters remain pure, deterministic functions of their arguments.
Render-time globals must likewise stay within the supplied context and remain deterministic.
Neither may read the environment or filesystem, access the network, or execute commands.
Templates cannot register filters or globals, and there is no plugin interface.
This decision introduces no globals and does not widen the render context.

The first convenience is [`yesno`](../templates/context.md#yesno).
It returns `yes` or `no` using MiniJinja truthiness, without parsing strings as booleans.
An undefined value received by the filter is false even under strict rendering, so authors can deliberately
format an optional value without first adding a fallback.

## Consequences

The ability to express a candidate with built-ins is no longer an automatic reason to reject it.
The extra surface remains limited by review rather than a promise to accept every useful helper.
A template using `yesno` requires a git-tpl version that provides it.
