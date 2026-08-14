# `git tpl init`

Attach a template to a repository, render it, and merge the result.

```sh
git tpl init <template> [--ref <ref>] [--answer k=v]... [options]
```

## What it does

1. Verifies the current directory is a Git repository (`--init` creates one).
2. Resolves the template source and its revision.
3. Loads the template manifest and any data sources it needs.
4. Builds the dependency graph and validates it.
5. Asks the questions, in dependency order.
6. Resolves computed values.
7. Writes `.config/git.tpl.toml`.
8. Renders the template.
9. Creates `refs/tpl/<id>` as an **orphan commit**.
10. Merges that commit into the current branch, allowing unrelated histories.

## The merge is the point

Step 10 is not a convenience. Without it the template commit is not an ancestor
of your branch, so the *first* `git tpl update` would have no merge base and
would conflict on every line of every file.

```
main:  A ─── M
            /
       G0 ──┘
```

`git log --graph` shows the template entering your history as a merged parent,
which is exactly what it is.

Use `--no-merge` to stop after step 9 and wire it up yourself.

## Options

| Option | Meaning |
|---|---|
| `--ref <ref>` | Branch, tag or commit. Defaults to the remote's default branch. |
| `--answer k=v` | Supply an answer, skipping its prompt. Repeatable. |
| `--defaults` | Accept every default without prompting. |
| `--id <id>` | Override the derived template id, and so the ref name. |
| `--no-merge` | Create the ref, do not merge it. |
| `--dirty` | Render the template's working tree rather than its `HEAD`. |
| `--dry-run` | Report what would be asked and rendered; create nothing. |

## Example

```console
$ git tpl init https://github.com/rawtools/rust-library-template

Template: rust-library
  A small Rust library

? Project name › my-project
? License › MIT
? Enable CI? › yes

Template: rawtools/rust-library-template
Revision: v1.4.0 (8b3e7d1)

Created refs/tpl/github-com-rawtools-rust-library-template

  added     Cargo.toml
  added     README.md
  added     src/lib.rs
  added     .github/workflows/ci.yml

Merged into main.
```

## Preconditions

**A Git repository.** `init` needs somewhere to put a ref. Pass `--init` to
create one, or run `git init` first.

**No existing `.config/git.tpl.toml`.** A project has one template. Re-running
`init` is refused, because it would silently discard the recorded answers; run
`git tpl update` instead.

**A clean worktree**, because step 10 is a merge. Uncommitted changes are
refused with the same message Git would give you.

## An empty repository

`init` in a repository with no commits works: the orphan template commit becomes
the first commit on the branch directly, with no merge — there is nothing to
merge it with.

```
main:  G0
```

Which is the cleanest possible history for a project generated from a template.
