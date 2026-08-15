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
running.

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

## In CI

```yaml
- run: git tpl lint . --dirty
```

Or with [`--json`](../reference/json.md) for anything that needs to read the
findings rather than print them.
