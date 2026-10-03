# ADR-037: hunks can be named in advance

**Status:** accepted

**Relates to:** [ADR-020](020-backport-is-a-patch.md), [ADR-022](022-backport-unsubstitutes.md),
[ADR-023](023-hunk-selection-precedes-the-proof.md)

Amends ADR-023's "`-p` without a terminal is refused, not ignored".
`-p` is still refused there, for the same reason.
This ADR adds the request that can be made there instead.

## Context

ADR-023 gave `backport` hunk selection, and made it interactive only.
Under `--json`, in a pipe, or with `tpl.interactive false`, `-p` fails with `tpl::backport::not_interactive`, and
the one non-interactive way to narrow a backport is a pathspec — which selects whole files.

That leaves a caller with no terminal — a script, CI, an agent working on someone's behalf — unable to send one
change out of a file that holds two.
It is a real gap, because the unit that belongs upstream is a *change*, and one change is often a few hunks of one
file, or one hunk each of several.
The alternatives the caller is left with are all bad:

- Send the whole file, and ship a change nobody decided to send.
- Edit the emitted mailbox by hand, filtering hunks out of a patch that ADR-020's proof was made against.
  The proof no longer describes what is sent, which is the exact failure ADR-023 exists to prevent.
- Stage the change as a separate commit and diff against it, which `backport` does not read: it measures the
  working tree.

The decision `-p` could not take for the user cannot be taken by git-tpl for a script either.
But a script *can* make it, if it can name what it is deciding about.

## Decision

**A hunk can be named, so that the choice is made before the command runs.**

- `--list-hunks` produces no patch.
  It reports every file's hunks — the same ones `-p` would offer — each with an `id`.
  It needs no terminal, and works under `--json`, where `result` is `plan`.
- `--hunk <path>:<id>`, repeatable, sends exactly those hunks.
  It is a [`Picker`](023-hunk-selection-precedes-the-proof.md) answered in advance: the selection still precedes the
  proof, so ADR-023's guarantee reads the same — *the patched source renders to your file with only the chosen
  hunks*.
- `-p`, `--list-hunks` and `--hunk` are mutually exclusive.
  `--list-hunks` also excludes `-o`, since it writes no patch.

### An unnamed file sends nothing

Once any `--hunk` is given, a file with no `--hunk` of its own contributes nothing.
This is the opposite of `-p`, which starts with everything selected, and the asymmetry is the same one ADR-023
draws.
`-p` starts from "everything" because a person is there to take things out.
With nobody there, "everything I did not mention" is the reading that ships a change the caller never saw.
Pathspecs and `--exclude` still apply first, and decide which files are considered at all.

### The id is derived from content

An id is the first 12 hex digits of a SHA-256 over the file's path, the hunk's `@@` header and its lines.
It is deterministic (invariant 2): it hashes text the cut already produced, and nothing else.

A position would have been simpler, and is wrong.
A script that lists hunks and selects one later would silently get a *different* hunk if the file had been edited
in between — hunk 2 is only "that" hunk while nothing before it has changed.
A content-derived id fails the other way: an edited file has different ids, so the old one matches nothing and the
command stops.
The path is hashed too, so the same change in two files does not share an id.

The id is opaque.
Callers copy it from the listing; the documentation does not describe how to build one, because a caller that
could would stop checking.

### A name that matches nothing is refused

`tpl::backport::unknown_hunk` covers a malformed `--hunk` and a well-formed one that no hunk answered to, and it
is raised even when every other name matched.
Skipping it would send a patch that silently lacks the change the caller asked for, and the likeliest cause —
the file changed since the listing — is exactly when the caller most needs to know.

### The listing is produced before the proof

A listing does not transpose, un-substitute or round-trip anything.
A file that a real backport would refuse is therefore still listed, and naming one of its hunks produces the
refusal, attributed to that hunk by the existing `hunk_refused`.
The alternative — listing only hunks known to be carriable — would need the proof run per hunk, which is both
expensive and not the same question: whether a hunk can be carried depends on which others are chosen with it.
A binary file is reported under `skipped` rather than refused, so that one cannot hide every hunk that can be
named.

## Consequences

- Two new flags, `--list-hunks` and `--hunk`, and one new diagnostic code, `tpl::backport::unknown_hunk`.
  `not_interactive` and `hunk_refused` keep their codes and meaning; only their `help` text changes, to point at
  the new flags.
- A new `--json` result value, `plan`, and a `plan` array in that payload only.
  The payload of every other `backport` is unchanged.
- `Picking` gains two variants, `Named` and `List`, and `ops` exports `HunkSelection`.
  The `Picker` trait is unchanged.
- No new capability is needed from Git, and nothing writes or spawns: invariants 1 and 5 are unaffected.
- Accepted cost: an id stops matching on *any* edit to its hunk or the file's path, including a harmless one.
  The remedy is listing again, which is cheap, and the failure is loud.
- Accepted cost: the hunks are still the rendered → project hunks of ADR-023, not the hunks of the emitted patch.
  A caller cannot name a hunk of the template source.
