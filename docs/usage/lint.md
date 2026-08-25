# `git tpl lint`

Check a template without rendering it. No project, no network.

```sh
git tpl lint .            # the template in this directory
git tpl lint . --dirty    # including uncommitted changes
```

A render only ever proves the branch it took.
The failures that hurt are the ones a given answer set never reaches — and the worst of them are silent.

Exit code is 1 when there is an error, 0 when there are only warnings.
A lint that fails on things a template may legitimately mean is a lint people stop running.
When a particular template *has* decided it never means one of them, [`--deny`](#choosing-what-fails) says so.

## What it checks

### The manifest and its graph

Everything `init` would check before the first prompt: unknown references, cycles, incoherent question
declarations.
The difference is that this needs no repository and no network.

### Every `.jinja` file parses

Including branches no answer set reaches.
Otherwise a syntax error in a rarely taken conditional is found by the first person who answers their way into
it.

### Conditional path segments — `tpl::lint::degenerate_path`

The one that motivates the command:

```
.github/workflows/{% if msrv %}msrv{% endif %}.yaml
```

The `.jinja` suffix is stripped from the whole path *before* the segments are rendered.
For a file that is not a template, the `.yaml` therefore sits outside the block — so with `msrv` false the
segment renders to `.yaml`, which is non-empty, is not `.` or `..`, and contains no separator.
Every check the renderer makes passes, and it writes a file called `.yaml`.

Two such files collide, and `tpl::render::collision` names them both. **One is silent.**

The fix is to put the whole name inside the block:

```
.github/workflows/{% if msrv %}msrv.yaml{% endif %}
```

For a `.jinja` file the outer form is correct, because the suffix is stripped first —
`{% if docs %}zensical.toml{% endif %}.jinja` collapses to nothing, as intended.
The check knows the difference.

A directory segment's `{% else %}.{% endif %}` — the documented way to make that segment
[transparent](../templates/format.md#paths-are-templates-too) rather than skipped — is not flagged either, for
the same reason: the whole thing, `.` included, sits inside the block.

### Foreign expressions — `tpl::lint::foreign_expression`

`${{ github.ref }}` contains `{{`, so MiniJinja consumes it: the result is `$`, the YAML is still valid, and
nothing fails until the workflow runs.

Three ways out, and the lint names all three:

- wrap the region in `{% raw %}…{% endraw %}`;
- drop the `.jinja` suffix, so the file is copied byte-for-byte;
- escape it as `${{ '{{' }} github.ref {{ '}}' }}`.

The escape idiom is not flagged.
A workflow that interpolates anything has to write it on every line.

### Undeclared names — `tpl::lint::undeclared`

A name a file body uses that the template never declares.
MiniJinja is lenient, so `{{ projct_name }}` renders to an empty string and the command succeeds, leaving
`name = ""` in a `Cargo.toml` that parses.

A warning, because that is still the default.
Set `strict = true` in `template.toml` to make it an error at render time.

Names that came from a `${{ … }}` are not reported: `matrix` belongs to GitHub Actions, and advising an author to
declare it would be advice not to take.

### Unguarded gate reads — `tpl::lint::unguarded_gate`

A [`when`-gated question](../templates/questions.md#conditional-questions) is *declared*, so `undeclared` has
nothing to say about it — but for every answer set where its `when` is false it has no value at all, absent from
the context rather than null or false.
Reading it bare in a file that is not itself gated by the same condition renders fine for every answer set that
turns the condition on, and fails for the one that turns it off:

```toml
[questions.docs]
type = "boolean"
default = true

[questions.docs_accent]
type = "string"
when = "{{ docs }}"
default = "blue"
```

```jinja
accent: {{ docs_accent }}
```

`accent: blue` today, whatever `docs` is — until `strict = true` is set, or until `strict` becomes the default.
Then it fails only when `docs` is false, and nothing warned beforehand, because `docs_accent` is a name the
manifest does declare.

The fix is the same idiom `undeclared` already recommends for an optional name:

```jinja
{% if docs_accent is defined %}accent: {{ docs_accent }}{% endif %}
accent: {{ docs_accent | default('blue') }}
```

A warning, for the same reason as `undeclared`: the renderer is still lenient by default.
It is a whole-file text search for the guard idiom, not control-flow analysis — a guard anywhere in the file
silences every read of the name in it, and one inside an imported macro is invisible, for the same reason
`undeclared` cannot see one.

A question declared with
[`default_when_skipped = true`](../templates/questions.md#keeping-a-default-when-skipped) is excluded: its
default fills in for a false `when`, so it is never actually absent, and a bare read of it is not the trap this
rule warns about.

### Absorbed keys — `tpl::lint::absorbed_key`

In TOML a bare key belongs to the table that most recently opened.
So a top-level manifest key written below a table header is that table's:

```toml
name = "probe"

[computed]
package = "{{ name | lower }}"
note_file = "NOTE.md"        # a computed value called `note_file`
```

The manifest is valid TOML, it loads without complaint, and the note never appears — `note_file` was never set.
Below `[remotes]` the same line adds a Git remote called `note_file` at `init`, pointing at `NOTE.md`.

Nothing else can catch this.
By the time the manifest is deserialised the key is gone, and a template that declared no note looks exactly the
same, so this rule reads the manifest source rather than the parsed manifest.

The fix is to move the key above the first table header.
A warning rather than an error, because a computed value or a remote genuinely named `name` is conceivable —
`--allow tpl::lint::absorbed_key` for a template that means it.

Keys whose value is a table are not reported: `[data.note_file]` is a data source that happens to be called
`note_file`, not an absorbed key.

### Shadowed names — `tpl::lint::shadowed_name`

An import alias binds in the same namespace as the answers and computed values.
If it collides with a question name, the alias wins, and every comparison against the name after that is
module-vs-string — never true, never an error.
`strict = true` does not help, because nothing is undefined:

```jinja
{%- import "macros/m.jinja" as stack -%}
{% if stack == "b" %}YES{% else %}NO{% endif %}
```

`stack` here is the imported module, not the `stack` question, and the `if` silently takes the wrong branch
forever.
Rename the alias.

`{% set %}`, `{% with %}`, `{% for %}` targets and `{% macro %}` names are checked the same way — anything a file
body binds can shadow a question just as an import alias can.
Two things are not flagged, because they are the common, intended use of a rebinding:

- a macro's own parameters, since shadowing inside the macro's body is what a parameter is for;
- a target whose right-hand side mentions its own name, the self-defaulting idiom —
  `{% set stack = stack | default('b') %}`.

A warning, the same as `tpl::lint::undeclared`: the binding is legal MiniJinja, so an author who means it can
`--allow` the code.
The manifest refuses the same collision between a question and a computed value outright — this is that
collision arriving from a third direction.

### Shadowed builtins — `tpl::lint::shadowed_builtin`

MiniJinja itself registers four global functions: `range`, `dict`, `namespace`, `debug`.
A question or computed value declared with one of those names collides with a name the manifest never wrote —
git-tpl did not put it there, MiniJinja did.

For a computed value, or a question with no `when`, this is harmless today: the manifest's own value is always
present in the rendered context, and always wins.
A `when`-gated question is not — whenever its own `when` is false it is *absent* from the context, not null, and
MiniJinja's lookup falls through the absent entry straight to its own builtin:

```toml
[questions.kind]
type = "string"
default = "python"

[questions.namespace]
type = "string"
when = "{{ kind == 'lib' }}"
default = ""
```

For every answer set where `kind != "lib"`, `namespace` is absent — but `{% if namespace is defined %}` reports
`true` anyway, and `{{ namespace }}` renders the builtin function's own representation instead of nothing.
The guard idiom `tpl::lint::unguarded_gate` recommends for reading a gated question does not help here, because
nothing is ever undefined to begin with.

The fix is to rename the question or computed value.
A warning rather than an error, and reported regardless of whether a `when` is present right now: the collision
is invisible either way, and adding a `when` to an already-declared name later would reopen the hole with no new
warning to catch it.

## Options

| Flag | Effect |
|---|---|
| `<template>` | The template — a Git URL or a path. Defaults to `.`. |
| `--ref` | Branch, tag or commit to check. Defaults to the remote's default branch. |
| `--root` | Check this subdirectory instead of the manifest's. |
| `--dirty` | Include the template's uncommitted changes. Local templates only. |
| `-D`, `--deny <CODE\|warnings>` | The finding fails the lint. Repeatable. |
| `-A`, `--allow <CODE\|warnings>` | The finding is not reported at all. Repeatable. |
| [`--json`](../reference/json.md#lint) | A global flag. The findings on stdout as one object. |

## Choosing what fails

The default severities are a judgement about templates in general.
A given template may have a firmer opinion — a workflow repository that never means a raw `${{ }}`, say.
Two repeatable flags, spelled as `cargo clippy` spells them:

| Flag | Effect |
|---|---|
| `-D`, `--deny <CODE\|warnings>` | The finding fails the lint |
| `-A`, `--allow <CODE\|warnings>` | The finding is not reported at all |

Both take either the word `warnings`, meaning the whole severity, or a single `tpl::lint::*`
[code](../reference/diagnostics.md#linting).

```sh
git tpl lint . -D warnings                        # any warning fails
git tpl lint . -D tpl::lint::foreign_expression   # only that one fails
git tpl lint . -A tpl::lint::undeclared           # stop reporting that one
```

A named code always overrides `warnings`, so an exception is a matter of naming it:

```sh
# Everything fatal, except the code this template is still migrating away from
git tpl lint . -D warnings -A tpl::lint::undeclared
```

Precedence is by specificity, not by position: writing the `-A` first means the same thing.
Unlike clippy, where the last flag wins, arguments here can be reordered by a shell fragment or a composed CI
config without changing what the build means.
Naming the same code in both flags is an error rather than a coin-toss, as is `-D warnings -A warnings`.

A misspelled code is an error too — `tpl::lint::unknown_code`, listing the valid ones.
Accepting it would deny nothing, and the symptom would be a green CI run.

Denying does not rewrite a severity.
A denied warning is still reported as a warning, marked `(denied)`, and [`--json`](../reference/json.md#lint)
keeps `"severity": "warning"` beside `"denied": true` — so a consumer can tell a rule the template broke from a
policy this run applied.

## In CI

```yaml
- run: git tpl lint . --dirty
```

For a repository where warnings are errors:

```yaml
- run: git tpl lint . --dirty -D warnings
```

Or with [`--json`](../reference/json.md) for anything that needs to read the findings rather than print them.
