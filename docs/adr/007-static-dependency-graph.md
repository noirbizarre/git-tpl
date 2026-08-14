# ADR-007: Question order is derived from a static dependency graph

**Status:** accepted

## Context

Questions depend on each other. `when = "{{ project_type == 'application' }}"`
requires `project_type`. `default = "{{ project_name | lower }}"` requires
`project_name`. Computed values and dynamic data source paths add more edges.

Two ways to handle it:

1. **Declaration order.** Ask in the order written; evaluate each expression when
   reached; error if it references something not yet resolved. Simple. Ordering
   becomes the template author's problem, and a cycle surfaces as a confusing
   "missing value" error partway through a questionnaire.

2. **Static graph.** Extract the referenced names from every expression up front,
   build a DAG, topologically sort, validate.

Option 2 sounded expensive until we found that MiniJinja exposes
`Template::undeclared_variables(nested: true)` as stable public API, returning
the dotted paths an expression reads. The extraction is a few lines.

## Decision

Before anything is prompted, parse every expression in the manifest — `when`,
`default`, `choices_from`, `[computed]`, and data source paths — extract the
names each references, and build a dependency graph over nodes `answers.*`,
`computed.*` and `data.*`.

Topologically sort it. Ties break by declaration order, so the sequence is stable.
Validate it: cycles and unknown references are errors before the first prompt.

## Consequences

Template authors never think about ordering. Declaring `package_name` above
`project_name` still asks `project_name` first, because the graph says so.

Cycles are detected and reported as cycles, with the path — not as a mysterious
undefined value.

An unknown reference is caught immediately, with a suggestion. Answering six
questions and *then* being told the seventh is unresolvable is the worst possible
moment to find out.

Data sources are loaded lazily in dependency order, and cached, so several
questions sharing a source cause one load, and a source nothing references is
never loaded at all.

Prompting is genuinely incremental: a `when` is evaluated against everything
resolved so far, so a question that does not apply is never shown — as opposed to
asking everything and filtering afterwards.

The cost is one more module and the requirement that dependencies be statically
visible. That is a constraint we want anyway: an expression whose dependencies
cannot be determined without running it is an expression we do not want.
