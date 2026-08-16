# `git tpl lint`

Check a template without rendering it. No project, no network.

```sh
git tpl lint .            # the template in this directory
git tpl lint . --dirty    # including uncommitted changes
```

A render only ever proves the branch it took. The failures that hurt are the
ones a given answer set never reaches — and the worst of them are silent.

Exit code is 1 when there is an error, 0 when there are only warnings. A lint
that fails on things a template may legitimately mean is a lint people stop
running. When a particular template *has* decided it never means one of them,
[`--deny`](#choosing-what-fails) says so.

## What it checks

### The manifest and its graph

Everything `init` would check before the first prompt: unknown references,
cycles, incoherent question declarations. The difference is that this needs no
repository and no network.

### Every `.jinja` file parses

Including branches no answer set reaches. Otherwise a syntax error in a rarely
taken conditional is found by the first person who answers their way into it.

### Conditional path segments — `tpl::lint::degenerate_path`

The one that motivates the command:

```
.github/workflows/{% if msrv %}msrv{% endif %}.yaml
```

The `.jinja` suffix is stripped from the whole path *before* the segments are
rendered. For a file that is not a template, the `.yaml` therefore sits outside
the block — so with `msrv` false the segment renders to `.yaml`, which is
non-empty, is not `.` or `..`, and contains no separator. Every check the
renderer makes passes, and it writes a file called `.yaml`.

Two such files collide, and `tpl::render::collision` names them both. **One is
silent.**

The fix is to put the whole name inside the block:

```
.github/workflows/{% if msrv %}msrv.yaml{% endif %}
```

For a `.jinja` file the outer form is correct, because the suffix is stripped
first — `{% if docs %}zensical.toml{% endif %}.jinja` collapses to nothing, as
intended. The check knows the difference.

### Foreign expressions — `tpl::lint::foreign_expression`

`${{ github.ref }}` contains `{{`, so MiniJinja consumes it: the result is `$`,
the YAML is still valid, and nothing fails until the workflow runs.

Three ways out, and the lint names all three:

- wrap the region in `{% raw %}…{% endraw %}`;
- drop the `.jinja` suffix, so the file is copied byte-for-byte;
- escape it as `${{ '{{' }} github.ref {{ '}}' }}`.

The escape idiom is not flagged. A workflow that interpolates anything has to
write it on every line.

### Undeclared names — `tpl::lint::undeclared`

A name a file body uses that the template never declares. MiniJinja is lenient,
so `{{ projct_name }}` renders to an empty string and the command succeeds,
leaving `name = ""` in a `Cargo.toml` that parses.

A warning, because that is still the default. Set `strict = true` in
`template.toml` to make it an error at render time.

Names that came from a `${{ … }}` are not reported: `matrix` belongs to GitHub
Actions, and advising an author to declare it would be advice not to take.

## Choosing what fails

The default severities are a judgement about templates in general. A given
template may have a firmer opinion — a workflow repository that never means a
raw `${{ }}`, say. Two repeatable flags, spelled as `cargo clippy` spells them:

| Flag | Effect |
|---|---|
| `-D`, `--deny <CODE\|warnings>` | The finding fails the lint |
| `-A`, `--allow <CODE\|warnings>` | The finding is not reported at all |

Both take either the word `warnings`, meaning the whole severity, or a single
`tpl::lint::*` [code](../reference/diagnostics.md#linting).

```sh
git tpl lint . -D warnings                        # any warning fails
git tpl lint . -D tpl::lint::foreign_expression   # only that one fails
git tpl lint . -A tpl::lint::undeclared           # stop reporting that one
```

A named code always overrides `warnings`, so an exception is a matter of
naming it:

```sh
# Everything fatal, except the code this template is still migrating away from
git tpl lint . -D warnings -A tpl::lint::undeclared
```

Precedence is by specificity, not by position: writing the `-A` first means the
same thing. Unlike clippy, where the last flag wins, arguments here can be
reordered by a shell fragment or a composed CI config without changing what the
build means. Naming the same code in both flags is an error rather than a
coin-toss, as is `-D warnings -A warnings`.

A misspelled code is an error too — `tpl::lint::unknown_code`, listing the
valid ones. Accepting it would deny nothing, and the symptom would be a green
CI run.

Denying does not rewrite a severity. A denied warning is still reported as a
warning, marked `(denied)`, and [`--json`](../reference/json.md#lint) keeps
`"severity": "warning"` beside `"denied": true` — so a consumer can tell a rule
the template broke from a policy this run applied.

## In CI

```yaml
- run: git tpl lint . --dirty
```

For a repository where warnings are errors:

```yaml
- run: git tpl lint . --dirty -D warnings
```

Or with [`--json`](../reference/json.md) for anything that needs to read the
findings rather than print them.
