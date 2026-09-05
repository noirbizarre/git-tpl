# ADR-036: `--shard` and `--record-durations` balance a test suite across a CI matrix

**Status:** accepted

**Narrows:** [ADR-032](032-write-only-writes.md)'s "a write-shaped flag only writes" to a second flag,
`--record-durations`.

**Relates to:** [ADR-016](016-template-tests-are-data.md) (the on-disk convention this reuses),
[ADR-035](035-github-actions-progress-reporter.md).

## Context

A template with enough cases to be worth testing eventually has enough cases to be worth splitting across a CI
matrix: a suite with a `[commands]`-heavy case or two can take minutes, and a matrix of four runners finishing in
a quarter the time is worth the extra YAML. Doing that by hand — four hand-picked lists of case names, one per
matrix job — is exactly the maintenance burden `git tpl test`'s "three files beat a combinatorial block"
(`docs/usage/test.md`) already refuses for a template's own cases; the same refusal applies to how a caller splits
them.

An even split by count is trivial but wrong the moment one case is a build-and-install and another is a bare
template render: four runners with three cases each does not mean four runners finishing at the same time.
`pytest-split` solved this by recording how long each test took on a real run (`--store-durations`) and using that
history to bin-pack a later `--splits N --group G` into balanced groups (`least_duration`: sort descending by
known duration, greedily assign each to the currently lightest group, unknown tests at the mean of the known
ones). There is no reason to invent a different algorithm for the same problem.

## Decision

Two new flags on `git tpl test`, mutually exclusive with each other:

**`--shard INDEX/TOTAL`** (both 1-based) selects only the cases assigned to this shard, applied *after* `discover`
and the existing positional case-name filter — a shard is "of whatever this invocation would otherwise run," not
a separate universe. Without a recorded durations file, the split is a deterministic slice by count, in
`discover`'s own name order: the first `len / TOTAL` cases (plus one each for the first `len % TOTAL` shards) go
to shard 1, and so on. **This never errors merely because no durations exist yet** — a template's first `--shard`
run, before anyone has run `--record-durations`, must work exactly as well as its hundredth. With one, the split is
balanced by the `least_duration` algorithm above, weighting an unrecorded case at the mean of the recorded ones
(or equal weight if none are recorded at all).

**`--record-durations`** runs the suite exactly as a plain `git tpl test` would — full render, `[commands]`,
snapshot compare, respecting every other flag — while timing each case's wall-clock duration, then writes
`tests/__timings__/durations`, merged over whatever was already recorded (a run filtered to one case name must not
erase every other case's history). This file is meant to be committed to the template's own repository, the same
way `pytest-split`'s `.test_durations` is, and read back by a later `--shard` run.

`--record-durations` conflicts with `--shard` (recording from a partial, sharded run would corrupt the file with
timings for only some of the suite) and with `--write` (ADR-032: `--write` does not run a case at all, so there is
nothing real to time — timing it would either time nothing, or silently start running cases `--write` was written
specifically not to run).

### The durations file reuses the snapshot convention

`tests/__timings__/durations` sits nested under a `/`, exactly like `tests/__snapshots__/…` — the same mechanism
that already excludes a snapshot directory from being mistaken for a case file (`discover`'s top-level-only scan)
excludes this file too, for free. Plain text, one `name millisecond-count` line per case, sorted by name, a
versioned first line (`# git-tpl durations 1`) mirroring `MANIFEST`'s own `# git-tpl snapshot 1` — the same
forward-compatibility reasoning ADR-016 already gives that line.

### Duration is always measured, never only under `--record-durations`

Every `CaseOutcome` carries `duration_ms`, whether or not `--record-durations`/`--shard` was passed. Two
`Instant::now()` calls per case cost nothing worth gating behind a flag, and a plain run's `--json` becomes more
useful for it — a caller can already see which case is slow without opting into anything. It is measured once, by
`run`'s own loop around the single `run_case` call, rather than threaded through `run_case`'s two dispatch paths
and three return points.

### A shard that resolves to zero cases is refused

The same reasoning `tpl::testing::no_such_case` already gives a mistyped case filter: a `--shard` whose `TOTAL`
outgrew the suite (a matrix reconfigured to more runners than the template has cases, say) must not exit `0`
having silently tested nothing. `tpl::testing::empty_shard` refuses it, naming how many cases exist so the mistake
is obvious.

### The GitHub Actions matrix recipe

```yaml
strategy:
  matrix:
    shard: [1, 2, 3, 4]
steps:
  - run: git tpl test --shard ${{ matrix.shard }}/${{ strategy.job-total }}
```

`strategy.job-total` supplies `TOTAL` directly from the matrix's own length, so the command line has exactly one
dynamic part (`matrix.shard`) rather than two values that could drift apart if the matrix were resized without
also updating a hand-written `TOTAL`. No matrix-generation support is needed in `git tpl test` itself — GitHub
Actions already exposes the two numbers a shard needs.

## Consequences

**`ops::testing::run`'s signature is refactored** into a `Target`/`RunOptions`/`&UserConfig`/`&mut dyn Progress`
shape, grouping `tests_dir`/`filter`/`write`/`run_commands`/`color`/`shard`/`record_durations` into one
`RunOptions` struct rather than growing an already-`#[allow(too_many_arguments)]` parameter list to ten, three of
them adjacent bare `bool`s. This mirrors the existing `Target`/`resolve::Request` pattern rather than introducing
a new one.

**New `TestingError` variants**: `tpl::testing::malformed_shard`, `tpl::testing::empty_shard`,
`tpl::testing::durations_read`, `tpl::testing::durations_write` — documented in
`docs/reference/diagnostics.md`.

**New `--json` fields**: `summary.durationsRecorded`, a top-level `shard` object (`null` unless `--shard` was
given), and a `durationMs` on every case. Renaming any of these is a breaking change, like every other `--json`
key.

**Nothing about ADR-035 depends on this**, and nothing here depends on it: a sharded run reports progress with
whichever `TestProgress` variant it would have anyway, GitHub Actions or not.
