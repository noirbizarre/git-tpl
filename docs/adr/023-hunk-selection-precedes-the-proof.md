# ADR-023: hunk selection precedes the proof

**Status:** accepted

**Relates to:** [ADR-020](020-backport-is-a-patch.md), [ADR-022](022-backport-unsubstitutes.md)

Amends what ADR-020's round-trip check is taken against.
Everything else in ADR-020 and ADR-022 stands.

## Context

`git tpl backport` carries a project's whole local divergence, filtered only by pathspec and `--exclude`.
That is the wrong granularity often enough to matter: a working tree usually holds one change that belongs
upstream and several that do not, and they are frequently in the same file.
`git add -p` is the idiom every user already has for this, and `-p` on `backport` is the same request.

Shelling out to `git add -p` is not available.
Nothing in this tree spawns a subprocess — the same reasoning that keeps `git am` out of the command (ADR-002,
ADR-020) — so the hunks have to be cut, shown and reassembled in process.

The difficulty is not the picker.
It is *when* it runs.

ADR-020's guarantee is that the emitted patch, rendered, reproduces the user's file.
It is a proof rather than a check because rendering is deterministic (invariant 2).
But a change that round-trips as a whole does not necessarily round-trip with half its hunks dropped: the
alignment that placed it in the source was computed against the whole change, and un-substitution (ADR-022) is
established per line against a specific rendered text.
Take three hunks, prove them, then discard one, and what remains has been proved of a document that is no longer
being sent.

So the question is what a partial selection is proved *against*, and the two places the picker can go answer it
differently.

## Decision

**The selection is taken on the rendered → project diff, before anything is transposed, and the chosen hunks are
assembled into a partial project text that the existing pipeline then treats as the file.**

Concretely, per file, `rendered` being what the template produced and `project` what the user has:

1. Cut `rendered → project` into hunks and offer them.
2. Reassemble `project'` — the rendering, with only the chosen hunks applied.
3. Run ADR-020's transposition, ADR-022's un-substitution and the round-trip check against `project'`, exactly as
   they would have run against `project`.

Nothing in steps 3 changes.
`verify` is untouched, and its guarantee reads with one clause added: **the patched source renders to your file
with only the chosen hunks**.
`project'` is a real, well-defined document, so this is the same proof about a different — and correct — target.

Two properties make step 2 safe to put in front of the proof, and both are pinned by unit tests: choosing every
hunk reproduces `project` byte for byte, and choosing none reproduces `rendered`.
The second is what makes "the user deselected everything" indistinguishable from "this file has no change", which
is a case the command already handles.

### The rejected alternative

Transpose first, cut the *emitted patch* into hunks, and let the user pick from those.
It is superficially more honest — those are the hunks that ship — and it is wrong in two ways:

- There is nothing left to verify against. The round-trip check compares a rendering to `project`, and after
  dropping a hunk of the patch neither `project` nor any obvious derivation of it is the right comparand. One
  would have to invent the corresponding rendered-side text, which is step 2 above arrived at backwards.
- A file that refuses is refused before the user sees a hunk. Transposition is where `substituted_region` comes
  from, so the case `-p` most wants to serve — "one of my five hunks is on a placeholder line, send the other
  four" — would never reach the picker at all.

Selecting on the user's own edits also matches what they are being asked to judge.
The template patch is a document about a file they have not opened; the rendered → project diff is the change
they just made.

### A selection can be refused, and says which hunk

Because the proof now runs on the selection, a selection can fail where the whole change would have succeeded,
and the reverse.
`tpl::backport::hunk_refused` wraps the underlying refusal — which keeps its own code, message and help as a
`diagnostic_source` — and adds the hunk's number and header, so the next action is "run it again without hunk 2"
rather than "find line 47".

Attribution is exact and therefore partial: only a refusal that names a line can be placed in a hunk, and only a
chosen hunk can be the cause.
Anything else is returned unwrapped rather than wrapped in a guess.

### `-p` without a terminal is refused, not ignored

Under `--json`, without a tty, or with `tpl.interactive false`, `-p` fails with `tpl::backport::not_interactive`.

This is the opposite of `--unsubstitute`'s absence, which is silence, and the asymmetry is the point.
Un-substitution is something git-tpl *offers*; declining to offer it where nobody can answer is a decision it may
take on the user's behalf, and the result is the ADR-020 refusal they would have had anyway.
`-p` is something the user *typed*, and the one thing it cannot be taken to mean is "send everything".
A flag that silently became its own opposite on a CI runner is the worst version of this feature.

For the same reason, cancelling the picker aborts with `tpl::backport::cancelled` rather than being read as an
empty selection.
Escape is at least as likely to mean "wait, start again" as "send nothing", and emitting a patch on that reading
would ship a selection nobody approved.

## Consequences

- A new module, `src/ops/hunks.rs`, holding the hunk type, the cut and the reassembly. It declares no diagnostics
  of its own.
- A new library trait, `Picker`, beside `Prompter`, `TrustGate` and `Unsubstituter`, with its `demand`
  implementation in the binary. No `GitBackend` method is added: the patch is `backport`'s own, formatted in
  process, and libgit2 has no structured hunk to offer that would fit it.
- Three new diagnostic codes: `hunk_refused`, `cancelled`, `not_interactive`. No existing code changes meaning —
  under `-p` a refusal is wrapped, and `-p` cannot be combined with `--json`, so nothing a script sees is
  affected.
- Hunks carry three lines of context, matching the emitted patch. A picker that grouped changes differently from
  the patch it produces would be showing the user a different document from the one they are approving.
- Accepted cost: the hunks shown are not one-to-one with the hunks of the emitted patch. Transposition can merge
  or split them, and a file whose selection is refused says which of *its* hunks failed, not which of the
  patch's.
