# Contributing

## Before opening a PR

```sh
mise run ci
```

Runs formatting, Clippy, spelling, workflow linting, the tests and the docs
build — everything CI does.

## Commits

[Conventional Commits](https://www.conventionalcommits.org). Enforced by
commitlint through a `commit-msg` hook, and used by git-cliff to generate the
changelog, so the type you choose becomes a section heading in the release
notes.

```
feat(render): support conditional directories
fix(git): report the URL when SSH authentication fails
docs: explain why update never touches the worktree
```

Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `style`, `build`,
`ci`, `chore`, `revert`.

A breaking change gets a `!` or a `BREAKING CHANGE:` trailer. git-tpl writes to
Git refs and defines an on-disk template format; a break in either must be
visible in the version number.

## What a change needs

**Tests.** A behaviour without a test is a behaviour that will regress. A bug fix
needs a test that fails before it.

**Documentation.** A user-visible feature is not finished until the relevant page
describes it. Documentation is written alongside the implementation, in the same
PR — not afterwards, because afterwards does not arrive.

**A reason.** Comments that explain *why*, especially where the obvious approach
was rejected.

## Architecture decisions

Anything that changes how the tool fundamentally works belongs in an
[ADR](../adr/README.md). Copy the shape of an existing one; they are short by
design.

Add one when the decision is hard to reverse, when there was a real alternative,
or when the next person will otherwise ask "why on earth is it done this way?".

## Things that will be declined

Not because they are bad ideas, but because they are contrary to what this
project is:

**Custom merge or reconciliation logic.** Git does this. See
[ADR-002](../adr/002-no-custom-reconciliation.md).

**A second template engine.** MiniJinja is the only one.
[ADR-003](../adr/003-minijinja-only.md).

**Code execution from templates.** Hooks, scripts, subprocesses, embedded
interpreters. Templates are untrusted input. This includes a `command` key in a
`git tpl test` case, which is where the rule is most tempting to break —
[ADR-016](../adr/016-template-tests-are-data.md).

**A matrix language for test cases.** Three files beat a combinatorial block
whose expansion nobody can predict. If a template needs twelve cases, twelve
files say so honestly.

**Runtime values in the render context.** `now()`, `git.user`, environment
access. [ADR-006](../adr/006-no-runtime-context.md).

**Automatic pushing or fetching of template refs.** They are explicit by design.

**Copier or Cruft compatibility.** A useful reference, not a goal.

If you think one of these is wrong, open an issue and make the case. A change of
mind belongs in an ADR that supersedes the old one.
