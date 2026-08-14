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
| `choices` | array | For `choice` and `multi_choice`. |
| `choices_from` | string | A data reference, instead of `choices`. |

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

## Dynamic choices

`choices_from` points at a data source rather than listing choices inline:

```toml
[data.licenses]
source = "data/licenses.toml"

[questions.license]
type = "choice"
prompt = "License"
choices_from = "data.licenses.ids"
```

The reference is a dotted path into the context and must resolve to an array of
scalars. If it resolves to a map or a string, the error says so, names the
question, and shows the path:

```
Failed to evaluate question `license`.

  Template:    rawtools/rust-library
  Reference:   data.licenses.ids
  Reason:      expected an array of choices, got a table
```

Use `choices` or `choices_from`, not both.

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
