# `git tpl update`

Re-render the template and advance `refs/tpl/<id>`.

```sh
git tpl update [options]
```

## What it does not do

**It does not touch your branch.** Not `HEAD`, not the index, not the worktree.

This is structural rather than a promise: the rendered tree is built directly as a Git tree object and the ref is
moved.
There is no code path that writes a file into your working directory, so there is nothing to go wrong.

```console
$ git tpl update
...
$ git status
On branch main
nothing to commit, working tree clean
```

## What it does

```
.config/git.tpl.toml
        │
        ▼
template source/ref
        │
        ▼
fetch/resolve template
        │
        ▼
load data sources
        │
        ▼
resolve answers/computed values
        │
        ▼
MiniJinja rendering
        │
        ▼
new Git tree
        │
        ▼
migrations newly crossed?  (see below) ──yes──▶ rename-only commit, if a move
        │ no                                    lands beside other changes
        ▼                                        │
new commit  (parent: the current ref tip,        │
             or the rename-only commit above) ◀──┘
        │
        ▼
refs/tpl/<template-id>
```

## Output

```console
$ git tpl update

Template:  https://github.com/noirbizarre/rust-library-template
Revision:  v1.3.0 (8b3e7d1) → v1.4.0 (4f2c1a9)

Updated refs/tpl/github-com-noirbizarre-rust-library-template

  modified  Cargo.toml
  modified  README.md
  added     .github/workflows/release.yml

── from the template ──────────────────────────────────────
  0.4 split `config.rs` into a module; your overrides moved
  with it.
────────────────────────────────────────────────────────────

Your working tree was not modified.

Run:
  git tpl diff
  git tpl merge
```

## When nothing changed

If the rendered tree is byte-identical to the current ref tip, no commit is made:

```console
$ git tpl update
Already up to date with https://github.com/noirbizarre/rust-library-template at v1.4.0 (4f2c1a9).
```

This is why [determinism](../concepts/determinism.md) matters.
A renderer that varied by a timestamp would create a commit every run, and every one of them would be noise you
had to merge.

## What triggers a change

Any of:

- the template moved (a new commit on the tracked branch, or a changed `ref`)
- an answer changed in `.config/git.tpl.toml`
- a data source returned something different
- git-tpl itself renders differently

All four produce the same thing — a new commit on the ref — because from Git's point of view they are the same
event: the desired state changed.

## Append-only

The new commit's parent is the current tip. `update` never amends, never rebases, never force-updates.

That holds even when the reason is a changed answer.
Rewriting the ref would destroy the merge base your branch already shares with it, and the next merge would
conflict on everything.
See [The Git model § Append-only](../concepts/git-model.md#append-only).

Occasionally `update` adds **two** commits instead of one: a migration that moves a file alongside some other
change gets a content-identical rename commit first, then the ordinary rendered commit as its child.
Both are ordinary commits on the same append-only history; see [Migrations](#migrations).

## Migrations

A template repository may declare **migrations** in a `migrations/` directory, sibling to `template.toml`.
Each file may carry a message and a list of path moves:

```toml
# migrations/2026-08-config-move.toml
message = "0.4 split `config.rs` into a module; your overrides moved with it."

[[moves]]
from = "src/config.rs"
to = "src/config/mod.rs"
```

No template ever declares a version, and no project ever records one: `update` discovers a migration by diffing
the template repository's own tree between the revision your project was previously rendered from and the one it
just resolved.
A migration is discovered — its message shown, its moves applied — exactly once, at whichever `update` first
crosses it, however many template commits separate the two renderings.
See [ADR-024](../adr/024-template-migrations.md) for the full reasoning, including why a move is applied as its
own commit rather than folded into the ordinary one.

A migration message is sanitised and framed exactly like a template's `init`-time note (see [`init`](init.md)):
shown once, in an attributed block, never executed.
Its `--json` field carries the raw text, since a `--json` consumer is not a terminal.

`git tpl lint` validates a migration file's shape without a project.
Whether its declared moves actually apply — whether `from` exists in your project's previous rendering — can only
be known at `update` time, and a mismatch there is refused before any commit is written.

## When there is no ref to advance

`update` needs `refs/tpl/<id>` to hang the new commit from.
If it is not there, the commit is written as an orphan and `update` says so:

```console
$ git tpl update
...
No refs/tpl/rust existed here, so this update started a new history.
If the ref exists on a remote, run `git tpl fetch` before merging: without a
merge base, `git tpl merge` can conflict on every file.
```

Two things cause it, and both are legitimate, which is why this is a warning and not a refusal:

- **You cloned a project and never fetched the ref.** `refs/tpl/*` is not fetched by default. Run
  [`git tpl fetch`](fetch.md) first, and the update continues the history everyone else has.
- **You edited `source` or `id` in `.config/git.tpl.toml`.** The ref name is derived from them, so a new one means
  a new ref. That is the documented way to
  [point a project at a different template](../templates/local-development.md#pointing-an-existing-project-at-a-different-template),
  and starting a fresh history is what you asked for.

## New questions

A template that added a question since your last render has no recorded answer for it.
`update` prompts, and writes the answer back to `.config/git.tpl.toml`.

With `--defaults`, or `tpl.interactive false`, the default is taken instead; a new question with no default is
then an error.

## Options

| Option | Meaning |
|---|---|
| `--ref <ref>` | Render this revision instead of the configured one. Does not change the configuration. |
| `--answer k=v` | Override an answer for this run and record it. Repeatable. |
| `--answers-from <path>` | Read answers from a TOML, JSON or YAML file. Repeatable. See [Answers from a file](answers.md). |
| `--defaults` | Accept defaults for unanswered questions instead of prompting. |
| `--strict-answers` | Fail on an answer that names no question, rather than warning. |
| `--trust` | Fetch [remote data sources](../data/remote.md) without confirming. Per invocation; nothing is recorded. |
| `--dirty` | Render the template's working tree. Local templates only. |
| `--push` | Push the ref afterwards. Same as `tpl.autoPush`. |
| `--remote <name>` | The remote `--push` uses. Default `origin`, or `tpl.remote`. |
| `--dry-run` | Report what would change; write nothing. |

## Machine-readable output

`git tpl --json update` emits its outcome on stdout as a single JSON object, with the prose on stderr.
The payload is described in [JSON output](../reference/json.md#update).
