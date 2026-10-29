# ADR-039: Native Git owns diff presentation

**Status:** Accepted

Amends the process-execution restriction recorded in ADR-019 and ADR-027 for diff presentation alone.

## Context

`git tpl diff` computes the merge preview through the Git backend, then prints a libgit2-generated patch.
That bypasses Git's configured pager and diff presentation, including delta, colors, diff drivers and textconv.
A user who configured `git diff` should get that same display when inspecting a template update.
Reimplementing Git's pager precedence and diff configuration would create a second, inevitably divergent policy.

## Decision

The binary's `diff` command may spawn native `git diff` to display patches and `--stat` output.
The operation layer still computes the same HEAD-tree → merge-preview-tree endpoints, including conflict markers.
The subprocess receives only those concrete object IDs, presentation flags and path arguments.
It runs in the project repository with inherited standard streams, so Git detects the actual terminal and selects
its own pager using `GIT_PAGER`, `pager.diff`, `core.pager` and its ordinary fallback rules.
There is no special delta integration and no git-tpl-specific pager preference.

Native Git also owns diff drivers and textconv behavior, including executing the configured programs as ordinary
`git diff` would.
The command does not force-enable an external tool that Git would leave disabled.
An explicit `--color=always|never` overrides Git's patch/stat colors;
the default `auto` leaves Git's color configuration and terminal detection in control.
Native diffstat is emitted on stdout, replacing the previous custom stderr summary.

The JSON response and plain `--name-only` path list retain their existing backend implementations.
They never invoke native presentation or its tools.
The difference exit code remains a structured comparison of the trees, independent of conflict warnings or pager exit.
Quitting a pager normally is successful; failure to launch Git or an unsuccessful Git command is actionable.
Patch/stat presentation requires `git` on PATH rather than silently falling back to a different display.

This exception is implemented only at the command edge, after preview computation and any `--dirty` rendering finish.
It introduces no executable template declaration and changes no rendering capability.
`render`, `init` and `update` still cannot execute template code.
The preview does not change HEAD, refs, the index or worktree files;
as before, temporary preview objects may be written to the object database.
The existing test-harness exception remains separate.

## Alternatives

Coloring the libgit2 patch fixes only colors, leaving pager and diff configuration ignored.
Piping a captured patch to a chosen pager duplicates Git's precedence rules and omits its diff tooling.
Diffing HEAD against the rendered ref directly reports project-owned files as deletions that a merge would never make.
