# `git tpl status`

What is attached, where it is, and whether anything is pending.

```sh
git tpl status [--dirty]
```

```console
$ git tpl status

Template:  https://github.com/noirbizarre/rust-library-template
Ref:       refs/tpl/github-com-noirbizarre-rust-library-template

Revision:  v1.3.0 (8b3e7d1) → v1.4.0 (4f2c1a9)   template has moved
Rendered:  2 renderings
Merged:    yes
Remote:    refs/remotes/origin/tpl/github-com-noirbizarre-rust-library-template — in sync
Worktree:  clean

The template has moved. Run:
  git tpl update
```

## What each line means

**Template** — `source` from `.config/git.tpl.toml`, exactly as it is written
there.

**Ref** — `refs/tpl/<id>`, and whether it exists locally at all.

**Revision** — the revision the ref was last rendered from, and what the
configured `ref` resolves to *now*. When they differ, the template has moved and
an `update` has something to do. Both sides are written as the name asked for
plus the commit it resolved to, so a branch that moved is visible even though
its name did not change.

**Rendered** — how many renderings are on the ref. Read from the
[commit trailers](../concepts/git-model.md#what-is-in-the-commit).

**Merged** — whether the ref tip is an ancestor of `HEAD`. `no` means there is a
rendering you have not taken yet, and `git tpl diff` will show it. `n/a` when
nothing has been rendered.

**Remote** — the remote-tracking ref, and how the local ref compares to it. Only
shown when a remote copy exists. `ahead` means you have renderings to
`git tpl push`; `behind` means someone else does and you should `git tpl fetch`.

**Worktree** — clean or dirty. `update` does not care, but `merge` does.

## Options

| Option | Meaning |
|---|---|
| `--dirty` | Compare against the template's working tree rather than the revision its `ref` resolves to. Local templates only. |
| [`--json`](../reference/json.md) | A global flag. The report on stdout as one object; everything else on stderr. |

`--dirty` answers "does my uncommitted template edit change anything here?"
without committing it first. It is how an author checks a work-in-progress
against a real project.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Up to date and merged. |
| `1` | An error. |
| `2` | Something is pending — the template moved, or the ref is not merged. |

Useful in CI, where `--quiet` — a global flag — suppresses the report and
leaves only the exit code:

```sh
git tpl status --quiet || echo "template drift detected"
```

## JSON

```sh
git tpl --json status
```

```json
{
  "ok": true,
  "source": "https://github.com/noirbizarre/rust-library-template",
  "id": "github-com-noirbizarre-rust-library-template",
  "ref": "refs/tpl/github-com-noirbizarre-rust-library-template",
  "tip": "15b50a532551dd7929e38db9c69f9ae1f22fc182",
  "renderedRevision": "v1.3.0",
  "renderedCommit": "8b3e7d1f7eee32eed1f846ccc477af18b4e605d6",
  "dirty": false,
  "availableRevision": "v1.4.0 (4f2c1a9)",
  "templateMoved": true,
  "merged": true,
  "renderingCount": 2,
  "remote": {
    "ref": "refs/remotes/origin/tpl/github-com-noirbizarre-rust-library-template",
    "ahead": 0,
    "behind": 0
  },
  "worktreeClean": true,
  "pending": true
}
```

`remote` is `null` when no remote copy exists. `dirty` records whether the
rendering on the ref was produced from a template working tree rather than a
commit. Human output goes to stderr, so `--json` leaves stdout machine-readable.
