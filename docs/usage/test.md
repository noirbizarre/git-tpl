# `git tpl test`

Run the test cases a template carries.

```sh
git tpl test ./my-template
```

A template above a few files has conditionals, and without a suite the first person to find a broken one is the
person generating a project from it.
A case says *given these answers, this is what comes out* — and it says it in the template repository, next to
the thing it describes.

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
".github/workflows/ci.yml" = ["deploy"]
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
| `snapshot` | Whether this case is written and compared against a recorded snapshot. `false` unless set. See [Snapshots](#snapshots). |

A path named in `expect.contains` or `expect.lacks` that the rendering never produces is a failure either way —
never a pass, vacuous or otherwise.
"This file does not mention `deploy`" is not proven by a file that never rendered.

Everything is optional.
A case with only `[answers]` asserts that the answer set renders at all, which is a real and frequently
sufficient test.

Unanswered questions take their defaults.
Nothing is ever prompted for — a prompt in a test runner is a hang — so a question with no default and no answer
fails the case with `tpl::eval::unanswered`, which is a true thing to know about the template.

## Options

| Flag | Effect |
|---|---|
| `[CASE]...` | Run only the named cases. A name, not a path: `tests/minimal.toml` is `minimal`. |
| `--tests` | Read cases from this directory instead of `tests`. |
| `--ref` | Branch, tag or commit to test. |
| `--root` | Test this subdirectory instead of the manifest's. |
| `--dirty` | Test the working tree. Local templates only. |
| `--write` | Record each case's rendering as its snapshot. |
| `--skip-commands` | Skip every case's `[commands]` for this run. |

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

A case with `error` cannot also have `files`, `absent`, `contains` or `lacks` — there is no rendering for them to
describe.
Split it into two cases.

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

The render is written **on top of** whatever `before` created, not into a directory cleared first — this is what
lets a case simulate rendering into a project that already exists, the way `update` actually renders, which
`--dirty`/`render` alone cannot exercise.

Each entry is a plain string, split into a program and its arguments the way a shell would parse quoting and
escapes — but no shell actually runs. There is no pipe, no glob, no redirection and no `$VAR` expansion. A
pipeline needs a script file: write it with `before`, then name it in `rendered`.

`before`, `rendered` and `after` each stop at their own first failure — they are sequential, and a later entry
usually assumes an earlier one worked. A failing `before`, or a render that fails without `expect.error` naming
it, skips straight to `finally`: there is nothing for `rendered`/`after` to run against. `finally` is the
opposite: every entry in it runs regardless of anything failing before or within it, because it is cleanup.

There is no timeout. A hanging command hangs the run.

### Running `git tpl test` is the consent

A case's `[commands]` need no confirmation: running `git tpl test` on a template you have in front of you is
already the same act as cloning a repository and running `make test` in it. Commands run by default. To skip them
for one invocation, pass `--skip-commands`; to disable them for yourself by default, set `tpl.testCommands` to
`false` (`git config tpl.testCommands false`) — `--skip-commands` can only disable further, never re-enable what
configuration turned off. See [ADR-027](../adr/027-test-case-commands.md) for the full reasoning, including why
this does not reopen the rule that a *rendered* project — `render`, `init`, `update` — cannot execute anything.

A case's [`trust`](#trust) rests on the same act of consent, extended to a template's declared remote data sources
— see [ADR-028](../adr/028-test-case-trust.md).

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
Those act on a real project for a real person, so they refuse by default and ask, once, for consent; `test` never
asks anybody, and the persistent `[trust]` list in `~/.config/git-tpl/config.toml` is not consulted here either —
a case's `trust` must mean the same thing on every machine that runs the suite, including CI, where there is
nobody to ask and nothing pre-trusted. See [ADR-028](../adr/028-test-case-trust.md) for the reasoning.

## Snapshots

```toml
# tests/minimal.toml
snapshot = true
```

```sh
git tpl test ./my-template --write
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

Four things worth knowing:

- **A case with `snapshot` unset (or `false`) is not a failure.** Snapshots are opt-in per case, and are neither
  written nor compared for it.
- **`snapshot = true` with nothing recorded yet is a failure, not a pass.** Run `--write` once to record it.
- **`--write` clears the case's snapshot directory rather than merging into it.** A template that stops
  producing a file has to be seen to stop.
- **`--write` does not stage or commit anything, and does not bless a broken case.** The `expect` assertions
  still run and still fail. You review the diff and commit it.

`--write` needs a local checkout, because the snapshot is written to a working tree.
On a template with none, it fails with `tpl::testing::write_needs_local`.

## Cases come from the revision, not from disk

| Given | Cases and snapshots come from |
|---|---|
| neither `--ref` nor `--dirty` | The local checkout's checked-out branch, at its `HEAD` — or, for a remote source, the remote's default branch. |
| `--ref X` | Commit, branch or tag `X`, committed. |
| `--dirty` | The working tree, uncommitted changes included. Local templates only. |

`git tpl test ./tpl --ref v1.2.0` runs *that tag's* cases against that tag's template, and compares against that
tag's snapshots.
`--dirty` runs the uncommitted ones.
Reading cases off the filesystem instead would make `--ref` mean something different here from everywhere else.

`--ref` and `--dirty` are mutually exclusive: a working tree and a named revision cannot both be "the" version
under test, and asking for both is refused at the parser rather than silently picking one.

The one exception is `--write`, which writes to the working tree — there is nowhere else to put a file somebody
has to review.

A snapshot is not subject to the project's `.gitignore` either, deliberately: it is data `--write` recorded, not a
project file a render produced.
An ordinary rule matching a snapshot's own filename — a bare `MANIFEST`, say, the Python `setup.py sdist`
convention — does not stop `--dirty` from reading it back.

## In CI

```sh
git tpl test .
```

Exit `0` if every case passed, `1` if any did not, and `1` with a [diagnostic code](../reference/diagnostics.md)
if the run could not start.

Under `--json`, note that `ok` is `true` whenever the *command ran*.
Whether the suite passed is `summary.failed`:

```console
$ git tpl --json test . | jq '{ok, failed: .summary.failed}'
{"ok": true, "failed": 2}
```

The two are separate because a caller has to be able to tell "two cases failed" from "the template could not be
resolved", and both are non-zero exits.

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
