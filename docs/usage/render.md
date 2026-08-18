# `git tpl render`

Render a template into a directory. No project, no ref, no merge.

```sh
git tpl render ./my-template --output ./out --defaults
```

Every other command renders *into* a repository, so asking "what does this
template produce?" meant creating one first: `git init`, `git tpl init --dirty`,
and a Git identity to commit the ref. This is that question with nothing else
attached.

Writing files is not a hole in the [ref model](../concepts/git-model.md):
`update` never touches `HEAD`, the index or the worktree, and this is a
different command whose entire purpose is a required flag.

## Options

| Flag | Effect |
|---|---|
| `--output`, `-o` | Where to write. Required. |
| `--ref` | Branch, tag or commit to render. |
| `--root` | Render this subdirectory instead of the manifest's. |
| `--dirty` | Render the template's working tree. Local templates only. |
| `--force` | Replace the contents of a non-empty output directory. |
| `--answer`, `--answers-from`, `--defaults` | As for [`init`](init.md). |
| `--strict-answers` | Fail on an answer that names no question. |
| `--trust` | Allow remote data sources without asking. |

## The output directory is cleared, not merged into

A template that stops producing a file has to be seen to stop. Rendering over a
previous run would leave the old file behind, and the author would conclude
their conditional works.

So a non-empty directory is refused, and `--force` clears it rather than
overwriting file by file.

## The authoring loop

```sh
git tpl render . --dirty -o /tmp/out --answers-from tests/answers/minimal.toml --defaults
cd /tmp/out && cargo build && actionlint .github/workflows/*.yaml
```

`--dirty` renders the working tree, so this is edit-and-see rather than
edit-commit-and-see.

!!! warning "A `--dirty` render honours `.gitignore`"

    It renders what Git would see, because that is what a committed revision
    would contain — so a new template file still matched by an ignore rule is
    left out, and the rendering is missing a file you can see on disk.

    The paths are named on stderr, listed under `-v`, and carried in
    `skippedByGitignore` under `--json`. The stack includes
    `core.excludesFile`, so a global rule set years ago on an unrelated project
    can be the cause — which is why the omission is reported rather than left
    to be noticed. See [ADR-017](../adr/017-ignore-evaluation.md).

Checking the *output* with the tools that understand it is the intended
division of labour. git-tpl does not run anything over a rendering, and
[will not](../adr/003-minijinja-only.md): your own CI does it better, and
executing a template's wishes is the one thing the renderer must never do.

## No local data sources

A `local` data source is resolved relative to the project root, and there is no
project. It fails with `tpl::data::needs_project` rather than being resolved
against the working directory — that would make the same template, the same
answers and the same revision render differently depending on where the command
was run.

Use a `template` source for data that belongs to the template.
