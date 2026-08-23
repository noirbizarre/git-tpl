# ADR-024: A migration is a file, discovered by diffing the template's own history

**Status:** accepted

**Relates to:**
[ADR-002](002-no-custom-reconciliation.md),
[ADR-005](005-append-only-refs.md),
[ADR-008](008-provenance-in-trailers.md),
[ADR-019](019-templates-address-never-act.md)

## Context

ADR-019 gave a template a way to address the user — `note`/`note_file` — and scoped migrations out deliberately:

> Shown on `init` only.
> `update` staying a ref-only operation is most of its value, and a note tied to a *version* boundary is a different
> feature — migrations — deliberately not this one.

Issue #63 opened that feature.
Two things it asks for, independent of each other:

- A message an `update` can show, tied to a boundary the template crossed —
  "0.4 moved `src/config.rs`; your overrides need moving too."
- A path move applied to the rendered ref, so a rename produces a rename in the user's own history rather than a
  delete and an add that orphans their edits on the old path.

Neither exists today.
`Config`/`Manifest` have no notion of a version at all, and there is no mechanism to say "this update carries
something the last one didn't."

## The rejected shape: a declared version

The obvious design — a manifest declares `version = "0.4.0"` and an array of `[[migrations]]` keyed to it — was
rejected before being built.
Two problems, both practical:

1. **The moment a migration is *authored* is not the moment its version is *known*.**
   A fix that needs a migration is written against a template whose next release number has not been decided, and is
   very often backported to more than one.
   A version field invites writing the wrong one.
2. **Every other fact this design tracks is already a Git fact.**
   The template is a Git repository; "has it moved since I last rendered" is a question Git already answers, and
   `ops::update` already asks a form of it — the previous rendered commit's `Template-Commit` trailer (ADR-008)
   names the exact template commit the project was last rendered from.

Re-deriving a parallel numbering scheme on top of a system that already has one is exactly the kind of thing the
project's own premise argues against.

## Decision

**A migration is a file in `migrations/`, at the template repository root, sibling to `template.toml`. No template
declares a version, and no project records one.**

### Discovery is a tree diff, not a version comparison

`ops::update` already reads the previous rendered commit's provenance back, to print "updated from A to B".
That gives it, for free, the exact commit the project was last rendered from.
`update` now also asks: which files under `migrations/` exist in the tree at the newly resolved revision that did
not exist in the tree at that previous one?

```rust
let old_tree = template.commit_tree(old_commit)?;
let discovered = migration::discover_new(&template, old_tree, new_tree)?;
```

That is the whole mechanism.
`discover_new` is `diff_trees` scoped to `migrations/`, filtered to `Added`.
No version is compared because none exists; the template's own history between the two commits *is* the boundary.
A migration is "new" — and is shown, and is applied — at whichever `update` first makes that diff non-empty,
regardless of how many template commits separate the two renderings.
A project that jumps from a very old revision straight to the newest one still discovers every migration in
between, in one pass, because a diff between two trees does not care how many commits produced the difference.

This is also what keeps the noise down without any new state.
A migration cannot resurface once discovered: the next `update`'s "old tree" is the one that already contains it.

### The one rule this shape depends on: migration files are never deleted

A migration is discovered by its *presence*, not its *history*.
A file removed from the template repository disappears from every future diff, not just the ones after its
removal — a project that skipped both the commit that added it and the commit that removed it would never see it
at all.
`migrations/` is therefore append-only by convention, the same discipline Rails, Django and Alembic migration
directories already enforce, for the same reason.

### What a migration file declares

```toml
# migrations/2026-08-config-move.toml
message = "0.4 split `config.rs` into a module; your overrides moved with it."

[[moves]]
from = "src/config.rs"
to = "src/config/mod.rs"
```

`message`/`message_file` are `Manifest::note`/`note_file` again, one file down: mutually exclusive, resolved the
same way — `message_file` is repository-root-relative and rendered only if it ends in `.jinja` — and shown through
the identical `note::sanitise` and attributed block.
A migration is still only ever *addressing* the user, never acting; ADR-019's closure rule is untouched.

`moves` is new.
Each entry names a rendered **output** path — the same namespace `git tpl diff` reports — not a template source
path, and both sides are literal strings.
A move whose destination depends on an answer cannot yet be expressed; see "Explicitly out of scope" below.

### Moves are applied as a content-identical commit, ahead of the ordinary one

The project's premise (`docs/concepts/git-model.md`) is that **the user merges with plain `git merge`** — never one
git-tpl runs or configures (ADR-002).
Git's own rename detection is a similarity heuristic over blob content; it is reliable only when the old and new
blob are still close to identical.
A move that lands in the same commit as an unrelated content rewrite of the same file risks Git seeing an ordinary
delete and an unrelated add, which loses the user's edits on the old path during the merge rather than carrying
them across the rename.

So `update` builds the rename as its own step, before the ordinary rendered commit:

1. Take the ref's current tip tree.
   For every declared move, drop the entry at `from` and re-insert it at `to`, **keeping its blob `oid` untouched**.
   That is the entire mechanism — no new `GitBackend` primitive, because a move is nothing but a different flat
   list passed to the tree builder that already exists.
2. If that tree is exactly what the fresh rendering already produces, it *is* the final commit: no rename step is
   needed, because the ordinary commit already is a pure rename.
3. Otherwise — a move landed alongside some other change in the same update — commit the renamed tree first, as a
   child of the previous tip.
   Content is byte-identical to its parent except for the moved paths, so Git's default similarity detection sees
   the closest thing to certainty a heuristic can see.
   The ordinary rendered commit, with whatever else changed, follows as its child.

The intermediate commit carries no provenance trailers — it is superseded within the same `update` call, and no
future `update` ever reads it as a ref tip.
Both commits are ordinary, unremarkable objects; a plain `git log` on the ref shows one more commit than usual, and
nothing else distinguishes it as special.

### A migration forces a commit, even when nothing else changed

`update`'s determinism guarantee — identical rendering, no commit — stands for everything except this one case.
A migration with a message and no content effect on the rendering (a text-only note) would otherwise never advance
the previous rendered commit's `Template-Commit` trailer, and the same migration would surface again on the very
next `update`.
Discovering a migration therefore always produces a commit, so the trailer advances past it and the diff in the
next `update` is empty again.
This is the one piece of extra state the whole design needs, and it is not new: it reuses the trailer ADR-008
already writes on every commit.

## Explicitly out of scope

- **`from`/`to` are literal, not expressions.**
  A template whose output path varies by answer cannot yet declare a move for it. Extending `moves` to Jinja is a
  small, additive change if it turns out to matter; it is left out now because nothing in the surveyed cases needed
  it, and a literal path is what `apply_moves` can validate without a project at lint time.
- **No `git tpl test` assertion for a migration.**
  ADR-016 keeps the test vocabulary closed and free of anything that needs two resolved revisions at once;
  asserting "this migration moves X to Y" would need exactly that. Left for a future ADR if it is wanted.
- **`git tpl status`/`diff` gain no rename awareness.**
  They report a move as the delete-and-add it structurally is, exactly as `update`'s own change list does today.
  Only the ref's own commits change shape; nothing about how git-tpl reports a diff does.

## Consequences

`migrations/` is a new, optional convention in a template repository.
No existing manifest is affected: a template with no such directory behaves exactly as before, because
`discover_new`'s diff is then always empty.

`.config/git.tpl.toml` and `template.toml` are both untouched.
Nothing was added to either, which is the point: this is the feature that could have needed a schema change to
both, and needs one to neither.

`update` can now write two commits instead of one or zero.
`tests/update.rs` gains a case for it; the single most important existing test —
`update_does_not_touch_head_the_index_or_the_worktree` — is unaffected, because both commits are built the same way
every other rendered commit is: as a Git object, with one ref moved at the end.
Invariant 1 does not distinguish "how many objects were written" from "was the worktree touched."

`git tpl lint` validates a migration file's shape — TOML, `message`/`message_file` not both set, a `message_file`
that names something, no move to an empty or self-identical path — without a project, the same guarantee
`missing_note_file` already gives `note_file`.
What it cannot check — whether a declared `from` exists in some project's previously rendered tree — is refused by
`update` itself, loudly, before any commit is written.
