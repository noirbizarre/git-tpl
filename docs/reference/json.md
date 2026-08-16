# JSON output

`--json` is global. Every command accepts it, every command emits its payload
to **stdout**, and every failure emits the same envelope — including the
commands that have no success payload of their own.

Human output goes to stderr, so a piped `--json` stream stays parseable even
when the command is chatty:

```sh
git tpl --json questions ./template | jq '.questions[] | select(.when == null) | .name'
```

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
                  "default": null, "defaultIsExpression": false, "when": null,
                  "pattern": "^[a-z0-9-]+$", "message": "…" }],
  "computed": ["lib_name"],
  "data": [{ "name": "targets", "source": "data/targets.toml", "format": "toml" }] }
```

Questions come in **resolution order**, which is the order they must be
answered in when a `when` or a `default` references an earlier answer.

`defaultIsExpression` distinguishes `"{{ crate }}"` from a literal.
`choicesResolved` appears when a `choices_from` points at a data file inside
the template, saving the caller from fetching and parsing it.

### `context`

```json
{ "ok": true,
  "answers": {…}, "computed": {…}, "data": {…}, "template": {…}, "flat": {…} }
```

`flat` is what a template body sees: answers and computed values at the top
level, `data` and `template` namespaced.

With `--eval`:

```json
{ "ok": true, "expression": "{{ x | length }}", "type": "an integer", "value": 3 }
```

### `status`

Documented in [status](../usage/status.md). `--format json` is deprecated in
favour of `--json`; it still works, and warns, for one more minor release.

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
  "ignoredAnswers": [] }
```

`template` is the expanded URL, never the `mine:` shortcut that may have been
typed: it is what was recorded in the project, and a shortcut means nothing on
anyone else's machine.

`merge` is `null` under `--no-merge`, which is a different thing from a merge
that ran and found nothing to do (`{"result": "upToDate"}`).

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
  "answersChanged": false, "ignoredAnswers": [], "pushed": null }
```

Branch on `result`: `upToDate` or `updated`. It is the one thing the exit code
does not say — both succeed.

`previousRevision` is `null` on the first rendering. `pushed` names the remote
when [`tpl.push`](../usage/push.md) pushed automatically, `null` otherwise —
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

### `push`

```json
{ "ok": true, "remote": "origin", "ref": "refs/tpl/rust" }
```

### `show`, `completion` and `man`

No success envelope, with or without `--json`. Their stdout is already the
payload — a rendered file's bytes, a shell script, troff — and wrapping it in
JSON would only mean nothing could read, source or render it. Failures still
carry the usual envelope.
