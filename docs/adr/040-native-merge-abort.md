# ADR-040: Native Git owns explicit merge abort recovery

**Status:** Accepted

## Context

A hard reset discards unrelated staged and unstaged changes even when no merge is active.
Git's abort operation already implements merge recovery and refuses unsafe recovery.

## Decision

Only explicit `git tpl merge --abort` may invoke native `git merge --abort` through the Git backend.
Require active merge state before spawning, target the backend repository explicitly, and capture failures.
Never fall back to reset or clear recovery state after failure.
This narrowly amends ADR-019's process prohibition; it grants no capability to templates,
rendering, init or update.

## Consequences

Abort requires native Git on PATH and inherits Git's recovery semantics, including refusal when edits obstruct recovery.
Stable diagnostics distinguish no active merge from native abort failure.
