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
