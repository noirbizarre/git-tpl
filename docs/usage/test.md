# `git tpl test`

Snapshot manifests accept mode `120000` for symbolic links.
The corresponding `files/` payload is an ordinary file containing target bytes, never a live link.
Comparison checks mode as well as bytes, including regular-file/link conversions.
Command sandboxes materialise real links on Unix and fail explicitly on unsupported platforms.
See [ADR-044](../adr/044-symlink-artifacts.md).

Run the test cases a template carries.

```sh
git tpl test
```

A template above a few files has conditionals, and without a suite the first person to find a broken one is the
person generating a project from it.
A case says *given these answers, this is what comes out* — and it says it in the template repository, next to
the thing it describes.

`test` takes an optional `--template PATH`, defaulting to the current directory, but only ever a local checkout —
never a remote source, with or without `--ref`. There is also no `--root` to override the manifest's declared
render subdirectory. `--template` is a flag rather than a positional so a bare case name is never mistaken for a
template path — see [ADR-030](../adr/030-test-never-resolves-a-remote-template.md).

## A case

Cases are files in `tests/` at the template repository root, in TOML, JSON or YAML.
The file's stem is the case name.

```toml
# tests/minimal.toml
[answers]
project_name = "thing"
with_ci = false

[expect]
files = ["pyproject.toml", "src/thing/__init__.py"]
absent = [".github/workflows/ci.yml"]

[expect.contains]
"pyproject.toml" = ['name = "thing"']

[expect.lacks]
"pyproject.toml" = ["deploy"]
```

| Key | Meaning |
|---|---|
| `answers` | The answer set to render with. Same shape as an [answers file](answers.md). |
| `trust` | Whether this case's render may reach the template's declared remote data sources. `true` unless set to `false`. See [Trust](#trust). |
| `expect.files` | Paths the rendering must contain. |
| `expect.absent` | Paths it must not. |
| `expect.contains` | Path to the text that must appear in it. A bare string or an array. |
| `expect.lacks` | Path to the text that must not appear in it. Same shape as `contains`. |
| `expect.error` | A [diagnostic code](../reference/diagnostics.md) the render must fail with. |
| `commands` | Setup, checks and teardown run around the rendering. See [Commands](#commands). |
| `git` | Whether the sandbox `commands` runs in is an isolated Git repository, seeded with one commit. `false`/absent unless set. See [Git sandbox](#git-sandbox). |
| `snapshot` | Whether this case is written and compared against a recorded snapshot. `false` unless set. See [Snapshots](#snapshots). |
| `migration` | Initialize an old project, update and merge it, then assert the result. See [Migration cases](#migration-cases). |

A path named in `expect.contains` or `expect.lacks` that the rendering never produces is a failure either way —
never a pass, vacuous or otherwise.
"This file does not mention `deploy`" is not proven by a file that never rendered.

Everything is optional.
A case with only `[answers]` asserts that the answer set renders at all, which is a real and frequently
sufficient test.

Unanswered questions take their defaults.
Nothing is ever prompted for — a prompt in a test runner is a hang — so a question with no default and no answer
fails the case with `tpl::eval::unanswered`, which is a true thing to know about the template.

An answer that names no question fails the case too, with `tpl::answers::unknown_key` — unconditionally, unlike
`render`'s `--strict-answers`, which this command has no equivalent of and never will (see
[Options](#options) below). A case's `[answers]` is hand-authored, not a recorded set that might outlive a dropped
question, so an unrecognised key here is always the case's own mistake. See
[ADR-029](../adr/029-test-case-answers-are-strict.md).

## Options

| Flag | Effect |
|---|---|
| `[CASE]...` | Run only the named cases. A name, not a path: `tests/minimal.toml` is `minimal`. |
| `--template` | The local checkout to test; defaults to the current directory. Never a remote source. |
| `--tests` | Read cases from this directory instead of `tests`. |
| `--ref` | Branch, tag or commit to test; without it, the working tree. |
| `--write` | Record each case's rendering as its snapshot. Only the cases that declare `snapshot = true` are touched; see [Snapshots](#snapshots). |
| `--skip-commands` | Skip every case's `[commands]` for this run. |
| `--shard INDEX/TOTAL` | Run only the cases assigned to this shard of a balanced split; see [Sharding](#sharding). |
| `--record-durations` | Time every case and record it for a later `--shard` to balance by; see [Sharding](#sharding). |

There is no `--answer`, `--answers-from` or `--defaults`.
The case file *is* the answer set: a flag that changed the answers would change what every case asserts while
every case file still said otherwise.

## Failures are asserted by code, never by message

```toml
[expect]
error = "tpl::eval::wrong_type"
```

A suite that pinned error prose would make every improvement to a diagnostic a breaking change, which is how
error messages stop improving.
The [codes](../reference/diagnostics.md) are already the stable surface, so they are what a case names.

The code is matched anywhere in the failure's cause chain.
`tpl::render::content` says a file failed to render; only the `tpl::eval::expression` beneath it says why, and
that is usually the one worth naming.
Either passes.

A case with `error` cannot also have output or migration expectations — there is no successful result for them to
describe.
Split it into two cases.

## Migration cases

A migration case exercises a real project upgrade, including committed project-local changes:

```toml
# tests/readme-move.toml
snapshot = true

[answers]
project_name = "widget"

[migration]
from = "v1"
# to = "v2"  # optional; defaults to this suite's target
absent = ["obsolete.txt"]

[migration.answers]
project_name = "old-widget"

[migration.files]
"README.md" = "# My project\n\nA project-local customisation.\n"
"local-only.txt" = "Keep this file.\n"

[expect]
files = ["docs/README.md", "local-only.txt"]
absent = ["README.md", "obsolete.txt"]
migrations = ["migrations/010-readme.toml"]

[expect.contains]
"docs/README.md" = "project-local customisation"

[expect.messages]
"migrations/010-readme.toml" = ["README", "widget"]
```

`from` is a required branch, tag or commit in the template's local repository.
`to`, if supplied, also names a committed revision there.
Without `to`, the destination is the suite's resolved target: the current working tree by default,
or the revision selected by `--ref`.
The suite target supplies case files and snapshots even when a case selects a different destination.
Keep old revisions available in your checkout; a shallow clone may need more history or tags.

The runner initializes an isolated Git project from `from`, applies and commits the literal local edits,
runs the production update flow, then merges the updated template ref with the Git backend.
The expectations and snapshot inspect that **merged project**, including local-only files.
They exclude the harness's attachment file `.config/git.tpl.toml` and Git metadata.
Any merge conflict fails the case and identifies Git's conflicting paths.

`migration.answers` supplies old-state answers.
If omitted, it copies the top-level `answers`; an explicit empty table supplies no old answers.
Initialization records defaults as usual, then top-level `answers` overrides those recorded answers during update.
Both hand-authored answer sets are strict against their respective revisions.
Old recorded questions removed by the destination are tolerated, as with an ordinary update.
Each endpoint uses the case's `trust` setting for its own declared data sources.
Ancestor and composed-child repository access still follows the ordinary trust rules.

`migration.files` maps project-relative paths to literal UTF-8 contents; there is no interpolation.
It adds or replaces files, preserving an existing regular file's executable bit.
Replacing a symlink creates an ordinary text file without following its target.
`migration.absent` deletes named existing files; a missing deletion is a setup failure.
Unsafe paths, Git metadata, attachment configuration and overlapping edits are rejected.

`expect.migrations` requires the exact discovered paths in application order.
Omit it to make no discovery assertion; use `[]` to require no newly crossed migrations.
`expect.messages` maps migration paths to a string or nonempty array of substrings in their evaluated messages.
A missing migration or message fails the assertion.
These are template-authored messages; git-tpl diagnostic failures are still asserted only by stable code.
Migration-specific expectations require a `[migration]` section.

For negative cases, `expect.error` matches a destination **update** failure's diagnostic cause chain.
Failure to resolve or initialize the old state, invalid local edits and merge conflicts cannot satisfy it.
As in render cases, it cannot be combined with successful-output expectations, snapshots,
or `commands.rendered`/`commands.after`.

### Intermediate migrations

Each case performs one direct endpoint update.
New migration files are discovered by comparing the old and destination template trees,
then applied in path order, with moves in declaration order.
Moves operate on the running old rendered tree; messages use the destination answers.
The runner does not render every historical intermediate revision.

Test `v1` → current to cover a skipped-version jump and chained moves.
Add separate cases with `from = "v1", to = "v2"` and `from = "v2"` to cover individual upgrade boundaries.
A file introduced only in a skipped intermediate rendering cannot be moved from the starting tree.
That is a real update failure, and a direct-jump case exposes it.

### Commands and recording

Migration cases always need isolated Git, including under `--skip-commands` and with `git = false` or omitted.
`git = true` or a Git identity table configures this same repository.
Its checkout defaults pin `core.autocrlf = false` and `core.eol = lf`
so ambient line-ending conversion cannot make the merged project appear dirty to commands.
`commands.before` runs before old-project initialization; declare committed customisations in `migration.files`
and `migration.absent`.
`commands.rendered` sees the completed merged project, `after` follows assertions,
and `finally` runs even after preparation, update or merge failures.
Command edits do not change the immutable tree used by expectations and snapshots.

`--write` runs the declarative upgrade to obtain a snapshot, but runs no commands or expectations.
Cases without `snapshot = true` are skipped.
Preparation, update and merge must succeed before anything is recorded.
Only snapshot files are written into the template checkout, unstaged.
See [ADR-046](../adr/046-declarative-migration-tests.md) for the contract.

## Commands

```toml
[commands]
before   = ["mkdir -p src", "touch src/existing.py"]
rendered = ["python -m venv .venv"]
after    = [".venv/bin/pip install -e ."]
finally  = ["rm -rf .venv"]
```

Four lists, each run in a fresh, empty scratch directory created just for this case and thrown away afterward:

| List | Runs | Sees |
|---|---|---|
| `before` | Before anything is rendered. | Nothing but what it creates itself. |
| `rendered` | After the rendering is written into the sandbox, before `expect` is checked. | `before`'s files, merged with the render. |
| `after` | After `expect` and the snapshot are checked. | The same merged sandbox. |
| `finally` | Always, last, regardless of anything above. | Whatever state the sandbox is in. |

The render is written **on top of** whatever `before` created, not into a directory cleared first.
This lets a case test its output alongside files that already exist in a project, which `--dirty`/`render` alone
cannot exercise. `update` itself builds a Git tree and never writes rendered files into the project worktree.

`before` sees "nothing but what it creates itself" only when the case has no [`git`](#git-sandbox): a case that
sets it finds a `.git` directory and one commit already there before `before` runs its first entry.

Each entry is a plain string, split into a program and its arguments the way a shell would parse quoting and
escapes — but no shell actually runs. There is no pipe, no glob, no redirection and no `$VAR` expansion. A
pipeline needs a script file: write it with `before`, then name it in `rendered`.

`--write` never runs any of the four lists. It only records what a case renders; proving what running it *does* is
what a plain `git tpl test` is for. See [Snapshots](#snapshots) and
[ADR-032](../adr/032-write-only-writes.md).

### Environment

Every command inherits the full environment `git tpl test` itself runs under. `commands.env` adds to it, for
every list in the case:

```toml
[commands]
env = { PDM_IGNORE_ACTIVE_VENV = "true" }
rendered = ["pdm install --plugins", "pdm install -dG:all", "pdm test"]
```

A single list can override `commands.env` for itself alone by writing it as a table with `run` and `env` instead
of a bare array:

```toml
[commands]
env = { PDM_IGNORE_ACTIVE_VENV = "true" }

[commands.after]
env = { PDM_IGNORE_ACTIVE_VENV = "false" }
run = ["pdm run something-that-needs-the-active-venv"]
```

`commands.after.env` wins over `commands.env` for a key both set; every other list still gets `commands.env`
alone. Neither adds `$VAR` expansion to a command's own text — a value only ever reaches a process as an
environment variable, never as text substituted into the command line before it runs.

Every command also sees `TEMPLATE_ROOT`, set to the resolved template's root on disk — the working tree for
`--dirty`, and the same working tree for a local `--ref`, since `test` never resolves a remote (see
[ADR-030](../adr/030-test-never-resolves-a-remote-template.md)). It is distinct from both `cwd` (each list's own
throwaway sandbox) and from a template's declared render subdirectory: a case's script does not live in the
sandbox, so a command that needs more than a line or two writes it once, commits it in the template repository,
and names it directly — `"{{ env.TEMPLATE_ROOT }}/tests/scripts/check.sh"` — instead of synthesizing it inline via
`before` on every run. A case's own `env`/`commands.env` may still override it for a key it deliberately sets, the
same as any other default.

`before`, `rendered` and `after` each stop at their own first failure — they are sequential, and a later entry
usually assumes an earlier one worked. A failing `before`, or a render that fails without `expect.error` naming
it, skips straight to `finally`: there is nothing for `rendered`/`after` to run against. `finally` is the
opposite: every entry in it runs regardless of anything failing before or within it, because it is cleanup.

There is no timeout. A hanging command hangs the run.

Every command also gets `CLICOLOR_FORCE=1` and `FORCE_COLOR=1` added to its environment, whenever `git tpl test`'s
own output is colourised under the [effective colour policy](../configuration.md#the-environment).
Explicit `--color always` or force-colour variables can enable colour despite `NO_COLOR` or non-terminal stderr.
A command talking to a pipe rather than a terminal otherwise assumes nobody can see colour.
Capturing its output requires a pipe, but that output can still be shown on a real terminal.
`commands.env` (or a list's own `env`) still wins for either variable,
so a case that wants plain output from a tool that only checks these two can still ask for it.

A CI job wants colour for the same reason, but has to ask for it explicitly: a runner's shell is not a terminal,
so `--color auto` (the default) never colourises there on its own — the same reason a workflow already sets
`CARGO_TERM_COLOR=always` to get colour out of `cargo` itself. Set `FORCE_COLOR=1` or `CLICOLOR_FORCE=1` (or pass
`--color always`) in the job's own environment — either variable works, they are treated as aliases for the same
signal:

```yaml
env:
  FORCE_COLOR: "1"
```

One variable does double duty: it colourises `git tpl test`'s own progress output below, *and* is what causes
`CLICOLOR_FORCE`/`FORCE_COLOR` to be forwarded to `[commands]` children, as described above — so a case's own
`cargo build` no longer needs `CARGO_TERM_COLOR` set separately for that part either. See
[the environment](../configuration.md#the-environment) for the full colour precedence.

### Templating

A command is rendered as a MiniJinja template — the same engine that renders every other string in the template —
before it is word-split, against two names:

- `env.*` — exactly what that command is about to be spawned with: the inherited environment, `TEMPLATE_ROOT`,
  `CLICOLOR_FORCE`/`FORCE_COLOR` when set, and the list's own `env`/`commands.env`, all merged in the order
  described above. `{{ env.TEMPLATE_ROOT }}` always names the same path `$TEMPLATE_ROOT` would inside the process.
- `answers.*` — the case's own `[answers]` table, exactly as written, with no defaults or computed values applied.
  Available to `before` as well as `rendered`/`after`/`finally`, since it comes from the case file itself rather
  than from a render.

```toml
[answers]
name = "widget"

[commands]
rendered = ["'{{ env.TEMPLATE_ROOT }}/tests/scripts/check.sh' '{{ answers.name }}'"]
```

A command with no `{{`/`{%` in it is never templated, so every case written before this existed keeps behaving
identically. One that references a name that does not exist — `{{ answers.projct_name }}`, a typo — fails that
command alone, reported the same way a missing program already is, rather than silently spawning it with an empty
string in place of the mistake.

Each list entry is a complete command, including its arguments.
The quotes above preserve spaces; values containing quote characters need appropriate word-splitting escapes.

This is not `$VAR` shell expansion: a literal `$FOO` in a command is still passed through unchanged. Only
`{{ }}`/`{% %}` mean anything, because MiniJinja renders the string — no shell runs, and nothing about the command
format above changes.

### Progress

A case in progress shows on stderr: a spinner naming the current case and phase — `rendering`, or a running
command — on a real terminal, or the same events as plain scrolling lines when stderr is not one (a CI log, say).
Neither shows under `--quiet` or `--json`.

`-v`/`--verbose` replaces the spinner with a full scrolling log: every phase, and every command's own stdout/stderr
forwarded live, exactly as it writes it — ANSI included. The final report no longer repeats a failed command's
captured output in that case, since it was already shown as it happened; `--json` is unaffected either way, and
always carries it.

Under `--write`, no `[commands]` run at all (see [Snapshots](#snapshots)), so there is nothing for `-v` to stream
live. In its place, `-v` shows the same coloured unified diff a normal run's snapshot-mismatch failure already
shows, once per case whose snapshot changed — and the final summary names which cases were written, updated or
left unchanged. See [ADR-032](../adr/032-write-only-writes.md).

### Under GitHub Actions

When `GITHUB_ACTIONS=true` is set — every job on GitHub Actions sets it, so this needs no flag and has no
opt-out — `git tpl test` reports progress as [workflow
commands](https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions) instead of any
of the above: each case is wrapped in a `::group::`/`::endgroup::` pair, so a scan of the log shows one line per
case with everything else folded away, expandable on demand. While a case's group is open, its commands' own
stdout/stderr are forwarded live, exactly as `-v` would, whether or not `-v` was passed — a folded group has
nothing left to trade that away for. A failing case gets exactly one `::error::` annotation, naming the case's own
file, so it shows up on a pull request's "Files changed" view without opening the log at all. Everything that
would otherwise be internal phase chatter — "rendering", "checking snapshot" — goes to `::debug::` instead,
visible only once a job has step debug logging enabled.

`--json` is unaffected: it is checked first, and always wins. See
[ADR-035](../adr/035-github-actions-progress-reporter.md).

### Running `git tpl test` is the consent

A case's `[commands]` need no confirmation: running `git tpl test` on a template you have in front of you is
already the same act as cloning a repository and running `make test` in it. Commands run by default. To skip them
for one invocation, pass `--skip-commands`; to disable them for yourself by default, set `tpl.testCommands` to
`false` (`git config --global tpl.testCommands false` also applies outside repositories).
`--skip-commands` can only disable further, never re-enable what
configuration turned off. See [ADR-027](../adr/027-test-case-commands.md) for the full reasoning, including why
this does not reopen the rule that a *rendered* project — `render`, `init`, `update` — cannot execute anything.

Both also skip an ordinary render case's [`git`](#git-sandbox), since no commands remain to use its sandbox.
Migration cases still initialize isolated Git and run their migration assertions;
only their external command lists are disabled.

A case's [`trust`](#trust) rests on the same act of consent, extended to a template's declared remote data sources
— see [ADR-028](../adr/028-test-case-trust.md).

## Git sandbox

```toml
git = true

[commands]
rendered = ["pdm install", "pdm build"]
```

Some tooling needs Git for its own identity — `pdm-backend`'s `source = "scm"`, and every `setuptools_scm`-alike
version source, read `git describe` before installing a single dependency. `git = true` makes the sandbox
`commands` runs in an isolated Git repository rather than a plain scratch directory: one empty commit, already
there before `before` runs its first entry, so there is something for that tooling to find.

The identity is a built-in default unless overridden:

```toml
[git]
user.name = "Someone"
user.email = "someone@example.com"
```

`git.user.name`/`git.user.email` are the only keys a case may set; everything else about the seed commit —
`commit.gpgsign` off, the branch it lands on — is fixed. `git = false`, or no `git` key at all, is today's plain
scratch directory, byte for byte: this is opt-in, the same choice `snapshot` already made for the same reason.

**Isolation is by construction, not by fighting the running machine's configuration file by file.** The seed
commit is written through git-tpl's own Git backend, never a spawned `git` process, so no hook, alias or ambient
identity is ever consulted for it — the chosen identity lands in the sandbox's own local `.git/config` before
anything asks for a signature, and Git's own precedence (local always outranks global and system) makes it
authoritative regardless of what either ambient level says. And because a case's own `[commands]` may still
invoke a literal `git` — the whole point of seeding a repository at all — every command in a case that set `git`
is spawned with `GIT_CONFIG_NOSYSTEM=1` and `GIT_CONFIG_GLOBAL` pointed at a path that is never created, so a
literal `git` sees only the sandbox's own local config: no credential helper, no alias, no `commit.gpgsign` the
machine running the suite happens to have. `commands.env`/a list's own `env` may still override either variable
for a case that deliberately wants to.

See [ADR-033](../adr/033-test-case-git-sandbox.md) for the full reasoning, and issue #155 for where the gap this
closes came from.

## Trust

```toml
# tests/remote.toml
trust = false

[expect]
error = "tpl::data::untrusted"
```

`trust` decides whether this case's render may reach the template's declared remote data sources — a `[data.*]`
source that is `remote` or `git`.
It defaults to `true`: a case renders for real unless it says otherwise, because the point of a suite is to prove
what the template's output actually looks like, and an untested remote source is a gap in the suite, not a safety
margin.

Set `trust = false` to assert the *refused* path instead — the render fails with
[`tpl::data::untrusted`](../reference/diagnostics.md) before anything reaches the network, deterministically and
with no host to reach.

This is not the `--trust`/`[trust]` mechanism `render`, `init`, `update` and `backport` use.
For case-level data loading, `test` never asks anybody and does not consult the persistent `[trust]` list:
a case's `trust` must mean the same thing on every machine, including CI.
See [ADR-028](../adr/028-test-case-trust.md) for the reasoning.

Before discovering cases, `test` resolves the template's `[extends]` ancestors using persistent trust from
`~/.config/git-tpl/config.toml`.
Remote ancestors must be authorized there on each machine, including CI;
case-level `trust` cannot authorize them, and `test` has no invocation-wide `--trust` option.
Missing authorization fails with `tpl::extends::untrusted` before cases run.

## Snapshots

```toml
# tests/minimal.toml
snapshot = true
```

```sh
git tpl test --write
```

records each case's rendering under `tests/__snapshots__/<case>/`, and every later run compares against it.

A case is written and compared only when it says `snapshot = true`. This is explicit for a reason: without it,
`--write` would silently start recording (and every future run silently start comparing) a case that never asked
for one. A case that says `snapshot = true` but has never been recorded fails outright — "record one with
`git tpl test --write`" — rather than passing having asserted nothing.

```
tests/
├── minimal.toml
└── __snapshots__/
    └── minimal/
        ├── MANIFEST
        └── files/
            ├── pyproject.toml
            └── src/thing/__init__.py
```

The rendered files are stored **verbatim**, at their rendered paths.
That is the whole point: a change to the template shows up as a `git diff` of the generated project, in the
generated project's own language.
Reviewing it needs no tooling and no Rust.

`MANIFEST` records what the files themselves cannot — the executable bit, a digest and a size, one sorted line
each.
A Windows checkout loses the mode on disk, and a snapshot that silently stopped asserting on `chmod +x` would be
worse than none.

Things worth knowing:

- **A case with `snapshot` unset (or `false`) is not a failure** on a plain run. Snapshots are opt-in per case, and
  are neither written nor compared for it.
- **`snapshot = true` with nothing recorded yet is a failure, not a pass.** Run `--write` once to record it.
- **`--write` clears the case's snapshot directory rather than merging into it.** A template that stops
  producing a file has to be seen to stop.
- **`--write` does not stage or commit anything.** You review the diff and commit it.
- **`--write` only records; it does not run a case.** A case's `[commands]` never run, and `expect` is never
  checked, under `--write` — recording a broken rendering is not the same as passing it. Run a plain
  `git tpl test` afterward (or let CI do it) to prove the recording still holds. See
  [ADR-032](../adr/032-write-only-writes.md).
- **A case with no `snapshot = true` is skipped entirely under `--write`** — not rendered, not checked, not
  counted as run. `--write`'s only reason to look at a case is to write its snapshot.

`--write` writes into the checked-out template's working tree — there is nowhere else for a snapshot to go.

## Sharding

```sh
git tpl test --record-durations   # once, to seed tests/__timings__/durations
git tpl test --shard 1/4          # then, in each of four CI matrix jobs
git tpl test --shard 2/4
git tpl test --shard 3/4
git tpl test --shard 4/4
```

`--shard INDEX/TOTAL` (both starting at 1) runs only the cases assigned to this shard, out of whatever this
invocation would otherwise run — applied after `[CASE]` filtering, not instead of it. Without a recorded durations
file, the split is a deterministic slice by count: this never fails just because nothing has been recorded yet, so
a template's first `--shard` run works exactly as well as its hundredth.

`--record-durations` runs the suite exactly as a plain invocation would — nothing about `[commands]`, `expect` or
snapshots changes — while timing each case, then writes `tests/__timings__/durations`, merged over whatever was
already there. Commit that file to the template repository; a later `--shard` reads it back and bin-packs the
split so every shard finishes in about the same time, rather than merely getting the same number of cases. A case
missing from the file (new, or never measured) is weighted at the mean of the ones that are.

`--record-durations` conflicts with `--shard` — recording from a partial run would corrupt the file with timings
for only some of the suite — and with `--write`, which does not run a case at all and so has nothing real to time.

A GitHub Actions matrix supplies both halves of `--shard` directly, with only the shard index varying between
jobs:

```yaml
strategy:
  matrix:
    shard: [1, 2, 3, 4]
steps:
  - run: git tpl test --shard ${{ matrix.shard }}/${{ strategy.job-total }}
```

This is unrelated to the "no matrix language" rule below: that rule is about a *case file* expanding into many
cases combinatorially, which this project still refuses. Splitting the cases that already exist across several CI
jobs is a different question, and `--shard`/`--record-durations` is this project's answer to it. See
[ADR-036](../adr/036-shard-and-record-durations.md).

## Cases come from the revision, not from disk

| Given | Cases and snapshots come from |
|---|---|
| no `--ref` | The working tree, uncommitted changes included. |
| `--ref X` | Commit, branch or tag `X`, committed. |

`git tpl test --ref v1.2.0` runs *that tag's* cases against that tag's template, and compares against that tag's
snapshots.
Without `--ref`, `test` reads the working tree instead: the point of testing is catching a broken conditional
before it is committed, not after.
Reading cases off the filesystem is not a special case, though it may look like one: the resolver builds a
synthetic tree of the working directory the same way it always has for a `--dirty` render on any other command, so
an edited-but-uncommitted case is picked up by the same code path, with the same `.gitignore` handling the
rendering got. See [ADR-030](../adr/030-test-never-resolves-a-remote-template.md).

A snapshot is not subject to the project's `.gitignore` either, deliberately: it is data `--write` recorded, not a
project file a render produced.
An ordinary rule matching a snapshot's own filename — a bare `MANIFEST`, say, the Python `setup.py sdist`
convention — does not stop it from being read back.

## In CI

```sh
git tpl test
```

Exit `0` if every case passed, `1` if any did not, and `1` with a [diagnostic code](../reference/diagnostics.md)
if the run could not start.

Under `--json`, note that `ok` is `true` whenever the *command ran*.
Whether the suite passed is `summary.failed`:

```console
$ git tpl --json test | jq '{ok, failed: .summary.failed}'
{"ok": true, "failed": 2}
```

The two are separate because a caller has to be able to tell "two cases failed" from "the template could not be
resolved", and both are non-zero exits.

`--json` also carries `summary.durationsRecorded` (whether this run wrote the durations file), a top-level `shard`
object (`null` unless `--shard` was given: `{index, total, casesTotal, balanced}`), and a `durationMs` on every
case — measured on every run, whether or not `--shard`/`--record-durations` was used. See
[Sharding](#sharding) and [ADR-036](../adr/036-shard-and-record-durations.md).

## What a case cannot do

`[commands]` runs against a scratch sandbox the test harness creates and destroys for one case — never against a
`render`, `init` or `update`, none of which can execute anything, still, unconditionally. See
[Commands](#commands) and [ADR-027](../adr/027-test-case-commands.md) for exactly what a case's commands can and
cannot see.

For everything outside that sandbox — checking a real, project-shaped rendering with the tools that understand
it, `actionlint`, a full `npm ci`, a linter with its own config discovery — your own CI still does it better:

```sh
git tpl render . --dirty -o /tmp/out --answers-from tests/minimal.toml
cd /tmp/out && cargo build && actionlint .github/workflows/*.yaml
```

There is also no matrix language.
Three files beat a combinatorial block whose expansion nobody can predict; if a template needs twelve cases,
twelve files say so honestly.

## A note on partials

A rendered file named `something.jinja` lands in the snapshot as
`tests/__snapshots__/<case>/files/something.jinja`, and the renderer collects every `.jinja` outside the render
root as an importable partial.
It is keyed by its full path, so it shadows nothing and changes no rendering.
