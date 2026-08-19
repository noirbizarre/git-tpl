# ADR-015: Machine-readable output

**Status:** accepted; `--format` removed in 0.7

## Context

git-tpl defines several dozen diagnostic codes of the form
`tpl::<area>::<kind>` — see [the catalogue](../reference/diagnostics.md) for
the current set. They are
carefully chosen, they are stable, and until now nothing could read them: every
failure was rendered by miette to stderr as prose, and every failure exited 1.

A caller could tell that something went wrong. It could not tell *what*, except
by matching on message text — which is the thing codes exist to prevent, and
which makes every improvement to a diagnostic a breaking change for somebody.

Only `status` had `--format json`. It was the one command anybody had needed to
script, and the shape of that need was general.

## Decision

`--json` is a global flag. Every command emits its payload to stdout; every
failure emits one envelope, from every command, and still exits non-zero:

```json
{"ok": false, "error": {"code": "…", "message": "…", "help": "…", "causes": […]}}
```

### The cause chain is included

miette's `#[diagnostic_source]` chain is where the actionable detail lives.
`RenderError::Content` says only "failed to render `x`"; the `EvalError`
beneath it names the expression and the reason. Flattening them into one string
would leave a caller parsing prose after all, which is the problem being
solved.

### Codes, not exit codes

The alternative was distinct exit codes per category. It is strictly worse: an
exit code is a coarse enum that needs extending forever and can never be
precise, while `error.code` is already a vocabulary of several dozen terms that says
exactly what happened.

`SUCCESS`, `FAILURE` and `PENDING` are unchanged. Reading the code to decide
*what* went wrong and the exit status to decide *whether* it did are different
jobs, and neither substitutes for the other.

### `--format json` becomes `--json`

Two spellings for one idea, on a single command out of all of them, is not a
surface worth keeping. `--format` was hidden and warned on stderr through 0.5
and 0.6, and was removed in 0.7.

### stdout and stderr keep their jobs

Human output goes to stderr; data goes to stdout. That split predates this
decision and is what makes `--json` composable — a piped stream stays parseable
however chatty the command is. `--json` implies `--quiet` for prose, but
warnings still reach stderr: an ignored answer key or a `.gitignore` that
removed files is something the caller is getting wrong *now*, and swallowing it
under `--json` would silence it for precisely the audience least able to notice.

## Consequences

The codes are now a promised surface rather than an implementation detail.
`tests/diagnostics.rs` pins the set: adding a code without documenting it fails
CI, and so does removing one. `docs/reference/diagnostics.md` is the catalogue,
and says for each what a caller should do about it.

That test is the real cost of this decision, and it is the right one to pay:
a code nobody documented is a code nobody can branch on, and the JSON envelope
would be half a contract without it.
