# ADR-011: `git2` is confined to one module, enforced by a hook

**Status:** accepted

## Context

libgit2 is the Git backend (gitoxide is explicitly out of scope).
We would still like to be able to replace it later without rewriting the domain, rendering and CLI layers.

The standard answer is a trait.
The standard failure is that a trait alone enforces nothing: one `use git2::Oid` in `render.rs`, a
`git2::Repository` in an `ops` signature, and the abstraction becomes decorative — with nothing failing until
someone attempts the swap years later and discovers the backend is everywhere.

A crate boundary would enforce it, but ADR-004 chose a single crate.

## Decision

A `GitBackend` trait in `src/git/mod.rs` defines the operations.
Domain code uses *our* types — `Oid`, `TreeEntry`, `Signature` — never `git2`'s.

The confinement is enforced by a prek hook:

```
! grep -rnP --include=*.rs "(?<![A-Za-z0-9_])git2::" src \
  | grep -v "^src/git/libgit2.rs:"
```

The lookbehind is load-bearing.
A plain `git2::` also matches `libgit2::`, which every module legitimately names when it constructs a backend —
the first version of this hook failed on day one, for the wrong reason.

`src/git/libgit2.rs` is the only file permitted to name `git2`.

## Consequences

The boundary is real, and violating it fails at commit time with an obvious message — rather than at some future
refactor.

This is stricter than a crate boundary, which would permit `pub use git2::Oid` and re-export the leak.
And it is one line.

Domain code is testable without constructing a real repository, because it depends on a trait.

The cost: our types must be converted at the boundary, which is a small amount of mechanical mapping in
`libgit2.rs`.
And we must not fall for the temptation to expose a `git2` type "just this once" in a signature — which is what
the hook is for.

No second backend is implemented, and none is planned.
The point is to keep the option open cheaply, not to build for it.
