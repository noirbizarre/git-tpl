# `git tpl push`

Publish template refs to a remote.

```sh
git tpl push [--remote <name>] [--dry-run]
```

Pushes `refs/tpl/<id>` explicitly:

```
refs/tpl/<id>:refs/tpl/<id>
```

## Why this is a separate command

Rendered refs are not pushed by default; `tpl.autoPush` or `git tpl update --push` opts in.
`git push` does not push them, and nothing git-tpl does changes what `git push` sends.

The two modes this gives you are both first-class:

=== "Local-only (default)"

    ```
    project
    ├── refs/heads/main      pushed
    └── refs/tpl/foo         local
    ```

    Nobody else needs it.
    The template attachment is fully described by `.config/git.tpl.toml`, which *is* pushed — so a collaborator
    can run `git tpl update` and render an identical ref for themselves.

=== "Shared"

    ```
    project
    ├── refs/heads/main      pushed
    └── refs/tpl/foo         pushed explicitly
    ```

    Useful when several people run updates, when CI does, or when you want the rendering history visible to the
    team.

## Never forced

```console
$ git tpl push
To github.com:acme/my-project
 * [new ref]  refs/tpl/github-com-noirbizarre-rust-library-template -> refs/tpl/github-com-noirbizarre-rust-library-template
```

## Divergence

```console
$ git tpl push

Cannot push refs/tpl/github-com-noirbizarre-rust-library-template: the remote copy has diverged.

  local   4f2c1a9  (2 commits not on the remote)
  remote  9a8b7c6  (1 commit not local)

Preserve both histories. No automatic reconciliation operation is available.
```

There is no `--force`.
A rendered ref is history that others may have merged from, and overwriting it destroys the merge base their
branch depends on — which turns their next update into a whole-file conflict.

Genuinely divergent rendered refs currently have no automatic reconciliation operation.
Merging a remote-tracking rendering into your project branch changes that branch, not the local rendered ref
that `git tpl push` publishes; it cannot repair the rejected push.
Preserve both histories for inspection. Do not force, reset, or fabricate a rendered-ref merge commit.
Equal rendered trees do not imply shared commit ancestry.

An absent local ref or one strictly behind the remote is different:
see [absent restoration](fetch.md#restoring-an-absent-local-ref)
and [conditional fast-forward adoption](fetch.md#fast-forwarding-an-existing-local-ref).

## Options

| Option | Meaning |
|---|---|
| `--remote <name>` | Default `origin`, or `tpl.remote`. |
| `--dry-run` | Report what would be pushed. |

## Pushing automatically after an update

```sh
git config tpl.autoPush true
```

Or per-invocation, `git tpl update --push`.

## Machine-readable output

`git tpl --json push` emits its outcome on stdout as a single JSON object, with the prose on stderr.
The payload is described in [JSON output](../reference/json.md#push).
