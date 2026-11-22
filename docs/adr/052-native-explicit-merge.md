# ADR-052: Native Git owns explicit template integration

**Status:** Accepted

## Context

libgit2 does not execute configured external merge drivers.
Repositories using `.gitattributes` with `merge=<name>` therefore had to bypass `git tpl merge`.
Reimplementing attribute selection, driver placeholders, recursive drivers or exit handling would violate ADR-002.

The existing backend merge also serves initialization, which must not execute template-provided code.
Changing that implementation for all callers would breach ADR-019's rendering and initialization boundary.

## Decision

Add a separate `GitBackend::merge_explicit` capability that delegates integration and commit creation to native Git.
Use it for `merge`, `pull`, `update --merge` and `switch --merge`, after rendering has completed.
Keep initialization's existing non-executing merge and in-memory previews on libgit2.

This narrowly amends ADR-019 and extends ADR-040's native-Git integration boundary beyond abort.
Explicit integration runs repository-configured merge drivers and normal Git hooks, filters and signing machinery.
Templates gain no command declarations or execution capability during rendering or initialization.

Target the backend repository and index explicitly, independent of the caller's directory or Git-selection environment.
Capture native output so drivers and hooks cannot corrupt JSON stdout.
Refuse active operations and staged user changes before spawning Git.
Preserve unrelated unstaged changes, including configuration edits from completed rendering.

Explicit merge options preserve fast-forward behavior, merge ancestry, ignored-file protection and commit control.
Disable autostash and squash even when branch merge options request them.
Do not select a merge strategy or approximate attribute handling.
Use the existing conventional message unless the user supplies another, without starting an editor.

Ordinary unresolved conflicts remain successful `Conflicted` outcomes with sorted paths and normal Git merge state.
Spawn, fatal driver and commit-finalization failures produce `tpl::git::merge_failed`.
Never reset, clear recovery state, undo rendering, or fall back to libgit2 after a native failure.
ADR-040's abort remains available when Git has established merge state.

## Consequences

Explicit integration requires native Git on PATH and honors external merge drivers with Git's actual inputs and results.
Hooks or signing can refuse a commit; inspect Git's captured reason and finish or abort the remaining merge.
Fast-forward integration does not create a merge commit, even with `--no-commit`.
Rendering stays deterministic and append-only; combined commands are still separate phases under ADR-050.

Previews do not execute merge drivers and can differ from explicit integration.
Initialization retains its non-executing merge behavior and does not support external drivers.
Plain `git merge refs/tpl/<id>` remains available for direct native integration.
