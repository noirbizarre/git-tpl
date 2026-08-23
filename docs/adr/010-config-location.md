# ADR-010: Project configuration lives at `.config/git.tpl.toml`

**Status:** accepted

## Context

The project needs a versioned file recording the template and the answers.
The obvious candidates are all at the repository root: `.git-tpl.toml`, `tpl.toml`, `.copier-answers.yml`.

Repository roots are crowded.
A generated project already has `Cargo.toml`, `README.md`, `LICENSE`, `.gitignore`, `mise.toml`,
`rust-toolchain.toml`, `cliff.toml`, `prek.toml`, `typos.toml`, `zensical.toml`.
Every tool that adds one more makes the project harder to read, and git-tpl's file is one a user looks at rarely.

## Decision

`.config/git.tpl.toml`.

It contains only the template reference and the answers.

```toml
[template]
source = "https://github.com/noirbizarre/rust-library-template"
ref = "main"

[answers]
project_name = "example"
license = "MIT"
```

Local and user preferences go in Git configuration instead, under `tpl.*`.

## Consequences

The repository root stays legible.

`.config/` is an established convention — it is where `nextest.toml` lives in this very repository — and it
groups tool configuration where a reader can find it without it competing for attention with the project's own
files.

The name `git.tpl.toml` mirrors the command, `git tpl`, so the association is immediate.

The split with Git configuration follows one rule: **would a new contributor cloning this repository need this to
be true?**
The template source, yes — a fresh clone must be understandable from this file alone, with no pre-existing Git
configuration.
`tpl.autoPush`, no — that is a statement about how one person works, and committing it imposes it on everyone.

Nothing generated goes in this file.
It is input: hand-editable, reviewable, and containing exactly what a human decided.
Rendered state lives in the ref, and provenance in the commit trailers (ADR-008).
Editing an answer here and running `git tpl update` is the supported way to change your mind.

The cost is one extra directory level, and being unlike every other tool in the category.
Both are fine.
