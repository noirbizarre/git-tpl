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

### `completion` and `man`

No success envelope, with or without `--json`. Their output is already a machine
format — a shell script and troff — and wrapping it in JSON would only mean
nothing could source or render it. Failures still carry the usual envelope.
