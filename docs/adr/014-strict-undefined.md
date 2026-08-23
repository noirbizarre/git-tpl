# ADR-014: Undefined names in rendered files

**Status:** accepted, staged — opt-in now, default in a later minor

## Context

git-tpl treats an unknown name in two opposite ways depending on where it
appears.

In a manifest expression — `computed`, `when`, `default`, `choices_from` — the
dependency graph rejects it before the first prompt, names it, and offers a
suggestion ([ADR-007](007-static-dependency-graph.md)). This is one of the
better diagnostics in the tool.

In a rendered file it does nothing at all. MiniJinja's default undefined
behaviour is lenient, so `{{ projct_name }}` renders to the empty string and
the command exits 0. What comes out is:

```toml
name = ""
```

which parses. Or:

```yaml
runs-on:
```

which is valid YAML. Nothing fails until a human reads the generated project,
which may be days later and in someone else's repository.

This is the worst available failure mode. It is worse still for a caller
driving git-tpl non-interactively, which has no output to read and no signal to
branch on: the render succeeded, the files exist, and one of them has a hole in
it.

The asymmetry is not defensible once noticed. It is not a decision anybody
made — it is MiniJinja's default, inherited.

## Decision

Rendered files may be strict, and eventually will be by default.

`template.toml` gains `strict = true`, which sets
`UndefinedBehavior::Strict` for file bodies and path segments. An undefined
name then fails the render with `tpl::render::content`, wrapping the
`tpl::eval::expression` that names the expression.

Manifest expressions stay lenient. The graph has already rejected an unknown
name in one, so making the environment strict there would be a second check for
a case that cannot arise — and `when`-gated questions are deliberately *absent*
from the context rather than null, which strictness would turn into an error in
a construct the documentation recommends.

`git tpl lint` reports the same names as `tpl::lint::undeclared` warnings,
whatever `strict` says.

### Staging

1. **Now.** `strict` is opt-in. `lint` warns.
2. **A later minor.** The default flips; `strict = false` opts out.

A flag day would break templates that render correctly today, for a change
whose whole purpose is to prevent silent breakage. Warning first means the
flip is a change authors have already been told about, in the words of the
diagnostic they will see.

## Consequences

Under `strict`, an intentionally optional value must say so:

```jinja
{{ maybe | default('') }}
{% if maybe is defined %}…{% endif %}
```

Both already work and both are clearer than relying on leniency — the
template states that the name may be absent, rather than the reader having to
know that it might be.

`lint` needs the set of declared names, which is the manifest's questions and
computed values plus the `data` and `template` namespaces. Names arriving from
a `${{ … }}` that MiniJinja would consume are excluded: `matrix` belongs to
GitHub Actions, and telling an author to declare it would be telling them to do
the wrong thing.

The lint cannot see through `{% import %}` — `undeclared_variables` does not
follow one — so a macro's own references are not checked. That is the same
limitation the graph analysis has, and for the same reason.

What `{% import %}`/`{% from %}` does put in the file's own namespace — the
alias itself — is checked, separately, by `tpl::lint::shadowed_name`: an alias
that reuses a question or computed name shadows it, and no name is ever
undefined, so the strict-undefined path above cannot catch it. See
[`lint`](../usage/lint.md#shadowed-names-tpllintshadowed_name).

A `when`-gated question is a third way an author can be caught out without any
name ever being undeclared. It is declared — it is a manifest key — so
`tpl::lint::undeclared` has nothing to say about it; the gap named above, in
the reason manifest expressions stay lenient, is precisely that it has no
*value* for every answer set where its `when` is false, and reading it bare in
a file that is not itself gated the same way renders fine for every other
answer set. `tpl::lint::unguarded_gate` closes that: a warning when a file
body reads a `when`-gated question without the
`is defined`/`is not defined`/`default(...)` idiom this ADR already
recommends. See
[`lint`](../usage/lint.md#unguarded-gate-reads-tpllintunguarded_gate).
