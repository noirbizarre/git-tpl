# `git tpl diff`

What merging the template would change.

```sh
git tpl diff [--stat] [--name-only] [--reverse] [--exit-code] [--dirty] [-- <path>...]
```

It merges `refs/tpl/<id>` into `HEAD` in memory — no ref, no index and no worktree file is touched — and diffs
`HEAD` against the result.
What you see is what `git tpl merge` would do, before you do it.

```console
$ git tpl diff --stat
  added     .github/workflows/release.yml    +48    -0
  deleted   .travis.yml                       +0   -12
  modified  README.md                         +9    -3
  modified  docs/logo.png                    Bin

4 files changed, 57 insertions(+), 15 deletions(-)
```

The counts are `git diff --stat`'s counts — the same libgit2 diff, walked hunk by hunk — so the summary line
matches what the plain-Git equivalent below prints.
A binary file shows `Bin`: two zeroes would read as "nothing changed".

## The plain Git equivalent

```sh
git diff HEAD "$(git merge-tree --write-tree HEAD refs/tpl/rust-library-template)"
```

Identical.
`git tpl diff` looks up the ref name and runs the merge for you; that is the whole of what it adds.
Pass `-- <path>` to narrow.
Use whichever you prefer; nothing about git-tpl requires its own diff command.

Note that a plain `git diff HEAD refs/tpl/<id>` is *not* the same thing.
It compares the two trees directly, so every file your project has and the template never produced — your own
sources, your `CHANGELOG.md`, git-tpl's own `.config/git.tpl.toml` — appears as a deletion.
Merging deletes none of them: they are in the merge base.

## Reading it

The direction is `HEAD` → merged, so:

- **added** — the template produces a file your project does not have
- **deleted** — the template stopped producing a file it once did, and merging would remove it
- **modified** — merging would change the file. Could be the template's change applied cleanly, or a conflict.

## Conflicts

When the merge cannot reconcile a file on its own, the preview shows it with `<<<<<<<` markers — exactly what a
merge would leave in your worktree — and says which files:

```console
$ git tpl diff --stat
warning: 1 file would conflict; shown with conflict markers
         README.md

  modified  README.md                        +12    -3

1 file changed, 12 insertions(+), 3 deletions(-)
```

The exit code is still `0`.
A conflicting preview is a correct answer, and knowing about it in advance is the point of looking.

## In CI

```sh
git tpl diff --exit-code --name-only
```

`--exit-code` exits `1` when merging the template would change something and `0` when it would not — Git's own
convention, so a job can assert "the template output has not drifted" without parsing anything.

It keys on *difference*, never on conflict.
Without the flag a conflicting preview stays at `0`, as above; with it, a conflicting preview exits `1` because
it is a difference, not because it conflicts — and a clean difference exits `1` just the same.
So `--exit-code` cannot answer "would this conflict?".
That is deliberate: a conflicting preview is a correct answer to the question asked, and failing on it would make
the flag useless on exactly the repositories that need it.
To gate on conflicts, read the `conflicts` array from `--json`.

`--json` is a reporting mode, not a separate command: the exit code means the same thing there.

## Previewing an uncommitted template edit

By default `diff` compares against `refs/tpl/<id>` — the last thing `update` rendered.
`--dirty` renders the template's *working tree* now and previews against that instead, so a template edit can be
inspected before it is committed, let alone updated to.

```sh
git tpl diff --dirty --stat
```

Answers come from the ones recorded in `.config/git.tpl.toml`, so the preview asks nothing: the question is "what
would my template edit do to this project?", not "what would different answers do?".
`--answer` overrides them for the preview only; nothing is recorded.

Nothing is written.
The preview is a commit no ref points at — a loose object `git gc` reclaims — so `refs/tpl/<id>` does not move and
a later `update` has nothing to reconcile.
Local templates only; a remote source fails with `tpl::resolve::dirty_needs_local`.

[`show --dirty`](show.md) reads one file out of the same preview, and [`status --dirty`](status.md) is the cheap
check that there is anything to look at.

## Narrowing

```sh
git tpl diff -- Cargo.toml
git tpl diff --stat -- src/
git tpl diff --name-only
```

`-- <path>` and `--reverse` apply to every mode: the patch, `--stat` and `--name-only` all report the same diff,
so they narrow and reverse alike.

## Options

| Option | Meaning |
|---|---|
| `--stat` | A per-file summary with line counts, instead of the full patch. |
| `--name-only` | Paths only, on stdout. Wins if `--stat` is also given. |
| `--reverse` | Diff the other way, merged → `HEAD`: the inverse patch. |
| `--exit-code` | Exit `1` when there is a difference, like `git diff --exit-code`. Keyed on difference, not conflict. |
| `--dirty` | Preview the template's working tree rather than the rendered ref. Local templates only. |
| `--answer`, `--answers-from`, `--defaults`, `--strict-answers` | Override the recorded answers for a `--dirty` preview. Nothing is recorded and no ref moves. |
| `-- <path>...` | Limit to these paths. |
