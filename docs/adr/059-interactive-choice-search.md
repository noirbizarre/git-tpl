# ADR-059: Choice search is a presentation hint

## Status

Accepted.

## Context

Data-driven choice lists can be too large to navigate by scrolling.
Choisir provides typed selections with substring and fuzzy matching and arrow-enabled confirmations,
while dialoguer handles text input and console handles terminal output.
The initial local integration exposed API gaps before choisir's first release.

## Decision

Use choisir for selection widgets and confirmations, dialoguer for text input, and console for terminal primitives.
Keep these dependencies at the binary edge rather than leak widget types into evaluation or operations.
Use the published choisir 0.1.0 crate and its `MinimalTheme` for both choisir and dialoguer widgets.

Choice and multi-choice questions use substring typeahead by default.
Add an optional question-level `fuzzy = true` flag for noncontiguous matching.
It applies to both inline `choices` and `choices_from`, and is rejected when enabled on other question kinds.
Omitted and false are equivalent and do not need to be serialized back into manifests.
Question introspection reports the boolean explicitly.

Filtering never changes validation, answer identity, defaults, or noninteractive resolution.
Selections carry underlying values independently of labels, including duplicate labels.
Multi-selection preserves checked values across filtering and returns them in original input order.
Space toggles multi-choice selections rather than becoming part of the query.
Escape clears a query before cancelling; Ctrl-C cancels a selection immediately.
Consent and hunk menus initially keep search disabled.

## Consequences

This is an additive manifest extension and an interactive UX change, not a change to rendered-value semantics.
Existing templates and unattended answer sets remain valid.
Stderr must remain a terminal for interactive widgets, while stdout may carry JSON or mailbox data.
Automation uses explicit answers/defaults instead of a line-oriented stdin fallback.

Preserve full prompt help above selection widgets until choisir supports prompt-level descriptions.
Record verified gaps and dialoguer-specific cancellation blockers in the
[integration report](../development/choisir-integration.md), rather than adding unrelated widgets to choisir.
