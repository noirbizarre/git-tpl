# `git tpl diff`

What merging the template would change.

```sh
git tpl diff [--stat] [--name-only] [--reverse] [-- <path>...]
```

It is a diff between `HEAD` and `refs/tpl/<id>`, computed by libgit2 — the same
machinery `git diff` uses, so the output is a diff you already know how to read.

```console
$ git tpl diff --stat
  added     .github/workflows/release.yml    +48    -0
  deleted   NOTES.md                          +0   -12
  modified  README.md                         +9    -3
  modified  docs/logo.png                    Bin

4 files changed, 57 insertions(+), 15 deletions(-)
```

The counts are `git diff --stat`'s counts — the same libgit2 diff, walked hunk
by hunk — so the summary line matches what the plain-Git equivalent below
prints. A binary file shows `Bin`: two zeroes would read as "nothing changed".

## The plain Git equivalent

```sh
git diff HEAD refs/tpl/github-com-noirbizarre-rust-library-template
```

Identical. `git tpl diff` looks up the ref name for you; that is the whole of
what it adds. Both compare the entire tree, so files you have that the template
does not — including `.config/git.tpl.toml` — appear as deletions. Pass
`-- <path>` to narrow. Use whichever you prefer; nothing about git-tpl requires
its own diff command.

## Reading it

The direction is `HEAD` → template, so:

- **added** — the template has a file your project does not
- **deleted** — your project has a file the template no longer produces, *or*
  never produced. A file you created yourself shows here.
- **modified** — both have it, and they differ. Could be your edit, the
  template's, or both.

!!! note "A large diff at first is normal"

    Before the first merge, everything the template produces that you have since
    changed appears as a difference. That is what it is telling you. After a
    merge the diff is empty, and thereafter it shows only what has genuinely
    accumulated.

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
| `--reverse` | Diff the other way, template → `HEAD`. |
| `-- <path>...` | Limit to these paths. |
