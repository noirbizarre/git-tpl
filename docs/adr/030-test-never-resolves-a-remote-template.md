# ADR-030: `test` never resolves a remote template, dirty by default

**Status:** accepted

**Supersedes:** the "Cases are read from the resolved revision" section of [ADR-016](016-template-tests-are-data.md),
for `--ref`/`--dirty` semantics on `git tpl test` only.

**Relates to:** [ADR-016](016-template-tests-are-data.md)

## Context

Every other command that resolves a template — `render`, `init`, `update`, `lint`, `questions`, `context` — needs an
explicit `--dirty` because there are two legitimate answers to "which version of the template": the committed ref,
which is what a teammate, a CI run, or `update` on someone else's machine would see, and the working tree, which is
only this machine's. Neither answer is obviously right, so the flag exists and the command picks the conservative
one — the committed ref — when neither is asked for.

`git tpl test` has no such tension. Nobody else consumes its output; there is no teammate for whom "the committed
ref" is the more honest answer. The whole reason to run it is to catch a broken conditional before committing it,
and requiring `--dirty` to see your own uncommitted edit had that backwards: the default answer, `HEAD`, was
whichever version you had *already decided was fine*, and the version you were actually iterating on needed an
extra flag to be seen at all.

`--root` — override the manifest's declared render subdirectory — is a different concept, the same on `test` as on
`render`/`lint`/every other command. A case exercises the template's own declared root; a hypothetical alternate
root is a question for `render --root`, not something a test suite needs an opinion on.

`TEMPLATE`, unlike `--root` and `--dirty`, is worth keeping: a monorepo script or an editor task may not have `cd`ed
into the template directory before invoking `test`, and naming it directly is the natural way to ask for it
without one. But letting it name a *remote* source reopens the same confusion `--dirty`'s removal fixed: a remote
clone has no working tree, so a remote `TEMPLATE` would need `--ref` to mean anything at all, and `test` would be
back to a flag whose requirement depends on what else was typed. Worse, testing a remote template is not this
command's job in the first place — there is no CI story for "clone this repository nobody asked to check out, then
test it"; the person doing that has, definitionally, already chosen to have it locally.

`test` is also the one command with *two* things a positional could mean: which template, and which cases to run.
Every other command has only the former, so `TEMPLATE` being a positional is unambiguous there. On `test`, a
positional `TEMPLATE` made `git tpl test minimal` genuinely ambiguous — "the case named `minimal`, against the
default template" and "the template at `./minimal`, every case" are both plausible readings, and clap has to pick
one (it picked the latter, silently). That is worse than the `--dirty` confusion this ADR already fixes: there,
the flag existed and its default was merely wrong; here, there was no way at all to filter cases without also
naming a template.

## Decision

`git tpl test` drops `--root` and `--dirty`. With no `--ref`, it reads the working tree of whatever `--template`
names; `--ref BRANCH|TAG|SHA` pins it to a committed revision instead:

```sh
git tpl test                              # the working tree of `.`, right now
git tpl test --template ./templates/foo   # the working tree of that checkout instead
git tpl test --ref v1.2.0                 # `.`, at that tag, committed
git tpl test minimal                      # case `minimal`, against `.` — never ambiguous with a template path
```

`--template` keeps the old positional's default (`.`), but not its shape: it is a flag, specifically so `CASE` stays
the only positional and a bare case name is never read as a template path instead. It is also restricted: a remote
source (a URL) is refused unconditionally, **even with `--ref`**. There is no committed-revision story for `test`
the way there is for `render` — checked once, up front, before anything else, so `--write`, `--ref` and the
implicit dirty read all fail the same way rather than three rules that could disagree about what "supported" means.

Nothing about how cases or snapshots are read changes otherwise: the resolver already builds a synthetic tree of the
working directory when there is no committed revision to prefer, exactly as `--dirty` did on every other command,
and `discover`/`read_snapshot` need no special case for it.

## Consequences

**`--write`'s locality guard is superseded by a broader one.** `--write` used to refuse a template with no working
tree (`tpl::testing::write_needs_local`) on its own. That check is replaced by an unconditional one — checked
regardless of `--write` — because a remote `--template` is refused outright now, not only when something tries to
write to it. `TestError::WriteNeedsLocal` becomes `TestError::RemoteNotSupported`
(`tpl::testing::remote_not_supported`).

**A breaking CLI change.** `git tpl test <url>` now fails immediately with `tpl::testing::remote_not_supported`
instead of cloning the remote's default branch (or a named `--ref`) and testing that. Clone it locally first.
`git tpl test some/path` (a bare positional, the old shape) now fails as an unexpected argument — use
`--template some/path` instead. `git tpl test --dirty` and `git tpl test --root <path>` fail as unknown flags. None
of this touches the case schema or the snapshot format — both are ADR-016's on-disk contracts and are untouched.

**What stays closed.** `render --dirty`, `render --root`, `init --dirty`, and the rest keep their existing meaning
and default. This ADR narrows `test` alone, for the reason above: it is the one command with no second party for
whom the committed ref is the more honest default, and the only one with no reason to ever resolve a remote source.
