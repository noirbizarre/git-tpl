# Contributing

## Before opening a PR

```sh
mise run ci
```

Runs formatting, Clippy, spelling, workflow and documentation linting, the tests and the docs build — everything CI
does.

## Commits

[Conventional Commits](https://www.conventionalcommits.org). Enforced by commitlint through a `commit-msg` hook,
and used by git-cliff to generate the changelog, so the type you choose becomes a section heading in the release
notes.

```
feat(render): support conditional directories
fix(git): report the URL when SSH authentication fails
docs: explain why update never touches the worktree
```

Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `style`, `build`, `ci`, `chore`, `revert`.

A breaking change gets a `!` or a `BREAKING CHANGE:` trailer. git-tpl writes to Git refs and defines an on-disk
template format; a break in either must be visible in the version number.

## What a change needs

**Tests.** A behaviour without a test is a behaviour that will regress. A bug fix needs a test that fails before
it.

**Documentation.** A user-visible feature is not finished until the relevant page describes it. Documentation is
written alongside the implementation, in the same PR — not afterwards, because afterwards does not arrive.

**A reason.** Comments that explain *why*, especially where the obvious approach was rejected.

## Architecture decisions

Anything that changes how the tool fundamentally works belongs in an [ADR](../adr/README.md). Copy the shape of an
existing one; they are short by design.

Add one when the decision is hard to reverse, when there was a real alternative, or when the next person will
otherwise ask "why on earth is it done this way?".

## Things that will be declined

Not because they are bad ideas, but because they are contrary to what this project is:

**Custom merge or reconciliation logic.** Git does this. See [ADR-002](../adr/002-no-custom-reconciliation.md).

**A second template engine.** MiniJinja is the only one. [ADR-003](../adr/003-minijinja-only.md).

**Code execution from a `render`, `init` or `update`.** Hooks, scripts, subprocesses, embedded interpreters.
Templates are untrusted input, and a rendered project is never a place code runs from — unconditionally, still.
[ADR-016](../adr/016-template-tests-are-data.md).
A `git tpl test` case's `[commands]` is not an exception to this: it is a different, narrower capability entirely
— a test author's own harness running their own declared checks against their own template, never reachable from
a render — added by [ADR-027](../adr/027-test-case-commands.md), which supersedes only the clause of ADR-016 that
closed that one door.

**Post-render tasks**, reviewed under issue #32 and declined. The proposal was narrow — a confirmed, `init`-only
command list, run after the merge, leaving rendering untouched — and it still does not pay for itself. Of the five
commands real templates run after a first render, `git init` and `git add` are already done by `git tpl init`, the
two installs can never be run at all, and only `git remote add` was left. A trust model and a confirmation prompt
to serve one command, while every template needing an install ships `scripts/bootstrap.sh` anyway, is a mechanism
buying nothing. What the surveyed templates actually wanted — a way to *tell* the user about `bootstrap.sh`, and a
declared `origin` — is [ADR-019](../adr/019-templates-address-never-act.md): a template may address the user and
declare Git remotes, and still runs nothing.

**A matrix language for test cases.** Three files beat a combinatorial block whose expansion nobody can predict. If
a template needs twelve cases, twelve files say so honestly.

**Runtime values in the render context.** `now()`, `git.user`, environment access.
[ADR-006](../adr/006-no-runtime-context.md).

**Automatic pushing or fetching of template refs.** They are explicit by design.

**Copier or Cruft compatibility.** A useful reference, not a goal.

If you think one of these is wrong, open an issue and make the case. A change of mind belongs in an ADR that
supersedes the old one.
