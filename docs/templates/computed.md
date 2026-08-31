# Computed values

A computed value is derived from answers, data and other computed values.
It is declared once and available everywhere, so a template does not repeat the same expression in twelve files.

```toml
[computed]
package_name = "{{ project_name | lower | replace(' ', '-') }}"
module_name = "{{ package_name | replace('-', '_') }}"
year_range = "{{ start_year }}–{{ end_year }}"
line_length = 100
```

A value is an expression or a plain literal — see [Values, not strings](#values-not-strings).

Computed values sit in the same namespace as answers, so a template reads them the same way:

```jinja
[package]
name = "{{ package_name }}"
```

They may depend on:

- answers
- other computed values
- loaded data
- template metadata

## Ordering

You do not declare an order.
It is derived from the expressions, the same way question order is — see
[Questions § Evaluation order](questions.md#evaluation-order).

So this is fine, even though `module_name` is declared first:

```toml
[computed]
module_name = "{{ package_name | replace('-', '_') }}"
package_name = "{{ project_name | lower }}"
```

And this is an error, reported before any prompt appears:

```toml
[computed]
a = "{{ b }}"
b = "{{ a }}"
```

```
Cyclic dependency in template `rust-library`.

  computed.a → computed.b → computed.a
```

### With `[extends]`

A child extending a parent (see [`[extends]`](format.md#extends)) inherits its parent's `[computed]` entries by
name, exactly like `[questions]`: the parent's own entries come first, an entry the child redeclares replaces the
parent's entirely and keeps the parent's position, and a name new to the child is appended after them.

## Available while prompting

A computed value is resolved as soon as its dependencies are, which means a *later* question can use it:

```toml
[questions.project_name]
type = "string"
prompt = "Project name"

[computed]
package_name = "{{ project_name | lower | replace(' ', '-') }}"

[questions.crate_path]
type = "string"
prompt = "Crate directory"
default = "crates/{{ package_name }}"
```

The sequence is: ask `project_name` → compute `package_name` → ask `crate_path` with `crates/<computed>`
pre-filled.

## Values, not strings

A computed value is an *expression* or a *literal*.
The rule is the one a question's [`default`](questions.md#dynamic-defaults) already follows: a string containing
`{{` or `{%` is an expression; **anything else is a literal, kept exactly as written**.

So a shared constant is written as itself:

```toml
[computed]
line_length = 100          # an integer
strict = true              # a boolean
editors = ["vim", "helix"] # a list
```

and reaches the template as that type, not as text:

```jinja
line-length = {{ line_length }}
indent = {{ line_length // 4 }}
```

An expression that produces a single value keeps *its* value's type too:

```toml
[computed]
# a boolean, not the string "true"
needs_tokio = "{{ cli and project_type == 'application' }}"

# a list, not "['a', 'b']"
all_features = "{{ data.features.base + extra_features }}"
```

This matters when the result is used in `{% if %}`, iterated, or serialised.
An expression that interpolates into surrounding text is a string, as you would expect:

```toml
[computed]
title = "{{ project_name }} — a Rust library"
```

## Names must not collide

A computed value may not have the same name as a question.
They share a namespace, so one would silently shadow the other and which one won would depend on evaluation
order.

```
Name collision in template `rust-library`.

  `package_name` is declared both as a question and as a computed value.

  Answers and computed values share one namespace, so a template cannot
  tell them apart. Rename one.
```

## Computed values are not recorded

Only *answers* are written to `.config/git.tpl.toml`.
Computed values are recomputed on every render, by design: they are a function of the answers and the template,
and a template that changes how `package_name` is derived should change it for existing projects too.

## Filtering choices

A computed value that resolves to a list can be pointed at by `choices_from`, which is how a question's choices
are filtered:

```toml
[questions.kind]
type = "choice"
choices = ["library", "application"]

[computed]
servers = "{{ ['nginx', 'caddy'] if kind == 'application' else [] }}"

[questions.server]
type = "choice"
choices_from = "servers"
```

The graph guarantees `kind` is answered before `servers` is computed, and `servers` before `server` is asked.
If the list comes out empty the question is skipped entirely.
See [Choices](questions.md#choices).
