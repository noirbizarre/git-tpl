# Questions

Questions collect the values a template needs. They are declared in
`template.toml` under `[questions.<name>]`.

```toml
[questions.project_name]
type = "string"
prompt = "Project name"
help = "Used for the package name and the README title"
```

| Key | Type | Meaning |
|---|---|---|
| `type` | string | `string`, `boolean`, `integer`, `choice`, `multi_choice`. |
| `prompt` | string | Shown to the user. Defaults to the question's name. |
| `help` | string | A line of explanation under the prompt. |
| `default` | any / expression | The pre-filled value. |
| `when` | expression | Ask only if this evaluates truthy. |
| `choices` | array | The offered choices, for `choice` and `multi_choice`. |
| `choices_from` | string | A reference to a list, instead of `choices`. |

## Types

=== "string"

    ```toml
    [questions.project_name]
    type = "string"
    prompt = "Project name"
    ```

=== "boolean"

    ```toml
    [questions.ci]
    type = "boolean"
    prompt = "Enable CI?"
    default = true
    ```

=== "integer"

    ```toml
    [questions.port]
    type = "integer"
    prompt = "Port"
    default = 8080
    ```

=== "choice"

    ```toml
    [questions.license]
    type = "choice"
    prompt = "License"
    choices = ["MIT", "Apache-2.0"]
    ```

    Choices can carry labels and come from a data source — see
    [Choices](#choices).

=== "multi_choice"

    ```toml
    [questions.features]
    type = "multi_choice"
    prompt = "Features"
    choices = ["serde", "async", "cli"]
    default = ["serde"]
    ```

    The answer is a list, so `{% if 'serde' in features %}` works in templates.

Types are enforced. Supplying `--answer port=nope` for an `integer` question is
an error naming the question, the value and the expected type — not a silent
coercion to a string.

## Conditional questions

`when` is an expression evaluated against everything resolved so far:

```toml
[questions.project_type]
type = "choice"
prompt = "Project type"
choices = ["library", "application"]

[questions.cli]
type = "boolean"
prompt = "Create a CLI?"
when = "{{ project_type == 'application' }}"
default = true
```

A question whose `when` is false is **not asked, and has no value**. It is absent
from the context, not null or false.

That distinction is deliberate. In a template:

```jinja
{% if cli is defined and cli %}
[[bin]]
name = "{{ package_name }}"
{% endif %}
```

`cli is defined` tells you the question was *relevant*; `cli` tells you the
answer. Collapsing the two would make "not applicable" indistinguishable from
"declined", and templates would render an empty `[[bin]]` section for libraries.

## Dynamic defaults

A default may be an expression:

```toml
[questions.package_name]
type = "string"
prompt = "Package name"
default = "{{ project_name | lower | replace(' ', '-') }}"
```

It is evaluated at prompt time, after everything it references has been resolved.
You see the computed value pre-filled and can accept or replace it.

## Choices

Two things are chosen independently: **how many** answers a question takes, and
**where** its choices come from.

|  | `choices` — written inline | `choices_from` — a reference |
|---|---|---|
| `type = "choice"` | one answer, fixed list | one answer, resolved list |
| `type = "multi_choice"` | several answers, fixed list | several answers, resolved list |

All four combinations work. There is no `multi_choice_from`, because
`type = "multi_choice"` with `choices_from` already *is* that:

```toml
[data.catalogue]
source = "data/features.toml"

[questions.features]
type = "multi_choice"
prompt = "Features"
choices_from = "data.catalogue.all"
default = ["serde"]
```

Use `choices` or `choices_from`, not both.

### Labels

A choice is a bare string, or a table:

```toml
[questions.license]
type = "choice"
prompt = "License"
choices = [
  "Unlicense",
  { value = "MIT", label = "MIT License", help = "Short and permissive" },
  { value = "Apache-2.0", label = "Apache License 2.0", help = "Patent grant" },
]
default = "MIT"
```

| Key | Meaning |
|---|---|
| `value` | What is recorded as the answer. Required, and must be a string. |
| `label` | What is shown. Defaults to the value. |
| `help` | Shown beside the label, for a choice that needs explaining. |

**Only the value is ever an answer.** It is what appears in
`.config/git.tpl.toml`, what `--answer license=MIT` takes, and what a template
sees in `{{ license }}`. A label is presentation and nothing else — so rewording
one changes no rendered file, produces no commit, and gives nobody a merge to
perform. Answering with the label is an error, not a second spelling.

A data source uses the same shape, so a list can move from the manifest into a
data file without being rewritten:

```toml
# data/features.toml
[[all]]
value = "serde"
label = "Serialisation"
help = "Derive Serialize and Deserialize"

[[all]]
value = "async"
label = "Async runtime"
```

Extra keys in a data file are ignored — a licence list carrying `url` and
`osi_approved` beside `value` is normal. In the manifest they are an error,
because there an unrecognised key is a typo.

### Filtering choices

Choices are filtered with [computed values](computed.md), which are evaluated
before the question that uses them. There is no per-choice `when`: `[computed]`
already does this, for both inline and referenced lists, and one mechanism is
easier to reason about than two.

A list that depends on an earlier answer:

```toml
[questions.kind]
type = "choice"
prompt = "Project kind"
choices = ["library", "application"]

[computed]
servers = "{{ ['nginx', 'caddy'] if kind == 'application' else [] }}"

[questions.server]
type = "choice"
prompt = "Web server"
choices_from = "servers"
```

Filtering a data source on an earlier answer:

```toml
[data.pythons]
source = "data/python.toml"

[computed]
usable = "{{ data.pythons.versions | selectattr('value', 'ge', minimum) | list }}"

[questions.max_python]
type = "choice"
choices_from = "usable"
```

Reshaping data that does not use `value` and `label`:

```toml
[computed]
licences = "{{ data.licences.all | map(attribute='id') | list }}"

[questions.license]
type = "choice"
choices_from = "licences"
```

Keeping expressions in `template.toml` rather than in data files is deliberate.
A data file supplies values; it never supplies logic. That matters most for
[remote data](../data/remote.md), which is not pinned by the template revision.

**A filter that leaves nothing skips the question.** An empty list means "this
does not apply", so the question is not asked and has no value — exactly as a
false `when` leaves it, and `server is defined` still tells the two apart. A
literal `choices = []` is a different thing: it can never be answered, so it is
rejected when the manifest loads.

### When a choice is withdrawn

Narrowing a filter can leave a project holding an answer the template no longer
offers. That is reported, not silently discarded:

```
`ap` is not a valid choice for `region`

  help: choose one of: eu, us
        if this answer was recorded by an earlier render, the template no
        longer offers it — edit `region` in `.config/git.tpl.toml`
```

Dropping it quietly would change the rendered tree and commit without anyone
having asked for it.

### Referencing a list

`choices_from` is a dotted path into the context — a data source, a computed
value or an earlier answer — and must resolve to an array. If it resolves to a
map or a string, the error says so and names the path:

```
Failed to evaluate question `license`.

  Template:    rawtools/rust-library
  Reference:   data.licenses.ids
  Reason:      expected an array of choices, got a table
```

## Evaluation order

Question order is not the order you wrote them in. It is derived.

git-tpl parses every expression in the manifest — `when`, `default`,
`choices_from`, `[computed]` entries, and data source paths — extracts the names
each one references, and builds a dependency graph. That graph is topologically
sorted, and questions are asked in the resulting order.

The consequence: **you never have to think about ordering.** Declare
`package_name` before `project_name` if it reads better; git-tpl still asks
`project_name` first, because `package_name`'s default depends on it.

Within that constraint, ties are broken by declaration order, so the sequence is
stable across runs.

### Errors caught before any prompt

The graph is validated up front, so these fail before you are asked anything:

**Cycles.**

```
Cyclic dependency in template `rust-library`.

  answers.a → answers.b → answers.a

  A question's `when`, `default` or `choices_from` may only reference
  values resolved before it.
```

**Unknown references.**

```
Unknown reference in template `rust-library`.

  Question:    package_name
  Expression:  {{ projct_name | lower }}
  Unknown:     projct_name

  Did you mean `project_name`?
```

Both are load-time errors. Answering six questions and *then* being told the
seventh is unresolvable would be the worst possible time to find out.

## Supplying answers non-interactively

```sh
git tpl init ../template --answer project_name=demo --answer license=MIT
```

A supplied answer skips its prompt but still participates in the graph, so
anything depending on it resolves normally. Values are parsed according to the
question's declared type.

`--defaults` accepts every default without prompting; a question with no default
and no supplied answer is then an error.

On `update`, answers come from `.config/git.tpl.toml`. A question added to the
template since the last render has no recorded answer, and is prompted for —
or, with `--defaults`, takes its default. Either way the answer is written back
to `.config/git.tpl.toml`.
