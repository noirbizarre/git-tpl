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

The one place a machine fact is legible to a template is
[`default_from`](questions.md#machine-seeded-defaults), which reads its own small
[seed context](../adr/018-seed-context.md) — the Git configuration, the
directory name, the remote. That context is not this one, is reachable from
nowhere else, and only ever pre-fills a prompt.

## Imported names

A `{% import %}` brings a [shared partial](format.md#shared-partials)'s macros
into scope for the file that imports it, and for that file only. Nothing is
implicitly available: a macro must be imported where it is used.

```jinja
{% import "macros.jinja" as m %}
{{ m.badge(project_name) }}
```

A partial is any `.jinja` file outside the rendered subdirectory, named by its
path from the repository root. It is read from the same pinned template revision
as everything else — the loader cannot name a path on your machine.

The same imports work in manifest expressions, so a `computed` value can use the
macros a file uses.

## Filters and functions

MiniJinja's [built-in filters](https://docs.rs/minijinja/latest/minijinja/filters/index.html)
are available: `lower`, `upper`, `replace`, `trim`, `join`, `default`, `length`,
`first`, `last`, `sort`, `map`, `select`, `tojson`, and the rest.

No custom functions are registered. Anything that would reach outside the
context — reading a file, making a request, running a command — is not, and will
not be, available to expressions. Fetching belongs to the
[data-source subsystem](../data/index.md), which owns it explicitly.

### `slugify`

One filter is added to the built-ins, because it is the one a real template
cannot write for itself:

```jinja
{{ project_name | slugify }}
```

It transliterates to ASCII, lowercases, and joins the remaining alphanumeric
runs with a single `-`:

| Input | Result |
|---|---|
| `My Project` | `my-project` |
| `Hello, World!` | `hello-world` |
| `Café Déjà-Vu` | `cafe-deja-vu` |
| `Größe` | `grosse` |
| `Москва` | `moskva` |
| `北京` | `bei-jing` |

`lower | replace(' ', '-')` is the usual substitute and it is wrong twice: it
leaves accents and punctuation in the result, and it produces something that is
not a valid module name, package name or path segment for anyone whose project
is not named in ASCII.

Input that slugs to nothing — `!!!`, or an empty string — yields an empty
string rather than an error. A filter that raised inside a `when` condition
would abort the whole render; an empty value is visible immediately.

The filter is available everywhere any other expression is: in a `default`, in a
`when`, in a computed value, in a file body, in a templated path, and in a
[`default_from`](questions.md#machine-seeded-defaults) seed expression — which is
where `{{ remote.name | slugify }}` earns its keep.

**The set of filters is closed.** There is no plugin point, and there will not
be one — see [ADR-003](../adr/003-minijinja-only.md). A filter is only
considered if it is pure, deterministic, and reaches nothing outside its own
argument, and every one added is a compatibility surface that cannot later be
removed.
