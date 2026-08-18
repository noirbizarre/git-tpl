# ADR-020: `backport` emits a patch, and proves it by re-rendering

**Status:** accepted, amended by [ADR-022](022-backport-unsubstitutes.md) and
[ADR-023](023-hunk-selection-precedes-the-proof.md)

**Relates to:** [ADR-002](002-no-custom-reconciliation.md),
[ADR-006](006-no-runtime-context.md)

## Context

A user fixes something in a generated project. Today the only route back to the
template is by hand: open the template, find the `.jinja` source, retype the
change, remember which literals were substituted. `git tpl backport` produces
the patch instead.

Two questions had to be settled before any code, because both are unrecoverable
once people have patches in flight.

### Which direction the diff runs, and against what

Not "project versus template". The template tree at the pinned revision is not
comparable to the working tree — one is `.jinja` sources, the other is output.
The only meaningful comparison is between two *rendered* trees: the tree the
template would produce at the recorded revision, which `refs/tpl/<id>` already
holds, and the project's working tree. The difference between them is the
user's local divergence, which is the thing worth sending upstream.

### How a rendered hunk becomes a source hunk

This is the hard part. A line in a rendered file is not a line in the template
source. `README.md` says `# acme-api`; `README.md.jinja` says
`# {{ project_name }}`. Sending the first upstream as if it were the second
replaces a placeholder with one user's answer, for everyone.

The obvious approach is a substitution table: collect the resolved answers and
computed values, and in each changed region replace an occurrence of a value
with the `{{ name }}` that produced it. Refuse the cases known to be dangerous
— two names resolving to the same value, a value below some minimum length, a
hunk falling inside a `{% if %}` region the render collapsed.

That list is a blacklist, and the failure it is guarding against is exactly the
failure a blacklist does not catch. If the answer is `author = "June"` and the
template happens to hard-code the word "June" in an unrelated sentence,
un-substituting it produces a template that renders correctly for this user and
wrongly for everyone else. No rule in the list fires. The patch looks right,
reviews as right, and is wrong.

A wrong un-substitution ships a broken template to every downstream project.
That is strictly worse than "do it by hand", which is the status quo and
therefore the floor: any refusal is acceptable, because the user is left
exactly where they started.

Three options:

1. **Heuristic refusal list.** Rejected above. It cannot distinguish a
   substitution from a coincidence, because at the level of bytes there is no
   difference between them.
2. **Track provenance through the renderer.** Instrument MiniJinja so each
   output byte range records the expression that produced it. Exact, and
   invasive: it means a fork of, or a shadow evaluator alongside, the one
   template engine (ADR-003), for one command.
3. **Verify rather than infer.** Produce a candidate patched source, render it,
   and compare.

## Decision

Option 3, narrowed by a scope restriction that makes option 1's hard cases not
arise in the first place.

**Backport is verified, not inferred.** For each candidate file, with `S` the
template source, `R` the rendered output and `P` the project's version:

1. Align `S` against `R` line by line.
2. Require every line the `R`→`P` change touches to fall inside an aligned
   *equal* run — a region where source and output are byte-identical, so no
   substitution occurred in it.
3. Map the change through the alignment onto `S`, giving `S'`.
4. **Render `S'` with the same context and require the result to equal `P`.**

Step 4 is the decision. Rendering is deterministic (invariant 2): the same
template revision, the same answers and the same data produce byte-identical
output. So a successful re-render is not a smoke test but a proof that the
patched source produces, for this user, exactly what they have. It disposes of
the collapsed-`{% if %}` class outright, without needing to detect such regions
at all — a change misplaced into a conditional region does not round-trip.

Step 2 is the scope restriction: because no touched line contained a
substitution, backport never *invents* a `{{ }}`. It moves text that was
already verbatim. The coincidental-substring failure above cannot occur,
because no substitution is reversed. Files copied byte-for-byte are the
degenerate case of this, and back-port entirely.

**Backport does not apply the patch.** It writes a mailbox to stdout or to
`--output`, and stops. It never writes to the template repository, never
commits, never pushes. Two reasons, both structural:

- A template resolved from a remote is a throwaway clone
  (`resolve::Resolved`, backed by a `TempDir`). Writing into it writes into a
  directory that is about to be deleted.
- Applying a patch is reconciliation, and ADR-002 says git-tpl contributes no
  reconciliation logic of its own. `git am` already does it, in the repository
  where the user can review, resolve and abort.

`git am` reads a mailbox from stdin, so the composition needs no help from us:

```sh
git tpl backport | git -C ../my-template am
```

A `--to <path>` flag was therefore declined. It could only be implemented by
spawning `git` — which nothing in this tree does — or by adding
`GitBackend::apply_patch` over `git2`, which is ADR-002's subject matter and
buys a *weaker* `git am`: no three-way fallback, no `--abort`/`--skip`, and
conflicts reported outside the repository where they would be resolved. What
git-tpl does instead is print the exact command it declines to run, built from
the configured source when that is a local path.

## Consequences

Backport either produces a patch that is correct for the user who ran it, or
refuses with a named code. There is no third outcome, and in particular no
plausible-looking wrong patch. `tpl::backport::substituted_region` is expected
to be common, and its diagnostic names editing the template by hand as the
fallback — the same work the user would have done anyway, minus the search for
which file to open.

Backport reads no new Git capability. Detection compares two trees that already
live in the project repository, so libgit2 supplies pathspec matching. Emission
formats a unified diff in process with `similar`, the way `git tpl test`
already does and for the same reason: `GitBackend::diff_patch` needs two
*trees*, and producing them would mean writing objects to answer a question
that reads nothing. `GitBackend` gains no method, so ADR-011 is untouched and
`apply_patch`, `cherry_pick` and cross-repository commit stay out of the trait.

`render::Rendered` gains the source path it came from. `render_entries` already
computed that mapping to detect collisions and discarded it; keeping it is the
only possible inverse of `render_path`, which cannot be inverted by
recomputation because a segment rendering empty erases its subtree.

The command re-renders the recorded revision rather than trusting the ref tip,
and refuses if the two trees disagree. That costs a render, which is needed
regardless for the source bytes and the context, and it catches the case where
the recorded answers no longer reproduce the recorded tree — after which every
line number in the alignment would be wrong.

The accepted cost is reach. A change to a templated line is refused today, and
the fix a user most wants to send upstream may well be on one. Extending to
substituted regions is a strictly additive change and does not need this ADR
revisited: step 4 already stands ready to verify a reversed substitution, and
option 1's refusal list becomes admissible once it is a *filter in front of a
proof* rather than the proof itself. What this ADR forecloses is shipping that
inference without the verification behind it.

!!! note "Amended by ADR-022"

    That last paragraph is the one part of this record that did not survive
    contact with the code. The extension shipped, but not as option 1 in front
    of a proof: a table's question — "does this text come from that value?" —
    has no answer at the level of bytes, so no refinement of its refusal list
    refines the *answer*. [ADR-022](022-backport-unsubstitutes.md) establishes
    provenance by re-rendering the source line instead, which is option 2
    reached without forking MiniJinja, and adds a human confirmation for the
    one thing step 4 cannot prove: that a patch right for *this* user's answers
    is right for everyone else's.
