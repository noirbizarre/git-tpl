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
