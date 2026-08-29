# ADR-029: A test case's `[answers]` is always validated strictly

**Status:** accepted

**Relates to:** [ADR-016](016-template-tests-are-data.md), [ADR-028](028-test-case-trust.md)

## Context

`render`, `init`, `update`, `context`, `diff --dirty` and `show --dirty` all reject a supplied answer that names no
question in the template — but only under `--strict-answers`. Left off, the key is ignored rather than fatal, and
the caller is only ever told about it in passing. That leniency exists for a *recorded* answer set: a template
drops a question over time, and a project whose answers file still names it is not at fault for that. Erroring
there would make `--answers-from` useless for the exact case that motivated it.

`git tpl test` renders every case through `ops::render_resolved`, the same function `render` and the rest use, and
that function always computes which supplied keys matched no question. But nothing in `git tpl test` ever looked at
that list. `TestArgs` has no `AnswerArgs` and no `--strict-answers` — deliberately, per `docs/usage/test.md`'s "There
is no `--answer`, `--answers-from` or `--defaults`" — because a case's `[answers]` already *is* the answer set,
with no separate flag able to change what it asserts. The consequence, reported as issue #135, is that a typo'd key
in a case's `[answers]` passes silently: `git tpl test` has no path today to catch its own mistake, unlike the same
typo made in a `render --strict-answers` invocation against the same template.

A case's `[answers]` is not a recorded artifact surviving a template it did not keep up with. It is hand-authored,
lives in the template repository next to the manifest it describes, and its only job is to say precisely what a
rendering asserts. If it names a question that does not exist — because the question was renamed, or because of a
plain typo — the case itself is stale or wrong, and that is exactly the class of mistake a test suite exists to
catch, not the class of mistake it should quietly tolerate.

## Decision

`git tpl test` calls `ops::enforce_strict_answers` on every case's render, with `strict` fixed to `true` — no CLI
flag, no way to pass `false`. A key in `[answers]` that matches no question in the template fails the case with
`tpl::answers::unknown_key`, classified against `expect.error` exactly the way a render that failed outright
already is: there is no rendering for `expect` or a snapshot to check either way.

Unlike `trust` (ADR-028), this is not a per-case, defaultable key added to the case schema. `trust` needed one
because ADR-028 could name a real scenario for each of its two outcomes — a case proving the refused path
deterministically is exactly as legitimate as one exercising the real fetch. No such scenario exists here: there is
no case whose point is to *demonstrate* that one of its own answer keys is silently ignored, only ever the
inconvenience of it not being ignored. Absent that argument, adding an opt-out key would only give a typo a second
way to hide — this time behind `strict_answers = false` instead of behind the leniency itself.

## Consequences

**What stays closed.** `render`, `init`, `update`, `context`, `diff` and `show` are unaffected: `--strict-answers`
keeps its existing, opt-in meaning for a supplied or recorded answer set. This ADR narrows only `git tpl test`,
which had no equivalent flag to begin with.

**What changes.** A case whose `[answers]` already names an unknown key — a typo, or a question renamed after the
case was last touched — starts failing where it previously passed. That is the fix, not a side effect of it: an
existing template's suite that relied on the old silence will need its case files corrected, once, the same way any
newly-caught bug in a test suite requires fixing the case rather than the check.

**Cost.** No new case-schema key, no new CLI surface, and no new diagnostic code: `tpl::answers::unknown_key`
already existed for `--strict-answers`, and this is a second, unconditional caller of the same check.
