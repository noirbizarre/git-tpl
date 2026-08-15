# `git tpl merge`

Merge `refs/tpl/<id>` into the current branch.

```sh
git tpl merge [--no-commit] [-m <msg>]
git tpl merge --abort
```

## It is a normal merge

```sh
git merge refs/tpl/github-com-noirbizarre-rust-library-template
```

That is the operation. `git tpl merge` looks up the ref name and uses a
conventional message; everything else is libgit2's merge, which is Git's merge.

Which means:

- three-way merge, with the previous rendering as the base
- ordinary conflict markers in ordinary files
- `git merge --abort` works
- `git rerere` works
- your `merge.tool` works
- `.gitattributes` merge drivers work

git-tpl implements **no** conflict resolution of its own. Not a custom
three-way merge, not patch replay, not rename detection, not heuristics. Those
exist in Git, they are better than anything this project would write, and
reimplementing them is the mistake this design exists to avoid.

## Success

```console
$ git tpl merge
Merged refs/tpl/github-com-noirbizarre-rust-library-template into the current branch
Merge commit 2fff441.
```

Fast-forwards when it can, and says so:

```console
$ git tpl merge
Fast-forwarded refs/tpl/github-com-noirbizarre-rust-library-template into the current branch
Now at 15b50a5.
```

With `--no-commit`, the merge is staged and left for you to commit.

## Conflicts

```console
$ git tpl merge

CONFLICT (content): Merge conflict in README.md

warning: automatic merge failed; fix conflicts and then commit the result.

  git status              see what conflicted
  git mergetool           resolve interactively
  git commit              finish
  git merge --abort       start over
```

The index is left exactly as Git leaves it. Every tool you already use applies,
because nothing here is special.

## Why conflicts are rarer than you expect

The merge base is your *previous* rendering. So a file the template did not
change in this update is not part of the merge at all, however much you edited
it — Git sees no change on one side.

A conflict means what it always means: you and the template both changed the
same region of the same file since the last time you agreed. Which is genuinely
ambiguous, and genuinely wants a human.

## Options

| Option | Meaning |
|---|---|
| `--no-commit` | Merge and stage, but do not commit. |
| `-m`, `--message <msg>` | Override the merge commit message. |
| `--abort` | Abort an in-progress merge. Same as `git merge --abort`. |
