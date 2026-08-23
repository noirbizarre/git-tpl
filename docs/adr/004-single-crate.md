# ADR-004: A single crate, not a workspace

**Status:** accepted

## Context

The natural instinct for a project with a domain model, a renderer, a data layer, a Git abstraction and two
frontends is a workspace: `tpl-core`, `tpl-render`, `tpl-data`, `tpl-git`, `git-tpl`, `gh-tpl`.
Crate boundaries enforce the layering.

Two things argued against it.

The `gh-tpl` frontend — the strongest reason for a workspace, since it is the one component that would carry
GitHub-specific dependencies the core must not have — was dropped from the initial implementation.

And the internal boundaries are not yet proven.
Where exactly `context` ends and `eval` begins, whether `graph` is part of `template` or separate, whether `data`
deserves its own layer — these are answered by writing the code, and every answer that turns out wrong is a
refactor.
A crate boundary makes each of those a Cargo edit and a visibility audit rather than moving a function.

The sibling projects (gh-ship, gh-settings) are both single-crate for the same reason.
gh-settings records it as ADR-013.

## Decision

One crate: `[lib] name = "tpl"` and `[[bin]] name = "git-tpl"`.

Layering is expressed as modules, with `git` and `data` behind traits so they remain extractable.

## Consequences

Refactoring is cheap during the period when it is most needed.

One `Cargo.toml`, one version, one build.
Faster compiles.

Integration tests can use the library for engine-level assertions and the binary for output snapshots, which is
what `[lib]` + `[[bin]]` is for.

The layering is no longer mechanically enforced by Cargo.
For the boundary that matters most — Git backend isolation — we enforce it with a hook instead, which is both
cheaper and stricter (ADR-011).

`gh-tpl`, when it arrives, is a second `[[bin]]`, or a promotion of this package to a workspace member.
Neither requires moving code.
