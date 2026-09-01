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

For a template rendered through an [`[extends]`](../templates/format.md#extends) chain, an `Extends` line follows
`Rendered`, naming each ancestor the way the trailers do:

```console
Extends:   https://github.com/org/base-template@a1b2c3d
```

Absent entirely for a template with no chain.

## What each line means

**Template** — `source` from `.config/git.tpl.toml`, exactly as it is written there.

**Ref** — `refs/tpl/<id>`, and whether it exists locally at all.

**Revision** — the revision the ref was last rendered from, and what the configured `ref` resolves to *now*.
When they differ, the template has moved and an `update` has something to do.
Both sides are written as the name asked for plus the commit it resolved to, so a branch that moved is visible
even though its name did not change.

**Rendered** — how many renderings are on the ref.
Read from the [commit trailers](../concepts/git-model.md#what-is-in-the-commit).

**Extends** — the [`[extends]`](../templates/format.md#extends) chain the last rendering recorded, nearest parent
first. Shown only when there is one.

**Merged** — whether the ref tip is an ancestor of `HEAD`.
`no` means there is a rendering you have not taken yet, and `git tpl diff` will show it.
`n/a` when nothing has been rendered.

**Remote** — the remote-tracking ref, and how the local ref compares to it.
Only shown when a remote copy exists.
`ahead` means you have renderings to `git tpl push`; `behind` means someone else does and you should
`git tpl fetch`.

**Worktree** — clean or dirty.
`update` does not care, but `merge` does.

## Options

| Option | Meaning |
|---|---|
| `--dirty` | Compare against the template's working tree rather than the revision its `ref` resolves to. Local templates only. |
| [`--json`](../reference/json.md) | A global flag. The report on stdout as one object; everything else on stderr. |

`--dirty` answers "does my uncommitted template edit change anything here?" without committing it first.
It is how an author checks a work-in-progress against a real project.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Up to date and merged. |
| `1` | An error. |
| `2` | Something is pending — the template moved, or the ref is not merged. |

Useful in CI, where `--quiet` — a global flag — suppresses the report and leaves only the exit code:

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
  "renderedReference": "v1.3.0",
  "renderedCommit": "8b3e7d1f7eee32eed1f846ccc477af18b4e605d6",
  "dirty": false,
  "renderedExtends": [],
  "availableReferenceDescription": "v1.4.0 (4f2c1a9)",
  "availableCommit": "4f2c1a9d5e6b7c8f9a0b1c2d3e4f5a6b7c8d9e0f",
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

`remote` is `null` when no remote copy exists.
`dirty` records whether the rendering on the ref was produced from a template working tree rather than a commit.
`renderedExtends` is the [`[extends]`](../templates/format.md#extends) chain the last rendering recorded, nearest
parent first, as `{source, revision}` objects — `[]`, not `null`, for a template with no chain, read from the
`Template-Extends` trailers rather than re-resolved, so it still answers correctly when the template cannot be
reached right now.
`availableCommit` is `null` exactly when `availableReferenceDescription` is — no template resolves, typically because
fetching it failed.
Human output goes to stderr, so `--json` leaves stdout machine-readable.

## Detaching

There is no dedicated command for this — see
[why](../development/contributing.md#things-that-will-be-declined). Removing the config is the whole thing:

```sh
git rm .config/git.tpl.toml
git commit -m "chore(tpl): detach from <template>"
```

Every git-tpl command that needs a project refuses cleanly afterwards, with a message saying there is no template
attached.

`refs/tpl/<id>` is left untouched. It is not pushed by a bare `git push` and costs nothing to keep — and it is
what a future `git tpl init` of the same template could use as a merge base again, rather than starting over with
an unrelated history.
