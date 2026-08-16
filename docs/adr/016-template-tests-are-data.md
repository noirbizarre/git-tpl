# ADR-016: Template tests are data

## Status

Accepted.

## Context

A template author had no way to say *"given these answers, this is what comes
out"* except to render into a scratch directory and look. Every template above
a few files therefore had no tests, and the first person to find a broken
conditional was the person generating a project from it.

The primitives already existed — `git tpl render` and `--answers-from`. What
was missing was a runner, and the shape of a runner is where a project of this
kind goes wrong: a test runner is the single most tempting place to break the
rule that templates cannot execute code.

## Decision

`git tpl test` runs cases written as **data** in the template repository. Cases
live at `tests/*.{toml,json,yaml}` and are read by the same parsers
`--answers-from` uses, so a `.yaml` case and a `.yaml` answers file cannot come
to disagree about what `no` means.

### The assertion vocabulary is closed

A case may assert `files`, `absent`, `contains`, `error`, and a snapshot.
Nothing else, and in particular:

**No `command` key.** This is invariant 5 — templates cannot execute code — at
the one surface where breaking it would feel most reasonable. A template
repository is untrusted input, and `git tpl test` on a template you have not
read must not be a way to run its author's shell. Checking the *output* with
the tools that understand it is the author's own CI's job, which it does better
anyway. This extends ADR-003's reasoning to a new surface rather than
introducing a new rule.

**No matrix language.** Three files beat a combinatorial `[[matrix]]` block
whose expansion nobody can predict. If a template really needs twelve cases,
twelve files say so honestly.

**No message matching.** A failure case names a diagnostic code:

```toml
[expect]
error = "tpl::questions::type"
```

A suite that pinned error prose would make every diagnostic improvement a
breaking change, which is how error messages stop improving. The codes are
already the stable surface (ADR-015), so they are what a case names. The code
is matched anywhere in the cause chain, because `tpl::render::content` says a
file failed and only the `tpl::eval::expression` beneath it says why — and
which wrapper a failure arrives in is not part of the stable surface.

### Cases are read from the resolved revision

Not from the filesystem. `--ref v1.2.0` runs that tag's cases against that
tag's template and compares against that tag's snapshots; `--dirty` runs the
uncommitted ones. Anything else would make `--ref` mean something different
here from everywhere else in the tool.

`--dirty` needs no special handling as a result: the resolver has already built
a synthetic tree of the working directory, so an edited-but-uncommitted case is
picked up by the same code path, with the same `.gitignore` handling the
rendering got.

### Snapshots are reviewable files, not a serialised blob

`--write` records the rendering under `tests/__snapshots__/<case>/`:

```
tests/__snapshots__/minimal/
├── MANIFEST
└── files/pyproject.toml
```

This is `insta`'s model, and it is the right one — but implemented plainly
rather than by depending on `insta`. The snapshots belong to the *user's*
template repository, and a template author should not need a Rust toolchain to
read one. Storing the rendered files verbatim at their rendered paths means a
template change shows up as a `git diff` of the generated project, in the
generated project's own language.

`MANIFEST` carries what verbatim files cannot: the executable bit, a digest and
a size. Git records nothing about permissions but the executable bit, and a
Windows checkout of the template repository loses even that — a snapshot that
silently stopped asserting on `chmod +x` would be worse than none. Its first
line is `# git-tpl snapshot 1` so that a later version can recognise and migrate
an older snapshot rather than reporting every file as changed.

The manifest is authoritative for the file list and the modes; `files/` is
authoritative for content. A disagreement between them is
`tpl::testing::snapshot_read` telling the author to re-record, never a silent
preference for one half.

### `--write` targets the working tree, and only the working tree

A snapshot has to be reviewable and committed deliberately. Writing it as a Git
object would put a rendering into the template's history that nobody asked for;
staging it would make review a step somebody skips. So `--write` writes files,
stages nothing, commits nothing, and is refused on a source with no working
tree — by the same locality rule `--dirty` uses, so the two flags cannot come to
disagree about what "local" means.

It clears the case's snapshot directory rather than merging into it, for the
reason `render --force` does: a template that stops producing a file has to be
seen to stop. And it never suppresses an `expect` failure — it records a
rendering, it does not bless a broken one.

### A case with no snapshot is not a failure

Snapshots are opt-in per case. A template with three cases and one snapshot is a
normal state; failing the other two would force `--write` on people who only
wanted `expect.files`.

### Failures are data, not errors

An unmet expectation is a value in the report, not an `OpError`. Twelve failing
cases must all be reported — an error that aborted at the first would report one
per invocation, and the author would fix them one release at a time. Only things
that stop the *run* are errors, and they use the area `testing` rather than
`test`, which is reserved for the diagnostic fixtures in `src/report.rs`.

Consequently `--json` reports `ok: true` for a run with failing cases: `ok` says
the command ran, and `summary.failed` says what it found. This is the split
`lint` already uses, and it exists so a caller can tell "two cases failed" from
"the template could not be resolved" — both of which exit non-zero.

## Consequences

The case file schema and the snapshot format are on-disk contracts in *users'*
repositories, with the same permanence as `template.toml` (ADR-010). Changing
either is a breaking change and needs an ADR superseding this one.

The template is resolved once per run rather than once per case. A remote
template is cloned once, and a report saying "12 cases at abc1234" is true even
if the branch moved mid-run. Remote data sources are likewise confirmed once,
and the decision — including a refusal — is replayed for every case.

A rendered file named `something.jinja` appears in the snapshot directory and is
therefore collected as an importable partial. It is keyed by its full path, so
it shadows nothing; if it ever becomes a nuisance, excluding the snapshots
directory from `collect_partials` is a one-line change.
