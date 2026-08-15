# `git tpl diff`

What merging the template would change.

```sh
git tpl diff [--stat] [--name-only] [--reverse] [-- <path>...]
```

It is a diff between `HEAD` and `refs/tpl/<id>`, computed by libgit2 — the same
machinery `git diff` uses, so the output is a diff you already know how to read.

```console
$ git tpl diff --stat
  added     .github/workflows/release.yml
  deleted   NOTES.md
  modified  README.md

3 file(s) differ
```

`--stat` lists what changed, not how much: there are no insertion or deletion
counts. Run the plain-Git equivalent below for a `git diff --stat` diffstat.

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
git tpl diff --name-only
```

## Options

| Option | Meaning |
|---|---|
| `--stat` | Summary instead of the full patch. |
| `--name-only` | Paths only. |
| `--reverse` | Diff the other way, template → `HEAD`. |
| `-- <path>...` | Limit to these paths. |
