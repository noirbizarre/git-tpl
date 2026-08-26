# ADR-028: A test case declares its own trust in remote data sources

**Status:** accepted

**Supersedes:** the application, to `git tpl test` only, of ADR-013's "never granted by omission" rule for a
template's declared remote data sources; and the framing in ADR-027's "Gating" section that a template's
data-source trust and its test-time capability are unrelated, oppositely-shaped concerns.

**Relates to:** [ADR-016](016-template-tests-are-data.md), [ADR-013](013-user-configuration.md),
[ADR-027](027-test-case-commands.md)

## Context

`--trust` on `git tpl test` authorised, once for the whole run, every remote data source the template declares.
Without it, the run fell back to `test`'s own gate — `Trust::Ask`, an interactive confirmation. That default is
wrong for this command specifically: `test` is meant to run non-interactively, most usefully in CI, and a template
with even one `[data.*]` source that is `remote` or `git` could not be tested for real there without `--trust` on
every invocation.

A flag that a template's own suite structurally requires on every run stops being a choice. It also asks the wrong
question. `--trust` on `init`/`update` is "may this reach the network on *my* behalf, against *my* project" — a
decision ADR-013 correctly keeps opt-in and never grants by omission, because a CI runner is the worst place to
acquire a capability by silence, and the consequence of getting it wrong is a commit made from data nobody agreed
to fetch. `git tpl test` has no project, writes nothing, and — since ADR-027 — already lets a case run arbitrary
`[commands]` with no such prompt, on the reasoning that running the suite at all is the consent: "the same trust a
`Makefile`'s `test` target already carries." Whether a case's render may reach the network its own template
declared is the same question, asked by the same author, at the same moment.

The status quo could not express that at all: trust was a property of the *run*, decided once, and replayed
identically for every case, so there was no way to write a case whose entire point was proving the refused path
(`tpl::data::untrusted`) — doing so meant either always refusing (which broke every case that needed real data) or
always trusting (which made the refusal path untestable, and unreachable from a machine with nobody to prompt).

## Decision

A case file may set `trust`, a boolean, alongside `answers`, `expect`, `commands` and `snapshot`:

```toml
# tests/remote.toml
trust = false

[expect]
error = "tpl::data::untrusted"
```

`true` on omission: a case renders against the template's real declared data sources unless it says otherwise,
extending ADR-027's "running `test` is the consent" argument from `[commands]` to a template's own `[data.*]`
declarations, for this command only. `trust = false` is the deliberate opt-out — a case that wants to prove the
refusal itself, deterministically, with no network reachable and nobody to ask.

`--trust` is removed from `git tpl test`'s command line entirely, along with the interactive `Trust::Ask` fallback
that flag existed to avoid. `git tpl test` never prompts and never consults `~/.config/git-tpl/config.toml`'s
persistent `[trust]` list (ADR-013): that list exists for `init`/`update`, which act on a real project one person
owns, and a case's `trust` has to mean the same thing on every machine that runs the suite, this developer's
laptop and CI alike. A config file able to override it would make `trust = false` pass on one machine and fail on
another for the same case, which defeats the point of writing it down.

The decision is still all-or-nothing per case, matching the existing granularity of `declared_remotes`: a case
cannot trust one of a template's several declared sources and refuse another. That remains a template-wide fact,
just no longer a whole-*run* one — each case now decides for itself which of the two outcomes it wants.

## Consequences

**What stays closed.** `render`, `init`, `update` and `backport` are entirely unaffected. Each keeps its own
`--trust`, the interactive confirmation, and the persistent `[trust]` list exactly as ADR-013 described them: a
real project, acted on for a real person, still refuses a remote data source by default and never grants one by
omission.

**What changes, narrowly.** Only `git tpl test`'s default flips, and only for the specific, already-declared set
of a template's own remote sources — the same narrowing ADR-027 already made for `[commands]`, extended one step
further by the same reasoning. A person who has not read a template and runs `git tpl test` on it has still chosen
to do so, in the same sense `git clone && make test` is already a choice; this ADR does not reopen that question,
only the one about which of that template's own declared network calls its suite is allowed to make on its behalf.

**On-disk contract.** `trust` is an addition to the case schema ADR-016 governs, parsed exactly like `snapshot` —
a plain boolean, defaulting to the opposite of `snapshot`'s default for the opposite reason: `snapshot` opts a case
*into* an extra assertion; `trust` opts a case *out of* the render its answers would otherwise produce.

**Cost.** The one integration test that exercised `--trust` on `test` is rewritten to exercise the new default
instead, and a new test exercises `trust = false` — a negative path that could not previously be tested at all
without a real, reachable, refusing network endpoint or a TTY to answer a prompt.
