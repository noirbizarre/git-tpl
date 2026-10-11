# ADR-038: Compose revision-tracked sibling templates as independent module trees

**Status:** Accepted

## Context

ADR-034's `[extends]` is one parent whose files and questions can be overridden by a child.
It cannot render two independent sibling repositories repeatedly into one project.
Module projects need this while keeping their source and integration skeletons independently maintained.

## Decision

An optional `[[compose]]` declaration on a parent template names a child Git repository,
a branch, tag or commit ref and a parent list question holding module IDs.
An empty answer reads no child repository.
Each ID renders the child from that same resolved commit with its own `module_name` and `module_path` answers.
The child may receive `version` and `memcastle` only when it declares those questions.
No other question, data source or partial leaks across siblings.
Duplicate IDs, invalid IDs, missing or non-list answers and rendered path collisions are refusals.

The parent records the list; its manifest names child refs, while project config and rendered provenance record
the exact child commits resolved at init or update.
Historical backport uses those recorded commits even after a branch moves.
Changing a selected list later runs the same append-only `update` and ordinary Git merge as any other answer change;
no second attachment, executable post-render hook or custom merge strategy is introduced.
A remote child requires its own explicit trust decision before cloning,
as an `[extends]` ancestor already does.
Nested composition is refused so the graph of fetches stays bounded and understandable.

A parent's files can iterate its typed list without executing a script during rendering.
The rendered tree remains deterministic for the recorded answers and child commits.

## Alternatives

Copying child file trees into the parent template would leave three skeletons to maintain.
Multiple independent `git tpl init` attachments would make updates and merge ancestry ambiguous.
A post-render shell hook would execute untrusted template code and violate ADR-019.
