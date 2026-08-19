# JSON output

`--json` is global. Every command accepts it and every failure emits the same
envelope — including the commands that have no success payload of their own.
Almost every command emits its payload to **stdout**; the three whose stdout is
already the payload are [`show`, `completion` and `man`](#show-completion-and-man).

Human output goes to stderr, so a piped `--json` stream stays parseable even
when the command is chatty:

```sh
git tpl --json questions ./template | jq '.questions[] | select(.when == null) | .name'
```

## The global flags

Four flags are accepted by every command, before or after the subcommand:

| Flag | Default | Meaning |
|---|---|---|
| `--json` | off | The payload on stdout as one object, as described here. Silences the prose narration, as `--quiet` does. |
| `-q`, `--quiet` | off | Suppress everything but errors and warnings. The exit code still says what happened. |
| `-v`, `--verbose` | off | More detail. Repeatable — `-vv` for more again. |
| `--color <auto\|always\|never>` | `auto` | `auto` colours only when stderr is a terminal. |

A warning is deliberately louder than the rest: neither `--quiet` nor `--json`
suppresses one, because a warning names something the caller is getting wrong
right now. Warnings go to stderr, so a JSON payload on stdout stays parseable.

## Failure

One shape, from every command:

```json
{
  "ok": false,
  "error": {
    "code": "tpl::render::collision",
    "message": "`a.jinja` and `b.jinja` both render to `x`",
    "help": "two template files cannot produce the same output file",
    "causes": [{ "code": "tpl::eval::expression", "message": "…", "help": "…" }],
    "labels": [{ "offset": 412, "length": 9, "label": "…" }]
  }
}
```

Branch on [`error.code`](diagnostics.md), never on `message`. `causes` carries
the chain: the outer error names the file, the one beneath names the reason,
and only the pair is actionable.

The exit code is unchanged — non-zero on failure, `2` from `status` when a
template update is pending. Reading the code to decide *what* went wrong and
the exit status to decide *whether* it did are different jobs.

## Success

Every payload carries `"ok": true`, so a caller can check one field without
first knowing which command it ran.

### `render`

```json
{ "ok": true,
  "template": { "name": "rust", "description": "…" },
  "revision": { "reference": "main", "commit": "a17b0b2…", "dirty": false },
  "output": "/tmp/out",
  "files": [{ "path": "Cargo.toml", "bytes": 412, "executable": false, "templated": true }],
  "ignoredAnswers": [],
  "skippedByGitignore": [] }
```

`templated` says whether the file went through MiniJinja or was copied
byte-for-byte. It is the only way to tell, from the output, that a workflow
full of `${{ }}` was copied rather than rendered-and-survived.

`skippedByGitignore` names the working-tree files a `.gitignore` kept out of a
`--dirty` render — always empty for a committed revision. Only paths a render
reads are listed: those under `root`, the `.jinja` partials outside it, and the
files named by declared data sources. A path that could never have been
rendered is not reported, however it is ignored. It is a report, not
an error: the render succeeded, and a caller comparing the file list against
its expectations needs to know why one is absent. See
[the authoring loop](../usage/render.md#the-authoring-loop).

### `lint`

```json
{ "ok": true,
  "template": "rust",
  "diagnostics": [{ "severity": "warning", "code": "tpl::lint::undeclared",
                    "message": "…", "help": "…", "path": "Cargo.toml.jinja",
                    "denied": false }],
  "errors": 0, "warnings": 1, "denied": 0 }
```

`ok` is about the command, not the template: warnings do not fail it unless
[`--deny`](../usage/lint.md#choosing-what-fails) says so. Check the exit code,
or `errors` and `denied` together.

`severity` is the rule's, `denied` is this run's policy. A `--deny` never
rewrites the severity, so `"severity": "warning", "denied": true` remains
distinguishable from a native error. `errors` and `warnings` count by severity;
`denied` counts the warnings promoted, and is what makes the exit code 1 when
`errors` is 0. An `--allow`ed finding appears nowhere and is counted nowhere.

### `questions`

```json
{ "ok": true,
  "template": { "name": "rust", "description": "…", "root": "template" },
  "questions": [{ "name": "crate", "order": 0, "type": "string", "prompt": "Crate name",
                  "help": null, "default": null, "defaultIsExpression": false,
                  "when": null, "pattern": "^[a-z0-9-]+$", "message": "…",
                  "defaultFrom": null }],
  "computed": ["lib_name"],
  "data": [{ "name": "targets", "source": "data/targets.toml", "kind": "template",
             "format": "toml", "sha256": null }] }
```

Questions come in **resolution order**, which is the order they must be
answered in when a `when` or a `default` references an earlier answer.

`defaultIsExpression` distinguishes `"{{ crate }}"` from a literal.
`defaultFrom` is the [machine-seeded default](../templates/questions.md#machine-seeded-defaults),
which pre-fills a prompt and is never the answer.

Three keys appear only when the question declares them: `choices`, an array of
`{ value, label, help }`; `choicesFrom`, the reference a `choices_from` names;
and `choicesResolved`, that reference's values, present only when it points at
a data file inside the template — which saves the caller fetching and parsing
it, and is why a remote source has no resolved form here.

### `context`

```json
{ "ok": true,
  "answers": {…}, "computed": {…}, "data": {…}, "template": {…}, "flat": {…} }
```

`flat` is what a template body sees: answers and computed values merged into
one table. `data` and `template` are not in it — they are siblings of it here,
and namespaces of their own in a template.

With `--eval`:

```json
{ "ok": true, "expression": "{{ x | length }}", "type": "an integer", "value": 3 }
```

### `status`

Documented in [status](../usage/status.md). `--json` is the only spelling:
the `--format json` it replaced was removed in 0.7.

### `diff`

```json
{ "ok": true,
  "conflicts": ["mise.toml"],
  "changes": [{ "path": "mise.toml", "kind": "modified", "insertions": 9,
                "deletions": 3, "binary": false }],
  "insertions": 9, "deletions": 3 }
```

`conflicts` names the paths a merge could not resolve on its own. They are
still reported as changes — the preview contains them with conflict markers,
which is what a merge would leave in the worktree.

### `init`

```json
{ "ok": true,
  "id": "rust", "ref": "refs/tpl/rust",
  "template": "https://github.com/noirbizarre/rust.tpl",
  "revision": "main (76ec0ea)", "commit": "a17b0b2…",
  "changes": [{ "path": "Cargo.toml", "kind": "added" }],
  "merge": { "result": "merged", "commit": "…" },
  "configPath": ".config/git.tpl.toml", "configCommitted": true,
  "ignoredAnswers": [],
  "note": "Next: run scripts/bootstrap.sh",
  "remotes": [{ "name": "origin", "url": "git@github.com:acme/demo.git",
                "status": "added", "existing": null }] }
```

`template` is the expanded URL, never the `mine:` shortcut that may have been
typed: it is what was recorded in the project, and a shortcut means nothing on
anyone else's machine.

`merge` is `null` under `--no-merge`, which is a different thing from a merge
that ran and found nothing to do (`{"result": "upToDate"}`).

`note` is the template's own note, `null` when it declares none. It is
**unsanitised** — escape sequences are a terminal's problem and this stream
reaches no terminal, so strip them yourself if you are going to print it.
Branch on its presence, never on its text: note prose is not a contract.

`remotes` lists the `[remotes]` a template declared, in declaration order, with
what became of each:

| `status` | Meaning |
|---|---|
| `added` | It was not configured, and now is. |
| `unchanged` | It was already configured with this URL. |
| `skipped` | A remote of that name exists with a *different* URL, and was left alone. `existing` names it. |

`url` is always what the template asked for, including when it was refused —
which is the case you most need it in. `existing` is `null` unless the two
disagree, so its presence is the signal. Both are `init`-only: `update` neither
adds a remote nor shows a note.

With `--dry-run` the payload is a different shape entirely, because nothing was
created and there is no ref, commit or merge to report:

```json
{ "ok": true, "dryRun": true,
  "template": "https://github.com/noirbizarre/rust.tpl",
  "revision": "main (76ec0ea)",
  "questions": [{ "name": "crate", "kind": "question", "supplied": false },
                { "name": "lib_name", "kind": "computed", "supplied": false }],
  "files": ["Cargo.toml"],
  "ignoredAnswers": [] }
```

`questions` is in resolution order and includes the `computed` and `data` nodes
the graph resolves alongside them, which `kind` distinguishes. `files` is
`null`, not `[]`, unless `--defaults` was passed: without it the list was never
computed, and producing it would mean asking the whole questionnaire — which is
what a dry run is avoiding. An empty array would claim the template renders
nothing.

### `update`

```json
{ "ok": true, "result": "upToDate",
  "id": "rust", "ref": "refs/tpl/rust",
  "template": "https://github.com/noirbizarre/rust.tpl",
  "revision": "main (76ec0ea)", "ignoredAnswers": [] }
```

```json
{ "ok": true, "result": "updated",
  "id": "rust", "ref": "refs/tpl/rust", "template": "…",
  "commit": "a17b0b2…",
  "previousRevision": "main (a1b2c3d)", "revision": "main (76ec0ea)",
  "changes": [{ "path": "Cargo.toml", "kind": "modified" }],
  "answersChanged": false, "startedNewHistory": false,
  "ignoredAnswers": [], "pushed": null }
```

Branch on `result`: `upToDate` or `updated`. It is the one thing the exit code
does not say — both succeed.

`startedNewHistory` is `true` when there was no `refs/tpl/<id>` to descend
from, so the new commit is an orphan sharing no ancestry with anything the
branch has merged. Two causes, both legitimate: the configuration's `source` or
`id` was edited, or the project was cloned without `refs/tpl/*` and never
fetched. Not an error, but a `git tpl merge` from here has no merge base and can
conflict on every file — fetch first if the ref exists on a remote.

`previousRevision` is `null` on the first rendering. `pushed` names the remote
when [`tpl.autoPush`](../usage/push.md) pushed automatically, `null` otherwise —
the push still happens under `--json`, only its prose is silenced.

With `--dry-run`, the same shape plus `"dryRun": true`, and `result` is
`upToDate` or `wouldUpdate`.

### `merge`

```json
{ "ok": true, "result": "merged", "commit": "a17b0b2…",
  "id": "rust", "ref": "refs/tpl/rust" }
```

`result` is one of `upToDate`, `fastForward`, `merged`, `staged`, `conflicted`
or `aborted`. `commit` accompanies `fastForward` and `merged`; `conflicts`
accompanies `conflicted`. This is the same object `init` reports under `merge`,
so a caller handles both with one function.

A conflicted merge is a success, not a failure: the index is left as Git leaves
it, for the user to resolve. `result` is how a caller finds out.

### `backport`

```json
{ "ok": true, "result": "patched",
  "template": "../my-template", "revision": "main (937573e)",
  "patch": "From 0000000…\nSubject: [PATCH] tpl: backport from my-service\n…",
  "output": null,
  "applyCommand": "git tpl backport | git -C ../my-template am",
  "files": [ { "rendered": "README.md", "source": "template/README.md.jinja",
               "insertions": 1, "deletions": 1, "added": false } ],
  "skipped": [], "unsubstituted": [], "insertions": 1, "deletions": 1 }
```

`result` is `patched` or `nothingToBackport`.

`patch` carries the mailbox itself, rather than it going to stdout beside the
payload: under `--json`, stdout is one JSON object, always. It is `""` when
there is nothing to backport. `output` is the `--output` path, or `null` when
the patch went to stdout.

`files[].source` is the path in the *template repository*, render root and
`.jinja` suffix included — the path the patch edits. `files[].rendered` is the
path in the project. `added` marks a file the template did not previously
produce.

`skipped` carries paths that were considered and deliberately not backported,
each with a `reason` — currently only files deleted locally, which would remove
them from every project rendering the template. Files the template never
produced are not listed: they are out of scope rather than skipped.

`unsubstituted` names every line whose template expression was reversed —
`path`, `source`, `line`, the `rendered` and `project` forms, the `patched`
source line, and the `expressions` it kept. It is empty unless `--unsubstitute`
was passed, since under `--json` there is nobody to confirm a reversal with.
Worth branching on: a reversed substitution changes what the template produces
for every project, and is the one part of a patch that should not be merged
unread ([ADR-022](../adr/022-backport-unsubstitutes.md)).

`applyCommand` is the `git am` invocation git-tpl declines to run
([ADR-020](../adr/020-backport-is-a-patch.md)), built from the configured
source. When the template is a URL it contains the literal
`<your-template-clone>`, because there is no local clone to name.

`-p` is refused under `--json` with `tpl::backport::not_interactive`: there is
nobody to show hunks to, and a flag that silently sent everything instead would
be the one answer that was not asked for. Limit the backport with pathspecs or
`--exclude`.

A refusal is a failure, with a `tpl::backport::*`
[code](diagnostics.md#backport). Branch on the code: `substituted_region` is
routine and means "edit the template by hand", while `stale_rendering` means
"run `update` first".

### `fetch`

```json
{ "ok": true, "remote": "origin", "ref": "refs/tpl/rust",
  "state": "behind",
  "relation": { "ahead": 0, "behind": 2, "synced": false, "diverged": false } }
```

`state` is one of `absent`, `synced`, `diverged`, `behind` or `ahead`, and
`relation` is `null` exactly when `state` is `absent` — the remote has no copy
of the ref, which is a different thing from a copy level with yours.

Fetching never moves the local ref, so `behind` is a report, not an action.

With `--dry-run` nothing is fetched and the payload says only what would be:

```json
{ "ok": true, "dryRun": true, "remote": "origin",
  "refspec": "+refs/tpl/*:refs/remotes/origin/tpl/*" }
```

### `push`

```json
{ "ok": true, "remote": "origin", "ref": "refs/tpl/rust" }
```

With `--dry-run` nothing is pushed:

```json
{ "ok": true, "dryRun": true, "remote": "origin", "ref": "refs/tpl/rust" }
```

`fetch` names a `refspec` and `push` a `ref`, because a fetch brings every
template ref and a push moves the one this project has.

### `test`

Documented with the command, at [`git tpl test`](../usage/test.md). The
payload carries `summary` and a `cases` array, each with its `failures`.

### `show`, `completion` and `man`

No success envelope, with or without `--json`. Their stdout is already the
payload — a rendered file's bytes, a shell script, troff — and wrapping it in
JSON would only mean nothing could read, source or render it. Failures still
carry the usual envelope.
