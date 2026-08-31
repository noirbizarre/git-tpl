# ADR-027: A test case may declare commands, run by the harness alone

**Status:** accepted

**Supersedes:** the "no `command` key" clause of [ADR-016](016-template-tests-are-data.md)

**Relates to:** [ADR-013](013-user-configuration.md), [ADR-019](019-templates-address-never-act.md)

## Context

ADR-016 closed the assertion vocabulary and forbade a `command` key in the same breath: "a template repository is
untrusted input, and `git tpl test` on a template you have not read must not be a way to run its author's shell."
The status quo it left in place — render with `git tpl render --dirty -o /tmp/out`, then check the output with
`cargo build`, `actionlint`, or whatever the template emits, wired up in the author's own CI — was correct then and
remains correct for that job. This ADR does not reopen it.

ADR-019 separately declined a *post-render task list* (issue #32): a confirmed, `init`-only mechanism for running
commands like `npm install` or `git remote add` against the user's real project after a merge. It failed its own
bar — every command surveyed was already served by `init`, permanently out of reach of invariant 5, or not worth
the mechanism for the one command left over.

Neither ADR considered what this one is about: what a template's own *author* needs proven, which the status quo
genuinely cannot express.

**Setup and teardown around a render.** A case asserting "this is a working Python project" wants
`python -m venv .venv && .venv/bin/pip install -e .` run against the rendering before it can be called green, and
wants the venv removed after — for every case, in every template repository, without an author hand-wiring that CI
step once per template they maintain. `expect.files`/`expect.contains` can check what a render *produced*; nothing
today can check what running it *does*.

**A pre-existing project.** git-tpl's one real consumer of a rendering, `update`, never renders into an empty
directory — it renders into a project that already exists, and what the user ends up with is a merge of the two.
`--dirty`/`render` cannot exercise that at all: git-tpl never touches a worktree outside the render itself
(invariant 1), so there was no way for a case to say "given a project that already has a `pyproject.toml` and a
half-written `src/app.py`, does the template's rendering merge onto it sanely?" That is exactly the scenario `update`
exists for, and the one the suite could not pose a question about.

## Decision

A case file may carry a `[commands]` table, alongside `[answers]` and `[expect]`:

```toml
[commands]
before   = ["mkdir -p src", "touch src/existing.py"]
rendered = ["python -m venv .venv"]
after    = [".venv/bin/pip install -e ."]
finally  = ["rm -rf .venv"]
```

| List | Runs | Sees |
|---|---|---|
| `before` | Before anything is rendered, in an empty sandbox. | Nothing but what it creates itself. |
| `rendered` | After the rendering is materialised onto the sandbox, before `expect` is checked. | `before`'s files, merged with the render. |
| `after` | After `expect` and the snapshot are checked. | The same merged sandbox. |
| `finally` | Always, last, regardless of anything above. | Whatever state the sandbox is in when everything else stops. |

### The sandbox is not a project, and rendering still never touches disk

Each case gets a fresh, throwaway temporary directory, created and destroyed by the test harness — never by a
template. It starts empty. `before` may seed it with files that stand in for "a project that already exists," and
the rendering, which still happens entirely in memory exactly as it always has, is afterward *materialised on top
of* whatever `before` left there rather than into a directory cleared first. That is what answers the second gap in
Context: a case can now seed a `pyproject.toml`, render, and assert the merged result — the shape `update` actually
produces, which `render --dirty -o` cannot.

Invariant 1 is unaffected by name: the worktree it guards is the *project's*, and there is no project here, nor has
there ever been one in `git tpl test`. A directory the harness creates and deletes for the duration of one case is
no more a worktree in that sense than the temporary clone `resolve::resolve` already makes of a remote template.

### The command format is a string, parsed without a shell

```toml
commands.before = ["mkdir -p src", "touch src/existing.rs"]
```

Each entry is word-split by `shlex` — quotes and backslash escapes honoured, nothing else — and spawned directly as
`argv[0]` with `argv[1..]`. There is no pipe, no glob, no redirection, and no `$VAR` expansion, and no shell process
sits in between. An author who wants a pipeline writes it into a file with `before` and names that file in
`rendered`; that is one explicit step more than a shell would need, and it is the step that keeps `sh -c` out of the
one place invariant 5 has never let it in.

### Gating: running `git tpl test` is the consent

Unlike the confirmation `[trust]`/`--trust` already asks for a template's remote *data* sources, running a case's
`[commands]` asks nobody. The two are unrelated capabilities. A data source reaches a network host the person
running the command may never have chosen to trust (ADR-013); a case's `[commands]` runs on the same machine, with
the same privileges, as the `test` process itself — no more dangerous than the test suite of any other repository a
person has cloned and decided to build. `git tpl test` on a template already means "I have this template in front
of me and am about to find out what it does," the same act as `git clone && make test` anywhere else. Extending the
data-source trust prompt to cover this would suggest a boundary that was never there to cross.

Commands run by default — opt-out, not opt-in — because a suite whose setup or teardown silently never ran would
report false confidence, which is worse than the alternative. `tpl.testCommands` (Git configuration, default
`true`) lets a person disable this for themselves; `--skip-commands` layers on top for one invocation and can only
turn commands off further, never force them back on once configuration has said no — the same one-directional shape
`tpl.interactive`/`--defaults` already has, and for the same reason: there is nothing to opt *into* here beyond what
running `test` at all already grants, only something to opt out of.

### Failure semantics

A nonzero exit, or a command that cannot even be spawned, is one fact about the case — `Failure::CommandFailed`,
carrying which list it came from, the exit code (`None` when the process never started or was signalled), and
captured stdout/stderr, capped well short of what a diagnostic report should ever carry in full.

`before`, `rendered` and `after` each stop at their own first failure. These lists are sequential setup and
assertion — `mkdir -p src` then `touch src/existing.rs` assumes the directory now exists — and running the rest
after a precondition failed would report noise, not information. A `before` failure, and a render that fails without
`expect.error` naming it, both skip straight to `finally`: there is nothing materialised for `rendered`/`after` to
run against.

`finally` is the opposite case, deliberately: every entry runs regardless of an earlier one failing, because it is
cleanup, and a container left running because the command that would have stopped it never got the chance is a
worse outcome than one more line in a failure report.

No timeout exists in this version. A command that hangs, hangs the run. This is a documented limitation, not an
oversight, and is left for a later ADR if it proves to matter in practice.

### `env` scopes a command's environment (added, issue #130)

Every entry in `[commands]` originally inherited the whole environment of the `git tpl test` process and nothing
more — there was no way to add or override a variable for one list, or one case, without exporting it for the
entire invocation and every case in it. A rendering that needs `pdm install` to ignore an already-active venv, say,
had no way to say so without either repeating a `env VAR=value` prefix on every entry in every case that needed it,
or setting the variable for the whole run and leaving every other case unable to tell, from its own file alone,
that it depended on it.

`commands.env` is a plain string map, merged into the inherited environment — never `.env_clear()` — of every
command in every list of the case. A single list may override it for itself alone by writing itself as a table
with `run` and `env` instead of a bare array; its own `env` wins over `commands.env` for a key both set, and every
other list in the case is unaffected either way.

None of the three exclusions above apply to it. It executes nothing itself, so invariant 5 is untouched. The
command format is unchanged — still a string, still word-split by `shlex`, still no `$VAR` expansion, since `env`
changes what a process is spawned *with*, never what its own argv line means. And it is scoped to the process
`Command` already spawns for a case's own `[commands]`, not a new surface: the one place this ADR already decided
`std::process::Command` may appear.

It is additive rather than a revision of the decision above, so it amends this ADR in place instead of a
superseding one: an old git-tpl reading a case with `commands.env`, or a list written as a table, fails loudly at
the existing vocabulary or shape check, not silently.

### `TEMPLATE_ROOT` exposes the resolved template's root (added, issue #134)

A case's sandbox is deliberately not the template repository — "The sandbox is not a project" above — but until
now that also meant a command had no way to reach back into the repository it was written in. Reproducing more
than a couple of lines meant synthesizing a script's content on every run via `before` (a `python3 -c '...'`
one-liner, say), because nothing named where the real checkout was; several cases sharing the same setup or
verification logic ended up repeating that dance once per case instead of writing one ordinary, reviewable script
file and pointing every case at it.

`TEMPLATE_ROOT` is set on every command in every list of every case to the resolved template's root on disk — the
working tree for `--dirty`, and the same working tree for a local `--ref`, since this command never resolves a
remote (ADR-030) and so never produces a separate checkout to point at instead. A case can now write
`"{{ env.TEMPLATE_ROOT }}/tests/scripts/check.sh"` directly, committed and reviewed like any other file in the
template — see "Commands are MiniJinja templates" below for why that is the form that actually works, rather than
the shell-style `$TEMPLATE_ROOT` this paragraph first proposed and issue #153 found never did.

Not `GIT_TPL_ROOT`, the name first proposed: this ADR's own case schema already uses `root` for the manifest's
declared render subdirectory, a different concept entirely, and reusing the word for a filesystem path would
invite exactly the confusion "one name per concept" exists to prevent.

None of the three exclusions this ADR opened with apply to it, for the same reasons `env` does not: it executes
nothing itself, so invariant 5 is untouched; it changes what a process is spawned *with*, never what its own argv
line means, so the command format is unchanged; and it is scoped to the one process `Command` this ADR already
permits, not a new surface. It is additive for the same reason `env` is, so it amends this ADR in place rather than
superseding it. Like `commands.env`, it is set before a case's own `env`/`commands.env` is applied, so a case that
deliberately sets the same key still wins — the same override a later, more specific value already has everywhere
else in this ADR.

### Commands are MiniJinja templates (added, issue #153)

The paragraph above always claimed `"$TEMPLATE_ROOT/tests/scripts/check.sh"` worked. It never did: "The command
format is a string, parsed without a shell" above is equally explicit that a command is only `shlex`-split, never
shell-expanded, so the literal text `$TEMPLATE_ROOT` reached `argv[0]` unchanged and the spawn failed with "No such
file or directory". The worked example this ADR itself gave contradicted the rule three paragraphs above it.

Fixed by rendering a command as a MiniJinja template — the engine every other string a template author writes is
already rendered by — before it is word-split, against two names:

- `env`, exactly what this command is about to be spawned with: the inherited environment, `TEMPLATE_ROOT`,
  `CLICOLOR_FORCE`/`FORCE_COLOR` when set, and this list's own `env`/`commands.env`, in that same order. `{{ env.X
  }}` in a command and `$X` inside the process it spawns can never disagree, because both are built from the one
  merged map.
- `answers`, the case's own `[answers]` table, unresolved — not `computed`, `data` or `template`, none of which
  exist until a render has actually run, so none of which `before` could ever see. `answers` alone is available to
  every list uniformly, because it is literally the case file's own text, not something evaluation produces.

This is not `$VAR` expansion: that sentence, three paragraphs up, stays true unmodified. A literal `$FOO` in a
command is still inert; only `{{ }}`/`{% %}` mean anything, and only because MiniJinja is asked to render the
string, exactly as it already renders a file's own body — no shell sits in between, no pipe, no glob, no
redirection is added, and invariant 5 is untouched for the same reason `env` and `TEMPLATE_ROOT` already are: this
changes what a string *is*, never what it does once it becomes `argv`.

A command with no `{{`/`{%` is never templated at all — the same `is_expression` check every other optional
templating site in git-tpl already uses — so every case written before this section existed keeps behaving
identically. One that does, and references a name that does not exist (`{{ answers.projct_name }}`, say), fails
that command with a message naming the mistake, rather than silently spawning `argv[0]` with an empty string
spliced in: MiniJinja's strict-undefined mode, not the lenient default a bare `{{ computed }}` in a rendered file
still gets. This is not the staged rollout ADR-014 describes for that asymmetry — there is no existing case
depending on a typo rendering to nothing here, because this capability did not exist until now.

## Consequences

**What reopens.** A test case's *harness* — not a template's render — may spawn a process. This is a new
capability at a new, narrow surface: the code path that runs `[commands]` is reached only by `git tpl test`, never
by `render`, `init` or `update`, and never touches the tree `render.rs` produces.

**What stays closed.** Invariant 5 is narrowed, not repealed: a template still cannot make `render`, `init` or
`update` execute anything, on any project, ever. What changes is that a test author, running a test command they
invoked themselves, may declare what proves their own template correct — the same trust a `Makefile`'s `test`
target already carries, extended nowhere else.

**What this is not.** It is not ADR-019's closure rule loosened. That rule governs built-in *Git* helpers reachable
from every render of every template — `git remote add`, and nothing that spawns a process, ever, by design.
`[commands]` is not a helper offered to every template; it is data a test author writes about their own template,
read only by the one command whose entire job is running a template's own declared checks. The two mechanisms
answer different questions, and neither one's bar applies to the other.

**On-disk contract.** `[commands]` is an addition to the case schema ADR-016 already governs, with the same
permanence. It needed an ADR of its own because it undoes a specific, named prohibition ADR-016 wrote down; it does
not touch the assertion vocabulary, the snapshot format, or anything else ADR-016 decided.

**Cost.** `shlex` moves from a transitive build dependency to a direct one. `std::process::Command` enters
`src/ops/testing.rs`, which is not `src/git/libgit2.rs` — deliberately and correctly: the `git-backend-isolation`
hook governs `git2::`, not process spawning in general, and no Git capability is involved here at all.
