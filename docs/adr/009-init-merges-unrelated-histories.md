# ADR-009: `init` merges the template commit into the branch

**Status:** accepted

## Context

`git tpl init` renders the template and creates `refs/tpl/<id>`. But the
generated files also have to reach the user's branch. Options:

1. **Check the files out** into the worktree and let the user commit them.
2. **Do nothing** — create the ref and print `git tpl merge`.
3. **Merge** the orphan template commit into the current branch, allowing
   unrelated histories.

Option 1 is what a generator does, and it is the trap. The files arrive on `main`
as a commit with no relationship to `G0`. When the first `git tpl update` creates
`G1` and the user merges it, Git looks for a common ancestor of `main` and `G1`
and finds none — so every difference between the generated files and the ones
the user has edited since comes back as a conflict, on a merge the user expected
to be routine.

That failure is the exact thing ADR-001 exists to prevent, and it would appear on
the *first* update, which is the worst possible first impression.

## Decision

`init` creates `G0` as an orphan commit on `refs/tpl/<id>`, then merges it into
the current branch with unrelated histories allowed.

```
main:  A ─── M
            /
       G0 ─┘
```

In a repository with no commits, `G0` becomes the first commit on the branch
directly — there is nothing to merge it with, and the result is the cleanest
possible history for a generated project.

`--no-merge` stops after creating the ref.

## Consequences

`G0` is an ancestor of `main`, so every subsequent update has a genuine merge
base. The first update behaves like the hundredth.

The history is honest: `git log --graph` shows the template entering the project
as a merged parent, which is what happened.

Nothing is hidden and no history is rewritten. The merge commit is a normal merge
commit.

The cost is that `init` requires a clean worktree, because it performs a merge.
That is the same requirement `git merge` has, and the error message says so.

Users unfamiliar with `--allow-unrelated-histories` may find the merge surprising
the first time. The documentation leads with it, because understanding it *is*
understanding the tool.

## A correction

Earlier revisions of this ADR said an unrelated-histories merge "treats every
file as added on both sides and conflicts on every line of every file". That is
wrong, and it mattered: it made attaching a template to an *existing* project
look impossible, and nearly bought a second command to work around a problem
that does not exist.

Git compares content, not ancestry. With no merge base it still runs a real
line-level diff, so a file identical on both sides merges silently, a file that
differs by one line conflicts on that one line, and a file only the template has
is simply added and staged. `tests/init.rs` now asserts all three — see
`a_file_that_differs_conflicts_only_on_the_differing_lines`.

The decision above is unchanged, because the merge base is still what makes the
*second* update cheap. Only the description of the cost was overstated.

The cost stated accurately: without a merge base Git cannot tell your edits from
the template's, so everything that differs conflicts — including a file you
customised that the template never changed. With one, only genuinely overlapping
edits conflict. That is demonstrated by
`without_a_merge_base_a_customisation_conflicts_the_template_never_touched`, and
its counterpart `resolving_the_first_merge_leaves_the_next_update_clean` shows
the same customisation surviving untouched once the base exists.
