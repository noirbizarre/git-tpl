# `git tpl update`

Re-render the template and advance `refs/tpl/<id>`.

```sh
git tpl update [options]
```

## What it does not do

**It does not touch your branch.** Not `HEAD`, not the index, not the worktree.

This is structural rather than a promise: the rendered tree is built directly
as a Git tree object and the ref is moved. There is no code path that writes a
file into your working directory, so there is nothing to go wrong.

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
new commit  (parent: the current ref tip)
        │
        ▼
refs/tpl/<template-id>
```

## Output

```console
$ git tpl update

Template: rawtools/rust-library
Revision: v1.3.0 → v1.4.0

Updated refs/tpl/rawtools-rust-library

  modified  Cargo.toml
  modified  README.md
  added     .github/workflows/release.yml

Your working tree was not modified.

Run:
  git tpl diff
  git tpl merge
```

## When nothing changed

If the rendered tree is byte-identical to the current ref tip, no commit is
made:

```console
$ git tpl update
Already up to date with rawtools/rust-library at v1.4.0.
```

This is why [determinism](../concepts/determinism.md) matters. A renderer that
varied by a timestamp would create a commit every run, and every one of them
would be noise you had to merge.

## What triggers a change

Any of:

- the template moved (a new commit on the tracked branch, or a changed `ref`)
- an answer changed in `.config/git.tpl.toml`
- a data source returned something different
- git-tpl itself renders differently

All four produce the same thing — a new commit on the ref — because from Git's
point of view they are the same event: the desired state changed.

## Append-only

The new commit's parent is the current tip. `update` never amends, never
rebases, never force-updates.

That holds even when the reason is a changed answer. Rewriting the ref would
destroy the merge base your branch already shares with it, and the next merge
would conflict on everything. See
[The Git model § Append-only](../concepts/git-model.md#append-only).

## New questions

A template that added a question since your last render has no recorded answer
for it. `update` prompts, and writes the answer back to `.config/git.tpl.toml`.

With `--defaults`, or `tpl.interactive false`, the default is taken instead; a
new question with no default is then an error.

## Options

| Option | Meaning |
|---|---|
| `--ref <ref>` | Render this revision instead of the configured one. Does not change the configuration. |
| `--answer k=v` | Override an answer for this run and record it. Repeatable. |
| `--answers-from <path>` | Read answers from a TOML, JSON or YAML file. Repeatable. See [Answers from a file](answers.md). |
| `--defaults` | Accept defaults for unanswered questions instead of prompting. |
| `--trust` | Fetch [remote data sources](../data/remote.md) without confirming. Per invocation; nothing is recorded. |
| `--dirty` | Render the template's working tree. Local templates only. |
| `--push` | Push the ref afterwards. Same as `tpl.autoPush`. |
| `--dry-run` | Report what would change; write nothing. |
