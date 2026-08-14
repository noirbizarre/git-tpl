# `git tpl push`

Publish template refs to a remote.

```sh
git tpl push [--remote <name>]
```

Pushes `refs/tpl/<id>` explicitly:

```
refs/tpl/<id>:refs/tpl/<id>
```

## Why this is a separate command

Rendered refs are never pushed implicitly. `git push` does not push them, and
nothing git-tpl does changes what `git push` sends.

The two modes this gives you are both first-class:

=== "Local-only (default)"

    ```
    project
    ├── refs/heads/main      pushed
    └── refs/tpl/foo         local
    ```

    Nobody else needs it. The template attachment is fully described by
    `.config/git.tpl.toml`, which *is* pushed — so a collaborator can run
    `git tpl update` and render an identical ref for themselves.

=== "Shared"

    ```
    project
    ├── refs/heads/main      pushed
    └── refs/tpl/foo         pushed explicitly
    ```

    Useful when several people run updates, when CI does, or when you want the
    rendering history visible to the team.

## Never forced

```console
$ git tpl push
To github.com:acme/my-project
 * [new ref]  refs/tpl/rawtools-rust-library -> refs/tpl/rawtools-rust-library
```

## Divergence

```console
$ git tpl push

Cannot push refs/tpl/rawtools-rust-library: the remote copy has diverged.

  local   4f2c1a9  (2 commits not on the remote)
  remote  9a8b7c6  (1 commit not local)

Both were rendered independently. Reconcile them first:

  git tpl fetch
  git merge refs/remotes/origin/tpl/rawtools-rust-library
  git tpl push
```

There is no `--force`. A rendered ref is history that others may have merged
from, and overwriting it destroys the merge base their branch depends on — which
turns their next update into a whole-file conflict.

Reconciling is a merge, like everything else here. The rendered tree is
deterministic, so two renderings of the same template revision with the same
answers merge without conflict; if they *do* conflict, the answers genuinely
differ and that is worth seeing.

## Options

| Option | Meaning |
|---|---|
| `--remote <name>` | Default `origin`, or `tpl.remote`. |
| `--all` | Push every `refs/tpl/*`. |
| `--dry-run` | Report what would be pushed. |

## Pushing automatically after an update

```sh
git config tpl.autoPush true
```

Or per-invocation, `git tpl update --push`.
