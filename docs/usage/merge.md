# `git tpl merge`

Detached `HEAD` supports both fast-forward and merge-commit updates, with the same safe-checkout protections.

Merge `refs/tpl/<id>` into the current branch.

Use [`git tpl pull`](pull.md) to render an update before merging in one invocation.

```sh
git tpl merge [--no-commit] [-m <msg>]
git tpl merge --abort
```

## It is a normal merge

```sh
git merge refs/tpl/github-com-noirbizarre-rust-library-template
```

Both merge the rendered ref into your branch.
`git tpl merge` looks up the ref name, uses a conventional message, and delegates merging to libgit2.
Its configuration support differs from native Git: it does not run configured external
`merge.<name>.driver` commands from `.gitattributes`.
Use plain `git merge refs/tpl/<id>` when your repository requires those drivers.

Which means:

- three-way merge, with the previous rendering as the base
- ordinary conflict markers in ordinary files
- `git merge --abort` works
- `git rerere` works
- your `merge.tool` works
- libgit2's built-in merge attribute behavior applies; configured external merge drivers require native Git

git-tpl implements **no** conflict resolution of its own.
Not a custom three-way merge, not patch replay, not rename detection, not heuristics.
Those exist in Git, they are better than anything this project would write, and reimplementing them is the
mistake this design exists to avoid.

## Success

```console
$ git tpl merge
Merged refs/tpl/github-com-noirbizarre-rust-library-template into the current branch
Merge commit 2fff441.
```

Fast-forwards when it can, and says so:

Fast-forward checkout protects tracked edits and untracked or ignored files obstructing incoming paths.
Unrelated local edits survive.
Checkout runs against the original HEAD before the branch moves; a refused checkout leaves the branch unpublished.

```console
$ git tpl merge
Fast-forwarded refs/tpl/github-com-noirbizarre-rust-library-template into the current branch
Now at 15b50a5.
```

With `--no-commit`, a non-fast-forward merge is staged and left for you to commit.
As with native Git, this prevents creating a merge commit, not fast-forwarding:
when a fast-forward is possible, `HEAD` advances immediately.

## Conflicts

`git tpl merge --abort` delegates to native `git merge --abort`, preserving unrelated local edits.
It refuses when no merge is in progress and retains Git's recovery state if Git cannot abort safely.
Native Git must be installed; there is no destructive reset fallback.

```console
$ git tpl merge

CONFLICT (content): Merge conflict in README.md

warning: automatic merge failed; fix conflicts and then commit the result.

  git status              see what conflicted
  git mergetool           resolve interactively
  git commit              finish
  git merge --abort       start over
```

The index is left exactly as Git leaves it.
Every tool you already use applies, because nothing here is special.

[`git tpl show <path>`](show.md) prints the template's version of a conflicted file, which is often the quickest
way to see what it was trying to say.

## Why conflicts are rarer than you expect

The merge base is your *previous* rendering.
So a file the template did not change in this update is not part of the merge at all, however much you edited
it — Git sees no change on one side.

A conflict means what it always means: you and the template both changed the same region of the same file since
the last time you agreed.
Which is genuinely ambiguous, and genuinely wants a human.

## Options

| Option | Meaning |
|---|---|
| `--no-commit` | Stage a non-fast-forward merge without committing. Fast-forwards still advance `HEAD`. |
| `-m`, `--message <msg>` | Override the merge commit message. |
| `--abort` | Abort an in-progress merge. Same as `git merge --abort`. |

## Machine-readable output

`git tpl --json merge` emits its outcome on stdout as a single JSON object, with the prose on stderr.
The payload is described in [JSON output](../reference/json.md#merge).
