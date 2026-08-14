# Template context

One context serves both the question engine and the renderer. A value resolved
while prompting is the same value the template sees.

## Shape

```
answers      →  at the top level, by name
computed     →  at the top level, by name
data         →  under `data`
template     →  under `template`
```

```jinja
{{ project_name }}          {# an answer #}
{{ package_name }}          {# a computed value #}
{{ data.licenses.mit.spdx }} {# loaded data #}
{{ template.name }}          {# template metadata #}
```

### Why answers are at the top level

Because `{{ answers.project_name }}` is noise, and templates are read far more
often than they are written. The cost is that answers and computed values share
one namespace — which is why a collision between them is
[an error](computed.md#names-must-not-collide) rather than a shadow.

`data` and `template` are namespaced because they are structured and are not
what a template reaches for most of the time.

## `template`

| Name | Type |
|---|---|
| `template.name` | string |
| `template.description` | string or undefined |

Deliberately minimal. The template's *revision* is not here: a template that
rendered its own commit SHA into a file would produce a different tree on every
template commit, which is exactly the non-determinism this design avoids. The
revision is recorded in the commit trailers instead, where it belongs.

## `data`

Whatever the data sources parsed to, as structured values — tables stay tables,
arrays stay arrays, numbers stay numbers. See [Data sources](../data/index.md).

## How it is built

```
template revision
        │
   ┌────┴────┐
   │         │
 local     remote          ← data sources
  data      data
   └────┬────┘
        ▼
 initial context
        │
  question engine          ← topologically ordered
        │
   ┌────┼────┐
   │    │    │
answers │  conditions
     computed
        │
        ▼
 resolved context
        │
   ┌────┴────┐
   ▼         ▼
prompting  MiniJinja
```

There is no second pass and no second context. The renderer receives exactly
what the last question saw.

## What is not in it

No environment variables. No current time. No Git user. No repository metadata.
No process state.

This is [not an omission](../concepts/determinism.md#no-runtime-context). A value
that varies by machine belongs in the answers, where it is recorded and shared —
not in the context, where it is invisible and different for everyone.

## Filters and functions

MiniJinja's [built-in filters](https://docs.rs/minijinja/latest/minijinja/filters/index.html)
are available: `lower`, `upper`, `replace`, `trim`, `join`, `default`, `length`,
`first`, `last`, `sort`, `map`, `select`, `tojson`, and the rest.

No custom functions are registered. Anything that would reach outside the
context — reading a file, making a request, running a command — is not, and will
not be, available to expressions. Fetching belongs to the
[data-source subsystem](../data/index.md), which owns it explicitly.
