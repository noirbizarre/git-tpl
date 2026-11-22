# ADR-050: Switching preserves rendered history; merging is an explicit second phase

## Status

Accepted.
Clarifies [ADR-001](001-rendered-ref-model.md): rendering operations remain isolated;
explicit command-layer merge composition can modify the working branch.

## Context

Changing a project's template must preserve useful answers without destroying the merge base.
Deriving a new rendered ref from the replacement source would ordinarily start unrelated history,
making Git unable to distinguish project customizations from the previous template's output.
Users also need an explicit way to request integration immediately after rendering.

## Decision

`switch <template>` retains the existing attachment ID and appends the replacement rendering to its ref.
It requires the existing rendered history rather than silently creating an orphan.
Inheritance detection uses the resolved ancestor chain and conservative source identity.
An inherited target starts from existing configuration; an unrelated target starts from fresh configuration.
Both reuse exact-name recorded answers only after full validation in the target's evaluation context.
Explicit answer inputs remain authoritative and invalid values fail normally.

Switch renders without cross-source migration discovery.
Its provenance establishes the target revision as the baseline for future ordinary updates.
Changed source identity requires a provenance-bearing commit even when rendered output is identical.
Resolved composition always supplies fresh target child locks.

Rendering remains isolated from HEAD, the index and rendered worktree files.
Switch saves configuration changes unstaged, as update already does for changed answers and locks.
`switch --merge` and `update --merge` opt into ordinary integration as a separate command-layer phase,
sharing the merge operation and presentation used by pull.
They require existing history, normal repository state, and a clean index and worktree before rendering.
The configuration edit produced by rendering is never implicitly staged for integration.

## Consequences

Git retains its merge base across both related and unrelated template transitions.
Ref identity may retain the old source's name; configuration explicitly records that ID.
No configuration schema change or custom merge algorithm is needed.
Incompatible recorded answers prompt or use defaults, while similarly named questions are never guessed.
Conservative source identity can classify remote URL aliases as unrelated.

Combined commands are two phases, not a transaction.
Merge failure or abort does not undo a successfully rendered ref or saved configuration.
Conflicts use ordinary Git recovery, and JSON reports separate rendering and merge outcomes.
