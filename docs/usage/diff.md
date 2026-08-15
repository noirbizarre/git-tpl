# `git tpl diff`

What merging the template would change.

```sh
git tpl diff [--stat] [--name-only] [--reverse] [-- <path>...]
```

It merges `refs/tpl/<id>` into `HEAD` in memory — no ref, no index and no
worktree file is touched — and diffs `HEAD` against the result. What you see is
what `git tpl merge` would do, before you do it.

```console
$ git tpl diff --stat
  added     .github/workflows/release.yml    +48    -0
  deleted   .travis.yml                       +0   -12
  modified  README.md                         +9    -3
  modified  docs/logo.png                    Bin

4 files changed, 57 insertions(+), 15 deletions(-)
```

The counts are `git diff --stat`'s counts — the same libgit2 diff, walked hunk
by hunk — so the summary line matches what the plain-Git equivalent below
prints. A binary file shows `Bin`: two zeroes would read as "nothing changed".

## The plain Git equivalent

```sh
git diff HEAD "$(git merge-tree --write-tree HEAD refs/tpl/rust-library-template)"
```

Identical. `git tpl diff` looks up the ref name and runs the merge for you; that
is the whole of what it adds. Pass `-- <path>` to narrow. Use whichever you
prefer; nothing about git-tpl requires its own diff command.

Note that a plain `git diff HEAD refs/tpl/<id>` is *not* the same thing. It
compares the two trees directly, so every file your project has and the template
never produced — your own sources, your `CHANGELOG.md`, git-tpl's own
`.config/git.tpl.toml` — appears as a deletion. Merging deletes none of them:
they are in the merge base.

## Reading it

The direction is `HEAD` → merged, so:

- **added** — the template produces a file your project does not have
- **deleted** — the template stopped producing a file it once did, and merging
  would remove it
- **modified** — merging would change the file. Could be the template's change
  applied cleanly, or a conflict.

## Conflicts

When the merge cannot reconcile a file on its own, the preview shows it with
`<<<<<<<` markers — exactly what a merge would leave in your worktree — and says
which files:

```console
$ git tpl diff --stat
warning: 1 file would conflict; shown with conflict markers
         README.md

  modified  README.md                        +12    -3

1 file changed, 12 insertions(+), 3 deletions(-)
```

The exit code is still `0`. A conflicting preview is a correct answer, and
knowing about it in advance is the point of looking.

## Narrowing

```sh
git tpl diff -- Cargo.toml
git tpl diff --stat -- src/
git tpl diff --name-only
```

`-- <path>` and `--reverse` apply to every mode: the patch, `--stat` and
`--name-only` all report the same diff, so they narrow and reverse alike.

## Options

| Option | Meaning |
|---|---|
| `--stat` | A per-file summary with line counts, instead of the full patch. |
| `--name-only` | Paths only, on stdout. Wins if `--stat` is also given. |
| `--reverse` | Diff the other way, merged → `HEAD`: the inverse patch. |
| `-- <path>...` | Limit to these paths. |
