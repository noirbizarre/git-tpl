# ADR-019: A template may address the user; it never acts

**Status:** accepted

**Relates to:** [ADR-003](003-minijinja-only.md), [ADR-006](006-no-runtime-context.md),
[ADR-016](016-template-tests-are-data.md)

## Context

Every real-world template surveyed runs the same handful of commands after a first render: `git init`, a
dependency install, a hook install, `git add`, and `git remote add`.
Issue #32 asked whether git-tpl should run them — a bounded, confirmed, `init`-only task list that would supersede
the "no code execution from templates" non-goal without touching invariants 1 or 2.

The bar #32 set for itself was that the status quo — a template renders `scripts/bootstrap.sh` and the user runs
it — has to be *demonstrably worse in practice, not merely less tidy*.

It is not. Counting the five commands against what already exists:

| Command | Already served by |
|---|---|
| `git init` | `git tpl init --init` |
| `git add` | `ops::init` stages and commits the attachment |
| dependency install | nothing, and nothing that respects invariant 5 ever will |
| hook install | likewise |
| `git remote add` | nothing |

Three of five are done or permanently out of reach.
A task runner — with a trust model, a confirmation prompt, a rendering of each command, and a failure mode that
must leave the ref and the merge intact — would be built to serve one command, while every template needing an
install still ships `bootstrap.sh` anyway.
The mechanism does not pay for itself.

Two much smaller things were left over once that was clear, and they are what the surveyed templates actually
wanted:

- A template cannot **say anything** to the user. `git tpl init` prints a fixed epilogue; the "one line telling
  you to run `bootstrap.sh`" that the status quo depends on has, until now, had nowhere to come from.
- A remote's URL is genuinely template knowledge when it is derived from the answers —
  `git@github.com:{{ github_org }}/{{ project }}.git` is a fact about the template's conventions, not about the
  person running it.

## Decision

**Post-render tasks are declined.** Invariant 5 stands unamended: no subprocess, no shell, no eval, no hooks, at
render time or after it.
`docs/development/contributing.md` records the decline and points here.

In their place, one principle: **a template may address the user and may declare Git state, but it never acts and
never runs.**

### The note

Two manifest keys, mutually exclusive:

```toml
note = "Next: run scripts/bootstrap.sh"
note_file = "NEXT-STEPS.md"
```

`note`, not `message`. `[questions.<name>].message` already exists — it is the one that explains a `pattern` — and
TOML would quietly fold a top-level `message =` written after any table into that question.
Nothing could diagnose it, so the name is not available.

`note_file` names a path in the **template repository**, relative to its root, which is the namespace partials
already live in (ADR-012).
It is read from the template and never written into the project.
That is the distinction worth holding on to: a note is guidance, not an artifact.
A template that wants a durable file in the project renders one, and the note says to read it.

It is rendered if and only if the path ends in `.jinja` — the same rule the renderer applies to files.
Nothing is inferred from the content, so an author who wants interpolation names the `.jinja` and an author who
does not gets their braces back verbatim.
The path itself may be an expression, so a template can choose its note by the answers; one that renders to
nothing means no note, which is a template deciding to stay quiet rather than failing to speak.

Neither form is in the rendered ref, and neither is diffable by `update`.
An earlier draft of this decision claimed otherwise for `note_file`, on the strength of it naming a path in the
rendered tree.
That was true and it was the wrong design: it forced every template with something to say to ship a file into
every project generated from it.
The honest division is duller — `note` for a line, `note_file` for more than fits comfortably in a TOML string,
and for composing with the partials an author already has.

Shown on `init` only.
`update` staying a ref-only operation is most of its value, and a note tied to a *version* boundary is a different
feature — migrations — deliberately not this one.

### A missing note is fatal

The note is resolved **before** the ref is created, before the configuration is written and before the merge.
Nothing has been written to the user's repository at that point, so failing costs them nothing.

That ordering is what makes the strictness affordable.
A `note_file` naming nothing is an authoring mistake, and the alternative — succeeding and showing nothing —
leaves the author with an `init` that works and a note that never appears, which is the hardest kind of bug to
notice.
`git tpl lint` reports the same thing without a repository, so it is normally caught earlier still.

A binary `note_file` is refused for the same reason a binary partial is: replacement characters would look like
something was shown.

### The remotes

```toml
[remotes]
origin = "git@github.com:{{ github_org }}/{{ project }}.git"
```

Added on `init`, after the merge, through `GitBackend`.
Never fetched, never pushed: template refs stay explicit.

If the remote already exists with the same URL, nothing happens.
If it exists with a *different* URL, it is **skipped with a warning naming both**.
git-tpl does not repoint someone's `origin`; a template that could would be a template that could redirect a push.

### The closure rule

A builtin helper qualifies only if it:

1. is a Git operation expressible through `GitBackend`,
2. is idempotent,
3. touches no worktree file, and
4. spawns no process.

`git remote add` passes all four.
`npm install`, `prek install` and `gh repo create` fail (4), (4) and (1) respectively.
The set is closed and extended only by an ADR that supersedes this one — the same discipline ADR-003 applies to
filters, and for the same reason: a mechanism with no stated bar becomes the declined feature one verb at a time.

### Untrusted text reaches a terminal

This is the part that is genuinely new, and the part that costs something.

A template repository is untrusted input, and a note is the first time its author's bytes are written to the
user's terminal.
That is a larger surface than it looks.
Terminal escape sequences can write the clipboard (OSC 52 — planting `curl … | sh` for the user's next paste), move
the cursor and erase git-tpl's own preceding output, or reproduce `theme::command()`'s styling so that a line the
template wrote appears to be a line git-tpl wrote.

So the note is sanitised with an **allowlist**, never a denylist — a denylist cannot anticipate the next terminal
extension:

- **Kept:** SGR (`CSI … m`), for colour and text attributes; and OSC 8 hyperlinks whose target is `https`.
- **Dropped:** everything else. Cursor motion, erase, scroll regions, OSC 52 and all other OSC, DCS/APC/PM/SOS, C1
  bytes, `\r`, backspace, NUL.
- Every line is terminated with `SGR 0`, so styling cannot leak past the note into git-tpl's own output.
- Under `--json`, when the stream is not a terminal, or when `NO_COLOR` is set, everything is stripped and the
  note is plain text.

Formatting was kept rather than flattened because a note with no emphasis and no clickable link is a note people
stop reading, and the status quo it replaces — a README the user opens in a pager — has both.
The allowlist is what makes that affordable.

The note is additionally printed inside a delimited block attributed to the template.
The attribution is load-bearing, not decoration: it is what keeps a note from *claiming* to be git-tpl.
Sanitisation stops the mechanical attacks; the frame stops the social one.

### What the note is not

It is not an extension point.
git-tpl does not run, resolve, or verify anything the note names; a note that says "run `curl … | sh`" is exactly
as dangerous as a `README.md` that says it, and no more.
Nothing here reopens invariant 5.

It is not exposed in the render context.
`template.*` stays `name` and `description` (ADR-006), and a rendered file cannot read the note.

Its prose is not a contract.
ADR-016's "no message matching" extends here: no test and no `--json` consumer may pin note text.

## Consequences

The status quo survives where it was already right.
Templates that need an install still render `bootstrap.sh` — but they can now tell the user it exists, which is
the one thing they could not do, and which was the whole of the practical complaint behind #32.

Two manifest keys and one table are added.
They are optional, so existing manifests are unaffected and the change is additive rather than breaking.

`GitBackend` grows `remote_url` and `add_remote`.
ADR-011 is unaffected: both take and return our own types.

Invariants 1 and 2 are untouched, and their tests are untouched with them.
Remote URLs and the note are evaluated in `ops`, against the context the render already produced — never in
`render.rs`, and nothing they do can reach the tree.
`update` neither prints a note nor adds a remote.

The sanitiser is now security-relevant code, and is tested as such: one test per dropped class of escape, named
for the attack it prevents.
It lives in the library rather than in the binary's `theme` module, because it is a defence against untrusted
input and not a presentation choice.

`render -o` has no repository, so remotes are skipped there.
`git tpl lint` checks a literal `note_file` against the template repository, so an author finds a wrong path
without a repository and long before a user does.
A path containing an expression is skipped rather than guessed at: a lint has no answers, and a false error there
would make the rule unusable on exactly the templates that need it.

The one thing this costs is a strictness that has to be got right in the ordering rather than in the diagnostics.
`note_file` is resolved before the first write, and that is the only reason it may fail loudly at all.
Anything that later moves the resolution after the ref, the configuration or the merge silently converts a good
error into a bad one.
