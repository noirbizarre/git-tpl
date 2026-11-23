# ADR-054: Backport routes separate mailboxes to historical source owners

**Status:** Accepted

## Context

Inherited and composed files can share source paths with the attached leaf while belonging to another repository.
Rejecting them avoids a wrong upstream patch but forces users to reconstruct ownership manually.
A composed file can also come from the child's ancestor, and one upstream source may render into several modules.
Equal rendered bytes alone establish neither historical source identity nor a correct source-level patch.

## Decision

Extend ADR-020's patch-only workflow with explicit owner routing.
Retain an opt-in, in-memory trace during historical reconstruction:
actual repository/source revision/root/bytes, plus the effective rendering environment for each output instance.
Capture this information before composition flattens file origins.
An inherited source uses the effective leaf environment; a child source uses that particular module's environment.
Retain owned snapshots instead of reopening mutable refs or introducing self-referential repository borrows.

Continue requiring the complete reconstructed tree to equal the rendered ref tip.
Also validate historical sources, ancestor commits and composed locks.
New top-level ancestor trailers contain full commits.
New child trailers carry versioned ancestry evidence, distinguishing an empty chain from absent legacy evidence.
Accept legacy child ancestry only when its manifest chain establishes full immutable commit pins.
Refuse moved ancestor pins and insufficient or inconsistent provenance rather than choosing a plausible current source.
Do not create a new rendering solely to upgrade missing legacy evidence.

Run the existing selection, transposition, reversal and round-trip proofs using each file's retained inputs.
Deduplicate identical edits to one source, refuse conflicting proposals,
and require every unselected instance consuming that source to preserve its baseline bytes.
Do not implement custom reconciliation of competing source edits.
Refuse combining different base revisions of one repository.

One owner keeps the existing mailbox workflow.
Several owners require `--output-dir` or structured JSON, with separate mailboxes and explicit source/base identities.
Never concatenate different owners' mailboxes into one pipeable stream.
Directory publication requires a new destination, stages validated artifacts first,
and publishes the manifest last as a completion marker, cleaning ordinary failures.
The manifest is versioned and maps numbered mailbox files to owners, full base commits and source/output paths.
Apply hints target user-owned checkouts and individual artifacts, never acquisition caches.

## Consequences

Backport still never applies a patch or changes a source checkout, project HEAD, index, configuration or ref.
Historical rendering and worktree snapshots may write project Git objects.
New provenance is additive; readers retain support for abbreviated ancestors and legacy child records.
Some older ambiguous histories must be handled directly upstream or gain evidence in a subsequent real rendering.
The proof covers recorded answer environments, not all future projects or arbitrary changes to imported partials.
Symlink, binary, deletion and executable-mode limitations remain unchanged.
