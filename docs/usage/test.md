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
| `expect.files` | Paths the rendering must contain. |
| `expect.absent` | Paths it must not. |
| `expect.contains` | Path to the text that must appear in it. A bare string or an array. |
| `expect.lacks` | Path to the text that must not appear in it. Same shape as `contains`. |
| `expect.error` | A [diagnostic code](../reference/diagnostics.md) the render must fail with. |

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
| `--trust` | Allow remote data sources without asking. |

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

## Snapshots

```sh
git tpl test ./my-template --write
```

records each case's rendering under `tests/__snapshots__/<case>/`, and every later run compares against it.

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

Three things worth knowing:

- **A case with no snapshot is not a failure.** Snapshots are opt-in per case.
- **`--write` clears the case's snapshot directory rather than merging into it.** A template that stops
  producing a file has to be seen to stop.
- **`--write` does not stage or commit anything, and does not bless a broken case.** The `expect` assertions
  still run and still fail. You review the diff and commit it.

`--write` needs a local checkout, because the snapshot is written to a working tree.
On a template with none, it fails with `tpl::testing::write_needs_local`.

## Cases come from the revision, not from disk

`git tpl test ./tpl --ref v1.2.0` runs *that tag's* cases against that tag's template, and compares against that
tag's snapshots.
`--dirty` runs the uncommitted ones.
Reading cases off the filesystem instead would make `--ref` mean something different here from everywhere else.

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

There is no `command` key, and there will not be one.
git-tpl runs nothing over a rendering — [templates cannot execute code](../adr/016-template-tests-are-data.md),
and a test runner is exactly where that rule is most tempting to break.

Checking the output with the tools that understand it is your own CI's job, and it does it better:

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
