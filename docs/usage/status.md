# `git tpl status`

What is attached, where it is, and whether anything is pending.

```sh
git tpl status [--format json]
```

```console
$ git tpl status

Template:  noirbizarre/rust-library-template
Ref:       refs/tpl/github-com-noirbizarre-rust-library-template

Revision:  v1.3.0 (8b3e7d1)  →  v1.4.0 (4f2c1a9)   template has moved
Rendered:  2 commits, last at 8b3e7d1
Merged:    yes, at commit c4d5e6f
Remote:    origin — 1 ahead
Worktree:  clean

Run `git tpl update` to render v1.4.0.
```

## What each line means

**Template** — `source` from `.config/git.tpl.toml`.

**Ref** — `refs/tpl/<id>`, and whether it exists locally at all.

**Revision** — the revision the ref was last rendered from, and what the
configured `ref` resolves to *now*. When they differ, the template has moved and
an `update` has something to do.

**Rendered** — the ref's own history: how many renderings, and the template
commit behind the latest. Read from the
[commit trailers](../concepts/git-model.md#what-is-in-the-commit).

**Merged** — whether the ref tip is an ancestor of `HEAD`. `no` means there is a
rendering you have not taken yet, and `git tpl diff` will show it.

**Remote** — how the local ref compares to `refs/remotes/<remote>/tpl/<id>`.
Only shown when a remote copy exists. `ahead` means you have renderings to
`git tpl push`; `behind` means someone else does and you should `git tpl fetch`.

**Worktree** — clean or dirty. `update` does not care, but `merge` does.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Up to date and merged. |
| `1` | An error. |
| `2` | Something is pending — the template moved, or the ref is not merged. |

Useful in CI:

```sh
git tpl status --quiet || echo "template drift detected"
```

## JSON

```sh
git tpl status --format json
```

Human output goes to stderr, so `--format json` leaves stdout machine-readable.
