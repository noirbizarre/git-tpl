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
7. Renders the template.
8. Writes `.config/git.tpl.toml`, and stages it. A template that fails to
   render leaves no half-initialised project behind, which is why this comes
   after the rendering rather than before it.
9. Creates `refs/tpl/<id>` as an **orphan commit**.
10. Merges that commit into the current branch, allowing unrelated histories.
11. Adds any [`[remotes]`](../templates/format.md#remotes) the template declared.
12. Shows the template's own [note](../templates/format.md#talking-to-the-user), if it has one.

Steps 11 and 12 come last, after everything git-tpl did itself, and happen on
`init` only. `git tpl update` does neither: it is a ref-only operation, and that
is most of its value.

## What a template may and may not do afterwards

It may add a Git remote, and it may say something to you. That is the whole
list, and it is closed —
[ADR-019](../adr/019-templates-address-never-act.md) states the bar for
anything joining it.

It may **not** run anything. There are no hooks, no scripts and no post-render
commands, on `init` or ever. A template that needs `npm install` renders a
`scripts/bootstrap.sh` and now, at last, has a way to tell you it is there; you
run it. A note that says "run `curl … | sh`" is exactly as trustworthy as a
`README.md` that says it.

A note is shown in a block attributed to the template, so it cannot be mistaken
for git-tpl's own output, and it is stripped of everything a terminal would act
on beyond colour and an `https` link. It is read from the template repository
and is never written into your project.

A `note_file` naming nothing is refused, and refused *early*: the note is
resolved before the rendered ref is created and before the merge, so a template
with a wrong path leaves your repository exactly as it found it.

An existing remote is never repointed. If the template declares an `origin` and
you already have one somewhere else, yours stays and a warning names both.

## The merge is the point

Step 10 is not a convenience. Without it the template commit is not an ancestor
of your branch, so the *first* `git tpl update` would have no merge base — and
without one, Git cannot tell your edits from the template's. Every file that
differs would conflict, including files you customised that the template never
changed.

```
main:  A ─── M
            /
       G0 ──┘
```

`git log --graph` shows the template entering your history as a merged parent,
which is exactly what it is.

Use `--no-merge` to stop after step 9 and wire it up yourself.

`<template>` is any Git URL or local path. If you have defined
[shortcuts](../configuration.md#shortcuts), a leading `<name>:` is expanded
first — and it is the expanded URL that is recorded in the project, so the
shortcut never leaves your machine.

## Options

| Option | Meaning |
|---|---|
| `--ref <ref>` | Branch, tag or commit. Defaults to the remote's default branch. |
| `--init` | Create the repository if there is not one here. |
| `--answer k=v` | Supply an answer, skipping its prompt. Repeatable. |
| `--answers-from <path>` | Read answers from a TOML, JSON or YAML file. Repeatable. See [Answers from a file](answers.md). |
| `--defaults` | Accept every default without prompting. |
| `--trust` | Fetch [remote data sources](../data/remote.md) without confirming. Per invocation; nothing is recorded. |
| `--id <id>` | Override the derived template id, and so the ref name. |
| `--no-merge` | Create the ref, do not merge it. |
| `--dirty` | Render the template's working tree rather than its `HEAD`. Local templates only. |
| `--dry-run` | Report what would be asked and rendered; create nothing. |

## Example

```console
$ git tpl init https://github.com/noirbizarre/rust-library-template

? Project name › my-project
? License › MIT
? Enable CI? › yes

Template:  https://github.com/noirbizarre/rust-library-template
Revision:  v1.4.0 (8b3e7d1)

Created refs/tpl/github-com-noirbizarre-rust-library-template

  added     Cargo.toml
  added     README.md
  added     src/lib.rs
  added     .github/workflows/ci.yml

Merged into main.

Answers recorded in .config/git.tpl.toml and committed.
```

**Template** is the source exactly as you typed it, not the manifest's `name`:
it is what `.config/git.tpl.toml` will record, and what a later `--ref` or
`--id` will be resolved against.

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

## An existing project

`init` also works on a project that already has files — one generated last year
by another tool, or written by hand and since grown to resemble a template.
There is no separate command, because there is no separate behaviour: the orphan
commit is merged, and Git reconciles the two sides.

```sh
cd my-existing-project
git tpl init https://github.com/noirbizarre/rust-library-template
```

**Expect conflicts, and expect them to be small.** Because there is no merge
base, Git compares the two sides by content:

| Your project | What happens |
|---|---|
| A file identical to the rendered one | Merged silently. Nothing to reconcile. |
| A file that differs | Conflicts **only on the lines that differ**, not the whole file. |
| A file the template renders and you lack | Added and staged for you. |
| A file the template does not render | Untouched. It is not the template's. |

So a `README.md` you changed one line of comes back as a three-line conflict,
not a page of markers.

Resolve them as you would any merge — this is ordinary Git, and git-tpl
contributes no conflict resolution of its own ([ADR-002](../adr/002-no-custom-reconciliation.md)):

```console
$ git tpl init ../rust-library-template --defaults

Template:  ../rust-library-template
Revision:  main (7fa834c)

Created refs/tpl/rust-library-template

  added     Cargo.toml
  added     README.md
  added     src/lib.rs

warning: the merge left conflicts. Resolve them and commit:

  README.md

  git status
  git commit

Answers recorded in .config/git.tpl.toml and staged.

$ git mergetool          # or edit the markers by hand
$ git commit
```

That first merge is the only awkward one. Afterwards `G0` is an ancestor of your
branch, so every `git tpl update` from then on is a small diff against a genuine
merge base — the same experience as a project generated from the template on day
one.

!!! tip "Look before you merge"

    `git tpl init <template> --no-merge` creates the ref without merging, so
    `git tpl diff` shows exactly what the merge would reconcile. When you are
    ready, `git tpl merge` performs it. `--dry-run` is smaller still: it reports
    what would be asked and rendered, and creates nothing.

## Machine-readable output

`git tpl --json init` emits its outcome on stdout as a single JSON object, with
the prose on stderr. The payload is described in
[JSON output](../reference/json.md#init).
