# ADR-003: MiniJinja is the only template engine

**Status:** accepted

## Context

Template tools tend to accumulate engines: Jinja for Python users, Handlebars for
JavaScript users, Tera because it is Rust-native. Each is a plugin point, a
configuration key, and a set of subtly different semantics.

## Decision

[MiniJinja](https://docs.rs/minijinja) renders everything, and there is no
extension point for a second engine.

Further, MiniJinja is not only the file renderer. It is the expression engine for
*all* dynamic behaviour:

- `when` conditions on questions
- dynamic defaults
- `choices_from` references
- computed values
- dynamic data source paths
- conditional rendering and templated paths

## Consequences

One syntax to learn. A user who can write `{{ project_name | lower }}` in a file
can write it in a `default`, and it means the same thing.

One implementation of the expression semantics, so a value cannot evaluate
differently in a condition than in a file.

Static analysis becomes possible. `Template::undeclared_variables` gives us the
names an expression references, which is what makes the dependency graph in
ADR-007 buildable at all. A pluggable engine could not offer that.

Jinja2 is the most widely known template syntax in this problem space, so the
learning cost is low for anyone arriving from Copier or Cookiecutter — without
any attempt at compatibility, which is explicitly not a goal.

No Python is embedded, and no arbitrary code executes. MiniJinja evaluates
expressions over a context we construct; it cannot reach outside it.

## Filters

The corollary, recorded here because it is the question this decision is
actually asked in practice: *"I need a filter — where is the extension point?"*

**The built-ins are the answer, and there is no extension point.** A template
cannot register a filter, and no configuration key will ever let it. A filter is
code, and code from a template is the one thing this project does not run.

git-tpl itself registers exactly one filter beyond MiniJinja's own —
[`slugify`](../templates/context.md#slugify) — because `project_name →
project_slug` appears in every project template and `lower | replace(' ', '-')`
is wrong for unicode and for punctuation.

**The set is closed by review.** A candidate qualifies only if it is pure,
deterministic, and reaches nothing outside its own argument; and even then, the
bar is that templates cannot reasonably express it with the built-ins. The
pressure will be to ship a *useful set*. It should be resisted: every filter
added is a compatibility surface that cannot be removed, and a template that
depends on one cannot render with an older git-tpl.

## Partials

A loader is *not* a filter, and [ADR-012](012-template-loader.md) admits one.
The distinction is that a loader resolves a name to bytes already committed in
the template repository and hands them to the same parser that reads every other
`.jinja` file. It executes nothing, and it reaches nothing a template could not
already achieve by pasting the text into each file. The closed filter set above
is untouched.
