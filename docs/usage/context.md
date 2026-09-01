# `git tpl context`

Show what a template actually sees, and try one expression against it.

```sh
git tpl context . --defaults
git tpl context . --defaults --eval "{{ keywords | split(',') | map('trim') | list }}"
```

Checking a filter chain otherwise costs a whole render, and the answer arrives buried in the output rather than
stated.

## `--eval`

```console
$ git tpl context . --defaults --eval "{{ project_name | upper }}"
(a string)
"DEMO"
```

The type is printed as well as the value.
`"1"` and `1` render identically and behave differently, which is the bug about half the time.

The expression sees everything a template body sees — answers, computed values, `data`, `template` — so it is
also how you check that a `choices_from` resolved the way you expected.

## The dump

Without `--eval`, the whole context, split the way the renderer sees it:

| Section | Contents |
|---|---|
| `answers` | What was answered or supplied. |
| `computed` | What `[computed]` produced. |
| `Gated defaults` (JSON: `gatedDefaults`) | Defaults injected for skipped [`default_when_skipped`](../templates/questions.md#keeping-a-default-when-skipped) questions — not answers. |
| `template` | `template.name`, `template.description`. |
| `data` | What each data source parsed to. |
| `flat` (JSON only) | Answers, computed values and gated defaults merged, as a body sees them. |

`flat` mirrors the renderer exactly.
A dump that disagreed with it would be worse than none, because it would be believed.

## `extends` (JSON only)

For a template with an [`[extends]`](../templates/format.md#extends) chain, `--json`'s payload carries an
`extends` key alongside the sections above:

```json
"extends": {
  "chain": [{ "source": "https://github.com/org/base-template", "revision": "a1b2c3d..." }],
  "questions": { "license": 0 },
  "data": { "licenses": 0 }
}
```

`chain` is the ancestor chain, nearest parent first. `questions` and `data` map a name to an index into `chain` —
which layer's declaration is the one currently in effect — for every question or data source an ancestor
contributes; a name the template's own manifest declares or overrides is simply absent. `chain` is `[]` and
`questions`/`data` are `{}` for a template with no `[extends]`, always present so a script never has to check
whether the key exists first.

This is a debugging aid, not a rendering concern: `Context` itself carries no such metadata — see
[ADR-006](../adr/006-no-runtime-context.md) — it is built separately, from the resolved template, at the point
`--json` writes its payload.

## No local data sources

A `local` data source is resolved relative to the project root, and `context` has no project — like `render`, it
builds the context in isolation.
It fails with `tpl::data::needs_project` rather than being resolved against the working directory — that would
make the same template, the same answers and the same revision report a different context depending on where the
command was run.

Use a `template` source for data that belongs to the template.

## Options

Takes the same answer flags as [`render`](render.md) — `--answer`, `--answers-from`, `--defaults`,
`--strict-answers` — along with:

| Option | Meaning |
|---|---|
| `--ref` | Branch, tag or commit to read. |
| `--root` | Use this subdirectory instead of the manifest's. |
| `--dirty` | Inspect the context an uncommitted template edit would produce. Local templates only. |
| `--trust` | Allow [network data sources](../data/index.md) without asking. |
| `--eval` | Evaluate one expression against the context and print the result. |

`--trust` is not an answer flag, and it matters here: a template with a network data source will otherwise
prompt, or fail outright where there is nobody to ask — which is exactly the case this command is used to debug.

`--ref` and `--dirty` are mutually exclusive — asking for both is refused at the parser. With neither, a local
template resolves its checked-out branch's `HEAD`; a URL resolves the remote's default branch.
