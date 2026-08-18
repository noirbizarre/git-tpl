# ADR-022: un-substitution is proved per line, and confirmed by a human

**Status:** accepted

**Relates to:** [ADR-020](020-backport-is-a-patch.md),
[ADR-003](003-minijinja-only.md)

Amends ADR-020's final paragraph, which forecast a different implementation of
this extension. Everything else in ADR-020 stands.

## Context

ADR-020 shipped a `backport` that never invents a `{{ }}`. A change is carried
only where every line it touches is byte-identical between the template source
and the rendered output, and everything else is refused with
`tpl::backport::substituted_region`. That covers most prose and most
configuration, and it is the commonest refusal by a wide margin — the fix a
user most wants to send upstream is very often on a line that also holds a
placeholder.

ADR-020 forecast lifting this by taking its own rejected option 1 — a
substitution table with a refusal list — and putting it *in front of* the
round-trip check rather than instead of it. That is available, and it is not
what was built.

The reason is that a table cannot be made to work even as a filter. Its input
is a set of values and its question is "does this text come from that value?",
which at the level of bytes has no answer:

```jinja
Written by {{ author }} in June.
```

With `author = "June"`, the rendered line is `Written by June in June.` Neither
occurrence is distinguishable from the other by any rule over the bytes, so a
table must either reverse both — shipping `{{ author }} in {{ author }}` — or
refuse the line outright. Every refinement of the refusal list is a refinement
of *when to give up*, not of the answer.

## Decision

**Provenance is established by re-rendering the source, not by searching the
output.** For a source line `S` that produced rendered line `R`:

1. Scan `S` into alternating literal and `{{ … }}` spans.
2. Render each expression span on its own.
3. Require `literal₀ + value₁ + literal₁ + … == R`, byte for byte.

Step 3 is a proof by the same determinism (invariant 2) ADR-020 leans on, and
what it proves is an exact byte-range provenance: which bytes of the output the
render copied, and which an expression produced. There is no searching, so
there is no coincidence to be fooled by. In the example above, `{{ author }}`
occupies bytes 11–15 and the word "June" at bytes 22–26 is literal text, and
the two are simply different ranges.

An edit is then attributed by diffing `R` against the project's line and
requiring each hunk to fall inside a literal range. An edit touching a produced
range is a change to an *answer*, and is refused.

This is close to ADR-020's rejected option 2 — track provenance through the
renderer — reached without its cost. Option 2 was rejected because instrumenting
MiniJinja means forking it, or running a shadow evaluator beside it, against
ADR-003. Re-rendering one line's expressions needs neither: it is the same
`render_string_with` every other part of the tree calls, and the proof comes
from comparing its output rather than from watching it work.

**Every reversal is confirmed by the person who made the edit.** This is the
second decision, and it is not a courtesy.

ADR-020's round trip proves the patched source produces *this user's* file.
Un-substitution is the first thing in the command for which that is not the
same as being right for everyone:

```text
source    version = "{{ version }}"      with version = "1.0"
rendered  version = "1.0"
project   version = "1.0.0"
```

The inserted `.0` sits against the value, and attributing it to the literal
gives `version = "{{ version }}.0"`, which round-trips perfectly and appends
`.0` to every downstream project's version. The user meant to change their
answer.

Two things stand between that and a shipped patch. The first is mechanical: a
hunk is slid as far as it will go in both directions before it is placed, and a
hunk that *could* have been placed inside a value is treated as though it was.
`similar` returns one alignment out of several equally short ones, and which
one is an artefact of its tie-breaking rather than a statement about intent —
so the `.0` above is refused, because it could equally have been an edit to the
value. The second is the human: the class is not decidable from the bytes at
all, and only the person who made the edit knows which they meant.

So un-substitution happens only when someone is there to look at it. With
nobody to ask — `--json`, a script, no terminal, `tpl.interactive` off — it is
not attempted and the line refuses exactly as it did before. `--unsubstitute`
is that decision taken in advance, and is how a non-interactive caller opts in.

## Refusals

A refusal is free: the user is left editing the template by hand, which is
where they started. A wrong patch is not. So the guard list is long and every
entry is conservative.

| Refused | Because |
|---|---|
| `{% … %}`, `{# … #}` on the line | A block tag spans lines and a comment produces nothing, so neither has a byte range here |
| `{{- … }}` or `{{ … -}}` | Whitespace control reaches into the neighbouring line's bytes |
| An expression rendering to `""` | A zero-width range is invisible in the output, so step 3 cannot confirm the expression was ever evaluated here — and this is what closes the `{% for %}`-body case under lenient undefined, where an unbound loop variable renders to exactly the empty string |
| A value containing a newline | One source line produced several rendered ones, and the line model does not hold |
| Step 3 mismatch | Catches loop bodies, `{% raw %}`, and anything else whose provenance is not line-local |
| A line with no editable literal | Every change to it is a change to the value |
| Two source lines reproducing the same rendered line, or one reproducing two | Ambiguity, and a loop body, respectively |
| A hunk whose slide interval leaves its literal | The alignment was a coin-toss |
| A changed line terminator | A line-ending conversion, not a content edit |

## Consequences

`tpl::backport::substituted_region` stays, with the same code and a narrower
set of causes. No diagnostic code is added: an edit that introduces `{{` into a
literal still fails the round trip and reports `round_trip`, which is what that
code has always been for.

`--json` gains an `unsubstituted` array naming every reversed line, its
before-and-after and the placeholders it kept. A reviewer who cannot see which
lines were reversed cannot review them, and this is the one part of a backport
that must not be skimmed past.

`similar` gains its `unicode` feature. The attribution diff runs *within* a
line, where byte or character granularity would split a `\r\n` or a combining
sequence and hand back a range that is not a character boundary.

`GitBackend` still gains no method, and nothing here reads the filesystem, the
clock or the network. The only new work is rendering a handful of one-line
templates against a context that is already resolved, which is bounded by the
number of lines a user changed.

The accepted cost is that a `Replace` mixing verbatim and substituted lines is
still refused whole. Reversing part of it and transposing the rest would need
an emission order with no test to pin it, and the case already refused before
this ADR — so nothing regresses, and the improvement is simply not universal.
