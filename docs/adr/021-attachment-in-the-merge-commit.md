# ADR-021: The attachment lives in the merge commit

**Status:** accepted

## Context

`git tpl init` renders the template into an orphan commit, points `refs/tpl/<id>` at it, merges that commit into
the branch (ADR-009), and records the template and the answers in `.config/git.tpl.toml` (ADR-010).

Until now the last of those was a commit of its own — `chore(tpl): attach the X template`, containing one file,
sitting on top of the merge.
So a single `init` put two commits on the user's branch.

Neither ADR-009 nor ADR-010 asked for that.
It came from an implementation constraint: libgit2 refuses to merge with a dirty index, so the configuration had
to be staged *after* the merge, and by then the merge commit was already written.
`docs/concepts/git-model.md` drew the merge and never mentioned the second commit, which is a fair sign of how
much it was worth.

## Decision

The configuration is staged into the merge's own index, before the merge tree is written.
`init` adds **one** commit to the branch.

`GitBackend::merge` takes the paths to stage for this, rather than the caller staging them itself — the index it
must go into is the one libgit2 populated during the merge, and that index exists only inside the merge.

Two cases keep a separate commit, because there is no merge commit to carry the file:

- **An empty repository.** The merge fast-forwards: the render commit *becomes* the branch (ADR-009). The
  configuration cannot go into that commit — it is the ref tip, and it must stay byte-identical to the rendering
  or an unchanged template would stop producing no commit (the determinism invariant).
- **`--no-merge`.** The user asked for no merge, so there is nothing to ride in.

And one case still commits nothing: a **conflicted** merge leaves the configuration staged, for the resolution
commit the user is about to make.

## Consequences

`git log --oneline` after an `init` shows one new commit, and `git show HEAD` shows the whole attachment: the
template's files and the record of where they came from.
That is one thing that happened, described in one place.

Reverting an `init` is `git revert -m 1 HEAD` rather than a revert of a commit and then of a merge.

The rendered ref is untouched by this.
`G0` still contains exactly the rendering, which is what the determinism invariant and every `update` after it
depend on.

The cost is that the two shapes differ: a normal `init` produces one commit, an `init` in an empty repository
produces two.
That asymmetry is real, and it is smaller than the alternative — putting a generated-looking file into the ref
would make the file both the input to and the output of the render, which ADR-010 rules out for reasons that
survive this change.
