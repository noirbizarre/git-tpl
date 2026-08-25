# ADR-025: A question may keep its default when skipped

**Status:** accepted

## Context

[ADR-014](014-strict-undefined.md) and `tpl::lint::unguarded_gate` name a real hazard: a `when`-gated question is
declared, so nothing warns about a file that reads it bare, but it has no value at all for every answer set
where its `when` is false. The documented fix is a guard at the call site:

```jinja
{% if docs_accent is defined %}accent: {{ docs_accent }}{% endif %}
{{ docs_accent | default('blue') }}
```

That is the right default, and stays the right default. But it is a per-call-site fix for something that is
often a per-question fact: the author already wrote `default = "blue"` once, and every guard at every call site
just repeats it back. Migrating a template from a tool that keeps a skipped question's default in context (a
deliberate difference from git-tpl's "absent, not null" behaviour, see `questions.md`) surfaces exactly this —
tens of warnings, each fixed by re-typing a default the manifest already declares.

## Decision

A question gains `default_when_skipped`, a boolean, default `false`. When true and the question's `when`
evaluates false, its own `default` (literal or expression, evaluated the same way an asked question's would be)
is injected into the render context under the question's name.

The question is still not asked. Its injected default is still not an answer:

- It is absent from `Context::answers()`, so it never reaches `.config/git.tpl.toml`.
- It is absent from `Context::answers_digest()`, so it never appears in a commit trailer or changes what
  `status` reports.
- The dependency graph is unaffected — it already added an edge for `default`'s own references
  (`graph.rs`), regardless of whether any run evaluates it, so ordering was already correct.

Concretely, `Context` gains a fourth bucket, `gated_defaults`, wired into `to_minijinja()`, `to_json()` and
`get_path()` exactly like `answers`/`computed`, but never read by `answers()` or `answers_digest()`. A name
lives in exactly one bucket: a question resolves to either "answered" or "skipped, default injected", never
both, so there is no collision to arbitrate.

Validated at load time: `default_when_skipped = true` with no `when` (nothing to skip) or no `default` (nothing
to inject) is rejected as `tpl::manifest::invalid_question`, the same way `message` without `pattern` already is.

`tpl::lint::unguarded_gate` excludes a `default_when_skipped` question from its gated-name set: it is never
actually absent, so an unguarded read of it is not the trap the rule exists to catch.

## Consequences

The trade-off is explicit and per-question: `docs_accent is defined` becomes `true` whether `docs` is or is not,
once the flag is set. That gives up the "not applicable vs. declined" distinction `questions.md` and ADR-014
build the guard idiom to preserve — for that one question, by that question's author's own choice, not as a
change to the default behaviour every other question keeps.

Two cases are explicitly out of scope for this cut, the same way transitive gating already is for
`unguarded_gate`:

- **The empty-`choices` skip.** A `choice`/`multi_choice` question whose `choices_from` filters to nothing is
  skipped the same way a false `when` is, but `default_when_skipped` does not extend to it — the two skips are
  triggered by different manifest keys, and the issue this ADR answers is about `when` specifically.
- **Propagation through `computed`.** A computed value that reads a `default_when_skipped` question sees the
  injected default like any other reachable name, with nothing new to configure — but gatedness itself is not
  tracked through `computed`, matching the limitation `unguarded_gate`'s own doc comment already names.

Both are the "if we ever need it" path, not a decision to revisit without a concrete case, per
[the contributing guidelines](../development/contributing.md).
