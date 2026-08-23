# ADR-002: No custom merge or reconciliation

**Status:** accepted

## Context

Given ADR-001, we could still be tempted to "improve" the merge — smarter conflict resolution, template-aware
heuristics, automatic acceptance of changes in regions the user never touched.

## Decision

None of it. git-tpl implements no:

- three-way merge
- conflict resolution
- patch replay
- rename detection
- merge heuristics

Merging is libgit2's merge, which is Git's merge.
`git tpl merge` is a convenience wrapper that resolves the ref name and nothing else.

## Consequences

Conflicts look exactly like Git conflicts, in files, with markers, in an index you can inspect.
Every tool the user has works, and none of them needed to know git-tpl exists.

`git merge --abort` works, which means a user can always get out.
A bespoke resolver would need its own escape hatch, and it would be worse.

Behaviour is predictable in the strongest possible sense: it is behaviour the user already knows.

We inherit Git's improvements for free — the `ort` strategy, better rename detection, whatever comes next.

The cost is that we cannot be cleverer than Git in the cases where a template-aware tool theoretically could.
That trade is worth it.
Merge algorithms are where correctness bugs hide, and a bug there silently corrupts a user's source code.
