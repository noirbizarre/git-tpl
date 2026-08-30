# The Git model

Everything git-tpl does rests on one idea: **the rendered state of a template is a Git ref.**

## The shape of it

```
TEMPLATE GIT REPOSITORY
          │
          │ fetch / resolve
          ▼
     template tree
          │
          │ MiniJinja + resolved context
          ▼
    rendered template tree
          │
          ▼
    refs/tpl/<template-id>
          │
          │ normal Git merge
          ▼
     your working branch
```

Your branch and the template ref are two lines of history that periodically meet:

```
main:

    A ─── B ─── C ─── D ─── M
         /                 /
        /                 /
refs/tpl/foo:            /
    G0 ─────── G1 ─── G2
```

`G0`, `G1`, `G2` are successive rendered states of the template.
`M` is a `git merge` you ran.
There is nothing else — no sidecar state, no `.rej` files, no lockfile describing what was applied.

## Why the ref, and not the worktree

Because a merge needs a merge base.

If a tool writes rendered files straight into your worktree, it has no record of what the *previous* rendering
looked like, so it cannot tell your edits apart from the template's.
Every tool in this space then invents machinery to compensate: stored diffs, patch replay, three-way reconstruction
from a cached copy of the old output.

The ref *is* that record, kept in the only place that already understands history.
When `G1` and `D` both changed `Cargo.toml`, `git merge` finds `G0` — the state both descend from — and does
exactly what it does for two humans editing the same file.

## The ref is a normal ref

`refs/tpl/<id>` is not special. Every Git command works on it:

```sh
git show refs/tpl/github-com-noirbizarre-rust-library-template
git log --oneline refs/tpl/github-com-noirbizarre-rust-library-template
git diff HEAD refs/tpl/github-com-noirbizarre-rust-library-template
git merge refs/tpl/github-com-noirbizarre-rust-library-template
```

git-tpl's own `merge` command is a convenience over the last one, and `diff` over the merge preview described in
[`git tpl diff`](../usage/diff.md).
You are never locked into them.

It lives under `refs/tpl/` rather than `refs/heads/` on purpose: it is not a branch you check out, it should not
appear in `git branch`, and it should not be pushed by a bare `git push`.

## Jujutsu

"Every Git command works on it" is a claim [Jujutsu](https://jj-vcs.dev) (`jj`) tests directly, because a
**colocated** `jj git init` workspace (the default since jj 0.44) is a real `.git` directory with jj layered on
top.

Colocated works: git-tpl writes `refs/tpl/<id>` and asks for a `git merge`, both of which are exactly what they are
in any other Git repository, and jj re-imports the result on its own.

**Non-colocated** (`jj git init --no-colocate`) does not: there is no `.git` anywhere to discover, only a bare
backing store nested inside `.jj/`. git-tpl reports this specifically —
[`tpl::git::jj_not_colocated`](../reference/diagnostics.md#git) — rather than the generic "not a repository", and
points at `jj git init --colocate` as the fix.

A colocated workspace has sharp edges worth knowing, because they belong to jj's model, not git-tpl's:

1. `refs/tpl/<id>` is invisible to `jj log` and to any revset until it is merged into a ref jj imports
   (`refs/heads`, `refs/tags`, `refs/remotes`). There is no `jj`-native way to inspect it beforehand — use
   `git show`/`git log` on the ref directly, as above.
2. **Data-losing:** jj does not understand `MERGE_HEAD`. Running `jj commit` or `jj squash` while a merge is in
   progress silently drops the second parent — the merge base the *next* `git tpl update` depends on. Finish a
   merge with `git commit`, never with a jj command, until it is done.
3. jj snapshots conflict markers as an ordinary file modification, not a jj conflict, so `jj resolve` does not
   apply to them. Resolve with `git mergetool` or by hand, exactly as [`git tpl merge`](../usage/merge.md#conflicts)
   already describes for plain Git.
4. `git tpl init`'s attachment commit is written through the Git index (`.config/git.tpl.toml`). jj ignores the
   index but imports the resulting `HEAD` regardless, so it works — it just shows up unusually in `jj op log`.
5. git-tpl reads identity from Git configuration (`user.name`/`user.email`), not `jj config`. A jj-only user with
   neither set in Git config gets `tpl::git::no_identity`.

## Append-only

`git tpl update` always creates a *new* commit whose parent is the current tip of `refs/tpl/<id>`.
It never amends, never rebases, never force-updates.

This holds even when the reason for re-rendering is that you changed an answer rather than that the template
moved.
The template ref is history, and history that gets rewritten cannot be merged from twice.

The practical consequence: once you have merged `G1`, merging `G2` is cheap and conflict-free for everything you
did not touch, because `G1` is a genuine common ancestor.
Rewriting `G1` would destroy that.

If the rendered tree is byte-identical to the current tip, `update` makes no commit at all and says so.

Occasionally `update` adds two commits rather than one: a template [migration](../usage/update.md#migrations) that
moves a file gets a content-identical rename commit first, so that a later `git merge` — plain, never one git-tpl
runs — reliably sees a rename rather than an unrelated delete and add, and the ordinary rendered commit follows as
its child.
Both are ordinary commits on the same append-only history; see [ADR-024](../adr/024-template-migrations.md).

## What `init` does to your history

`git tpl init` creates `G0` as an **orphan** commit — no parent, because the template has no history in your
project before this moment — and then **merges it into your current branch**, allowing unrelated histories.

```
main:  A ─── M
            /
           /
       G0 ┘
```

That merge is the load-bearing step.
Without it, `G0` is not an ancestor of `main`, so the *first* `git tpl update` would have no merge base — and
without one, Git cannot tell your edits from the template's.
Every file that differs would conflict, including files you customised that the template never changed.
Which is exactly the failure mode this whole design exists to avoid.

`M` also carries `.config/git.tpl.toml`, the record of which template you attached and what you answered.
So an `init` is one commit on your branch, and `git show HEAD` shows the whole of it.
In an empty repository there is no `M` — the merge fast-forwards and `G0` *becomes* the branch — so the
configuration gets a small commit of its own; it cannot go into `G0`, which is the ref tip and must stay
byte-identical to the rendering.
See [ADR-021](../adr/021-attachment-in-the-merge-commit.md).

If you would rather wire it up yourself, `git tpl init --no-merge` stops after creating the ref.

This is also how a project that already has files joins a template.
There is no second command for it: the same merge runs, and Git reconciles the two sides by content rather than by
ancestry.
A file identical to the rendered one merges silently, a file you have edited conflicts only on the lines that
differ, and a file the template adds is staged for you.
See [an existing project](../usage/init.md#an-existing-project).

## What is in the commit

The tree is exactly the rendered output.
Nothing else — no manifest, no lockfile, no `.git-tpl/` directory.
A `git tpl diff` shows real file differences and nothing you would have to learn to ignore.

Provenance lives in the commit message, as trailers:

```
tpl: render rust-library at v1.4.0

Template-Source: https://github.com/noirbizarre/rust-library-template
Template-Ref: v1.4.0
Template-Commit: 4f2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29
Answers-Digest: sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
Data-Source: licenses = template:data/licenses.toml@4f2c1a9
Tpl-Version: X.Y.Z
```

The subject names the template's manifest `name` and the revision asked for.
`Data-Source` appears once per data source the rendering actually used — see
[Data sources](../data/index.md#provenance).

`git tpl status` reads them back. So can you:

```sh
git show --no-patch refs/tpl/github-com-noirbizarre-rust-library-template
```

## Sharing, or not

Rendered refs are never pushed implicitly. Both modes are first-class:

**Local-only.** The ref exists in your clone. Nobody else needs it. Nothing to configure — this is the default.

**Shared.** `git tpl push` publishes `refs/tpl/*` to the remote; collaborators run `git tpl fetch`. Useful when
several people run updates, or when CI does.

`git push` and `git pull` never touch `refs/tpl/*`, in either mode.
A contributor who clones the project and never runs git-tpl is unaffected by its existence.
