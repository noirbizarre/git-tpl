# `git tpl context`

Show what a template actually sees, and try one expression against it.

```sh
git tpl context . --defaults
git tpl context . --defaults --eval "{{ keywords | split(',') | map('trim') | list }}"
```

Checking a filter chain otherwise costs a whole render, and the answer arrives
buried in the output rather than stated.

## `--eval`

```console
$ git tpl context . --defaults --eval "{{ project_name | upper }}"
(a string)
"DEMO"
```

The type is printed as well as the value. `"1"` and `1` render identically and
behave differently, which is the bug about half the time.

The expression sees everything a template body sees — answers, computed values,
`data`, `template` — so it is also how you check that a `choices_from` resolved
the way you expected.

## The dump

Without `--eval`, the whole context, split the way the renderer sees it:

| Section | Contents |
|---|---|
| `answers` | What was answered or supplied. |
| `computed` | What `[computed]` produced. |
| `template` | `template.name`, `template.description`. |
| `data` | What each data source parsed to. |
| `flat` (JSON only) | Answers and computed values merged, as a body sees them. |

`flat` mirrors the renderer exactly. A dump that disagreed with it would be
worse than none, because it would be believed.

## Options

Takes the same answer flags as [`render`](render.md) — `--answer`,
`--answers-from`, `--defaults`, `--strict-answers` — along with:

| Option | Meaning |
|---|---|
| `--ref` | Branch, tag or commit to read. |
| `--root` | Use this subdirectory instead of the manifest's. |
| `--dirty` | Inspect the context an uncommitted template edit would produce. Local templates only. |
| `--trust` | Allow [network data sources](../data/index.md) without asking. |
| `--eval` | Evaluate one expression against the context and print the result. |

`--trust` is not an answer flag, and it matters here: a template with a network
data source will otherwise prompt, or fail outright where there is nobody to
ask — which is exactly the case this command is used to debug.
