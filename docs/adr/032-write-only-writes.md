# ADR-032: `--write` only writes; it does not run a case

**Status:** accepted

**Supersedes:** the "it never suppresses an `expect` failure" clause of [ADR-016](016-template-tests-are-data.md).

**Narrows:** [ADR-027](027-test-case-commands.md)'s "commands run by default" to a run that is not `--write`.

## Context

`--write` was meant to be the fast half of the snapshot loop: change a template, re-record what it now produces,
read the `git diff` of the recording. ADR-016 modelled this on `insta`, which is exactly that split —
`cargo insta review` records, `cargo test` proves — but `--write` itself never actually was the fast half. It
rendered a case, ran every one of its `[commands]` (ADR-027) — `before`, `rendered`, `after`, `finally`, any one of
which might build a virtualenv or run an installer — and checked every `expect.files`/`absent`/`contains`/`lacks`
against the result, exactly as a plain `git tpl test` does, before it ever got to the one thing `--write` exists
for. Recording a changed template cost the same as proving the whole suite still holds, for every case, every time.

That cost was deliberate: ADR-016 says outright that `--write` "never suppresses an `expect` failure — it records a
rendering, it does not bless a broken one." The reasoning was sound on its own terms, but it answered a question
nobody needed `--write` to answer twice. A case's `expect` and `[commands]` still get checked — by the plain
`git tpl test` a person runs right after recording, or that CI runs on every push regardless. `--write` blessing a
broken case was never actually possible: the very next `git tpl test` still fails it, snapshot or no snapshot.
What ADR-016's wording bought was not safety, only a slower `--write`.

## Decision

Under `--write`, a case that declares `snapshot = true` is rendered, and only rendered. None of its `[commands]`
run — not `before`, not `rendered`, not `after`, not `finally` — and `expect.files`/`absent`/`contains`/`lacks` are
never checked. The one thing that can still fail such a case is the render itself failing outright: nothing was
produced, so there is nothing to record, and that failure is reported exactly as it always was — it is not an
`expect` assertion, it is why `--write` has nothing to do.

A case that does not declare `snapshot = true` is not touched at all under `--write`: not rendered, not checked,
not counted as run. `--write`'s only reason to look at a case is to write its snapshot, and a case that never asked
for one has none to write. This is reported as its own outcome, `skipped`, distinct from the `none` a plain run
already reports for the same case — `none` means "checked, and it has no snapshot"; `skipped` means "`--write`
never looked."

A case naming `expect.error` needs no special case here: it is already forbidden to also declare `snapshot = true`
(`testing.rs`'s case-shape check — a render that is expected to fail has nothing for a snapshot to record), so it
always falls into the row above and is skipped under `--write` like any other case without one.

### Why this does not reopen what ADR-016 closed

ADR-016's "never suppresses" was never the only thing standing between a broken case and a green suite — a plain
`git tpl test` was always going to run again, in CI if nowhere else, and it still checks everything `--write` no
longer does. What changes is where that check happens, not whether it happens: the workflow becomes two steps,
`git tpl test --write` to capture what a template now produces, `git tpl test` to prove it still holds — the same
two steps `insta` always had, and the split ADR-016 cited as the model without actually taking it.

### Verbose output shows what changed, not commands that no longer run

With `[commands]` and `expect` gone from a `--write` run, `-v` has nothing left to stream live — there are no
commands producing stdout or stderr to forward. In their place, the report shows the same unified diff, coloured
the same way (`patch_line`), that a normal run's `Failure::SnapshotDiff` already shows for a case whose rendering
no longer matches what is recorded — the difference is only that here it is not a failure, it is what `--write`
just recorded over. It appears once per case whose snapshot changed, not for one that was freshly written (nothing
to diff against) or found unchanged (nothing to show).

The summary line splits `written`, `updated` and `unchanged` instead of the one "recorded" total ADR-016's
original report used — a reviewer treats a brand-new snapshot differently from a changed one, and `-v` needs the
two kept apart to say which case is in which. A case `--write` skipped entirely is counted too, so a suite that is
mostly `expect`-only cases does not read as if `--write` silently did nothing for most of it.

## Consequences

`--write` no longer proves, in the same invocation, that a case's `[commands]` or `expect` still hold for the
rendering it just recorded. A template author who wants both runs `git tpl test --write` followed by
`git tpl test`; a CI pipeline that only ever runs the latter is unaffected either way, since it never passed
`--write` to begin with.

`tests/test_commands.rs::a_case_with_commands_can_also_record_a_snapshot` and
`tests/test_command.rs::write_still_fails_a_case_whose_expectations_are_unmet` asserted the behaviour this ADR
reverses; both are rewritten to assert the new one. `docs/usage/test.md` is updated throughout — the Options
table, the Commands section, the Progress section and the Snapshots section all described the old behaviour as
deliberate and now describe this one.

The on-disk snapshot format (ADR-016) and the case schema (ADR-016, ADR-027) are unchanged: this ADR narrows when
`[commands]` and `expect` run, not what a case may declare or what a snapshot looks like on disk.
