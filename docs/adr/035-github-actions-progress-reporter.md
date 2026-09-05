# ADR-035: A GitHub Actions progress reporter for `git tpl test`

**Status:** accepted

**Relates to:** [ADR-027](027-test-case-commands.md) (the verbose forwarding this mirrors),
[ADR-015](015-machine-readable-output.md) (why this must never touch `--json`).

## Context

`git tpl test`'s existing progress reporting (`TestProgress`) picks between a spinner, a scrolling log, and
silence, based on whether stderr is a terminal and whether `-v`/`--quiet`/`--json` were passed. None of the three
is right for a GitHub Actions log: a spinner's carriage-return redraws render as a wall of stale lines once
captured; the plain scrolling log (today's fallback for a non-terminal, which a runner's log is) shows every
case's own command output inline, all the time, with nothing to fold — a suite of forty cases becomes forty
screens to scroll past to find the one that failed.

GitHub Actions logs understand a small vocabulary of *workflow commands* — lines of the form `::group::title` /
`::endgroup::`, `::error file=…,title=…::message`, `::debug::message` — sent over the step's own stdout. A log
viewer folds everything between a `::group::`/`::endgroup::` pair by default, and a `::error::` becomes an
annotation on the file it names, visible on a pull request's "Files changed" view without opening the log at all.
`git tpl test` already has exactly the structure this rewards: one case, one name, one pass/fail outcome, one file
to annotate — the case file itself.

## Decision

When `GITHUB_ACTIONS=true` is set (GitHub's own signal that a job runs there — no other variable is checked, and
no flag exists to force or suppress this, matching this project's existing CI auto-detection philosophy, e.g.
`TERM=dumb` in `theme::decide`) and the run is not `--json`/`--quiet`, `git tpl test` reports progress as:

- Every case wrapped in `::group::<case name>` / `::endgroup::`, so a scan of the log shows one line per case by
  default, expandable on demand.
- While a case's group is open, behaviour matching today's `-v`: every command's own stdout/stderr forwarded
  live, byte for byte, and a status line per command as it finishes — regardless of whether `-v` was actually
  passed. There is no reason to ask separately; a GitHub Actions log is already folded, so the cost `-v` normally
  trades away (a wall of text) does not apply here.
- Exactly one `::error::` per failing case — not one per `Failure` — annotated to the case's own file
  (`file=<case.path>`), titled with the case name, and carrying every one of that case's failures' prose as the
  message. One annotation per case, however many assertions it failed, is what keeps a pull request's "Files
  changed" tab legible: a case with six failing `expect.contains` entries is one thing wrong with one file, not
  six.
- Everything that is today only `-v`'s internal phase chatter — "rendering", "checking snapshot", the pre-run
  announcement of a command about to start — goes to `::debug::` instead of the group's ordinary output. GitHub
  only shows a `::debug::` line when a job has step debug logging enabled, which is exactly the audience for "is
  the runner stuck, or has it not started running yet" — not every log's default view.

`--json` never sees any of this: `Reporter::speaks()` is already `false` whenever `global.json` (or `global.quiet`)
is set, and `TestProgress::choose` checks that before the GitHub Actions branch, not after — the precedence
guarantees it, rather than a separate check that could drift out of sync with it.

### `::group::`/`::error::`/`::debug::` all go to stdout, breaking this project's own convention

Every other command's human-readable output goes to stderr, so a piped `--json` stream stays parseable
(`src/report.rs`'s own header). GitHub's workflow commands are read from a step's stdout specifically; splitting a
`::group::` marker onto one stream and the output it is meant to fold onto another risks the two arriving out of
order in the runner's merged log, silently breaking the fold. So this one reporter — and only this one, gated by
the same `GITHUB_ACTIONS` detection that activates it — writes everything, including the plain per-case summary
line and the forwarded command output, to stdout instead of stderr. This is safe only because it can never
coincide with `--json`, whose own stdout purity this would otherwise corrupt.

### No new `Progress` hook

`tpl::ops::testing::Progress`'s five existing methods — `case_started`, `case_status`, `command_finished`,
`command_output`, `case_finished` — already carry everything this needs, including `case_finished`'s
`&CaseOutcome`, which already has the case's own `path` and every `Failure`. No hook is added; `testing.rs` remains
unaware that GitHub Actions, or any CI system, exists.

### Reusing failure prose

`commands::test::print_failure`'s per-`Failure` prose is extracted into a pure `failure_lines`, returning the same
lines as data instead of printing them. The GitHub Actions reporter calls it with an uncoloured theme (an ANSI
escape inside a workflow command's message is either ignored or misread) and its own always-non-verbose form —
the group's own live forwarding already showed a captured command's output, so the annotation itself does not
repeat it — then joins every failure's lines with `\n`, percent-encoded per GitHub's own escaping rules (`%` →
`%25`, `\r` → `%0D`, `\n` → `%0A`, and additionally `:` → `%3A`, `,` → `%2C` for the `file=`/`title=` properties).

## Consequences

**A fourth `TestProgress` variant**, not a second `Progress` implementation: `GitHubActions` is chosen by the same
`choose` function that already picks `Silent`/`Spinner`/`Line`, ahead of the `-v`/terminal checks in precedence,
since it implies verbose-style forwarding unconditionally.

**No CLI flag.** `GITHUB_ACTIONS` is the only signal; there is nothing to document as an option in
`docs/usage/test.md`'s Options table, only a note in its Progress section.

**Nothing about `--shard`/`--record-durations` (ADR-036) depends on this** — the two features are independent,
and a GitHub Actions run may use either, both, or neither.
