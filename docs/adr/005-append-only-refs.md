# ADR-005: Template refs are append-only

**Status:** accepted

## Context

`git tpl update` re-renders and needs to record the result. It could:

1. commit with the current ref tip as parent — always
2. amend the tip when the change is "not a real update" (an answer changed, a
   `--dirty` render, a mistake)
3. reset the ref and start over when it has diverged

Options 2 and 3 are tempting because they keep the ref's history tidy.

## Decision

Always option 1. `update` commits with the current tip as parent, whatever the
reason for re-rendering. Never amend, never rebase, never force-update.

If the rendered tree is byte-identical to the tip, no commit is made at all.

`git tpl push` refuses to force, and there is no flag to make it.

## Consequences

Every rendering that was ever merged remains an ancestor of the user's branch, so
it remains a valid merge base. This is the property the whole model depends on:
rewriting a rendering that someone has already merged from destroys the base
their next merge needs, and turns it into a whole-file conflict.

The reason for a re-render — template moved, answer changed, data changed —
becomes irrelevant to the mechanism. All four are the same event: the desired
state changed. One code path.

A locally diverged ref is not a problem to detect and repair. It is simply the
parent of the next commit.

The cost is a ref history that accumulates commits, including ones from `--dirty`
experiments. That history is cheap, informative, and never rewritten — which is
exactly what makes it trustworthy.

The determinism guarantee (ADR-006) is what keeps this from becoming noise:
identical inputs make no commit, so the ref grows only when something real
changed.
