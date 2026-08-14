# Computed values

A computed value is derived from answers, data and other computed values. It is
declared once and available everywhere, so a template does not repeat the same
expression in twelve files.

```toml
[computed]
package_name = "{{ project_name | lower | replace(' ', '-') }}"
module_name = "{{ package_name | replace('-', '_') }}"
year_range = "{{ start_year }}–{{ end_year }}"
```

Computed values sit in the same namespace as answers, so a template reads them
the same way:

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

You do not declare an order. It is derived from the expressions, the same way
question order is — see
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

## Available while prompting

A computed value is resolved as soon as its dependencies are, which means a
*later* question can use it:

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

The sequence is: ask `project_name` → compute `package_name` → ask `crate_path`
with `crates/<computed>` pre-filled.

## Values, not strings

An expression that produces a single value keeps that value's type:

```toml
[computed]
# a boolean, not the string "true"
needs_tokio = "{{ cli and project_type == 'application' }}"

# a list, not "['a', 'b']"
all_features = "{{ data.features.base + extra_features }}"
```

This matters when the result is used in `{% if %}`, iterated, or serialised. An
expression that interpolates into surrounding text is a string, as you would
expect:

```toml
[computed]
title = "{{ project_name }} — a Rust library"
```

## Names must not collide

A computed value may not have the same name as a question. They share a
namespace, so one would silently shadow the other and which one won would depend
on evaluation order.

```
Name collision in template `rust-library`.

  `package_name` is declared both as a question and as a computed value.

  Answers and computed values share one namespace, so a template cannot
  tell them apart. Rename one.
```

## Computed values are not recorded

Only *answers* are written to `.config/git.tpl.toml`. Computed values are
recomputed on every render, by design: they are a function of the answers and the
template, and a template that changes how `package_name` is derived should change
it for existing projects too.
