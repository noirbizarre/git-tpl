# ADR-034: A template may extend one parent, pinned, merged by name

**Status:** accepted

**Relates to:** [ADR-005](005-append-only-refs.md) (a parent revision is a pinned Git reference, the same rule),
[ADR-007](007-static-dependency-graph.md) (the merge happens before the graph is ever built),
[ADR-008](008-provenance-in-trailers.md) (the new trailer), [ADR-012](012-template-loader.md) (the `parent:`
loader namespace this decision fulfils).

## Context

Template inheritance changes `template.toml`, which is a contract, and changes what "the template" means for
provenance and for `refs/tpl/<id>`. It needs an ADR before it needs code.

The motivating case is an organisation with fifteen templates that all render the same CI workflow, the same
licence header, the same contributing guide — copied fifteen times, and fixed fifteen times whenever one of them
was wrong. What is wanted is a `base` template the other fifteen extend, adding their own questions and files and
overriding the handful of things that are actually specific to them.

```toml
# template.toml
[extends]
source = "https://github.com/org/base-template"
rev = "v3.1.0"
```

The child renders on top of the parent: it may add questions, data sources and computed values, override any of
them by name, add files, and replace files the parent renders.

**The parent must be pinned, and this is not negotiable.**

`rev` is required, and it must resolve to an immutable revision — a tag or a commit, never a branch. An unpinned
parent means the same child revision renders two different trees on two different days, and then "an unchanged
template produces no commit" is false, `update` produces mystery diffs, and invariant 2 has been traded for a
convenience. `docs/data/reproducibility.md` already states this rule for Git-hosted data — "a tag or a SHA is a
pin; a branch is not" — and a template's parent is exactly that same kind of reference, read the same way, so it
follows the same rule rather than inventing a second one.

Checking it needs new capability, though: a tag and a branch can be spelled identically, and nothing about the
string `"v3.1.0"` says which one it is until the actual repository is asked.

**Provenance must record the whole chain.** Without it, `git tpl status` on a project cannot say what it was
actually rendered from, and ADR-008 exists precisely so that question always has an answer.

**Merge semantics, one rule, applied everywhere:** the unit of override is the name. A child's `[questions.x]`
replaces the parent's entirely; it does not merge field by field. Field-level merging is the friendlier-looking
choice and the wrong one — a child that means to change a default would silently inherit a `when` clause it never
read, and debugging that means reading two repositories. Replacement costs the author four lines of copying and
costs the reader nothing. It applies uniformly to `[questions]`, `[computed]`, `[data]` and, by the same
reasoning even though the issue that proposed this ADR did not name it explicitly, `[remotes]` — all four are
name-keyed declarations with no field a partial merge would preserve safely.

**`{% extends %}` and the loader.** ADR-012 registered a loader backed by the template's own tree and left the
`<prefix>:` namespace free "for template inheritance, where a child must be able to name its parent's file
explicitly (`{% extends "parent:base.html.jinja" %}`) rather than resolve to itself." Inheritance needs the same
loader backed by layered trees.

The obvious reading of "layered trees" — resolve a bare name within whichever template's file is asking, and
`parent:` steps up exactly one layer from there — runs into a real constraint: MiniJinja's loader is a flat
`name -> content` function with no notion of which template is asking. Honouring that reading exactly needs a
fresh loader scoped to each file's own origin layer, rebuilt every time a `parent:` boundary is crossed, and even
then a `parent:`-reached file that itself makes a further *bare* reference to one of its own siblings cannot be
resolved correctly without rewriting import statements at the text level — real engineering for a case genuinely
rare in template inheritance's own motivating use (Jinja-style block inheritance, one level).

**Scope for v1, deliberately small:** a single chain, one parent per template. No multiple inheritance and no
diamonds — both raise a resolution-order question with no obvious right answer, and neither is needed by the case
that motivates this. Cycles are detected up front and by name. The chain has a depth limit.

**It multiplies a known rough edge.** Every ancestor is cloned on every run, so a three-deep chain is three
clones. The template cache is currently "stays until it actually hurts"; this is what makes it start to.

## Decision

### The manifest

```toml
[extends]
source = "https://github.com/org/base-template"  # required
rev = "v3.1.0"                                    # required; a tag or a commit SHA, never a branch
remove = ["template/.github/workflows/ci.yml.jinja"]  # optional, parent-repository-relative
```

`rev` is required — there is no default meaning "the parent's default branch," because that default branch is
itself unpinned. `remove` names paths relative to the *parent's own repository root*, not the render root, so a
child can also remove an inherited partial (which lives outside the render root by definition). Removing a path
the parent does not have is an error: otherwise a rename upstream silently resurrects a file the child spent a
release removing, and nobody notices until it ships.

`name`, `description`, `root`, `strict`, `note` and `note_file` are **per template, not inherited** — the same
treatment as `name`/`description` already had before this ADR, extended to the rest of the manifest's
non-name-keyed scalars for the same reason: `root` says where *this* layer's own files live, and `note` says
something about *this* rendering. A child that wants its parent's note copies it; there is no partial answer that
reads as anything but confusing when the child's own `note` silently was the parent's.

### Pinning is checked, not assumed

`rev` is accepted as pinned when it looks like a hex commit SHA (`^[0-9a-f]{7,40}$`) or names an existing tag in
the parent repository. Anything else — a branch name, or a name that resolves only through the parent's default
branch — is rejected before the parent is ever merged in. This needs one new capability on `GitBackend`,
`is_tag`, since `resolve_revision` already resolves a tag, a branch or a SHA to the same `Oid` and deliberately
does not say which kind of reference it matched.

### Merge order

Ancestors' questions, computed values, data sources and remotes come first, in ancestor order (root ancestor
first), then the child's own new ones. An *overridden* entry keeps the **position of its first declaration** —
inserting an override does not reshuffle the prompt sequence, and does not move a data source's evaluation point
relative to entries that do not reference it.

A name declared as one *kind* by one layer (a question) and a different kind by another (a computed value) is an
error, checked once the whole chain is merged — the same ambiguity `tpl::manifest::name_collision` already
catches within one manifest, now caught across a chain too.

### A layer need not be self-sufficient

Resolving a single template has always required its `root` subtree to exist — an empty render root is almost
always a typo in `root`, not a template with nothing to say, and `tpl::resolve::missing_root` catches it before
anything is asked. `[extends]` needs one exception: a layer that declares `[extends]` may have **no files of its
own at all**, root subtree included, because it may exist purely to add a question, override a data source, or
add a remote — contributing zero files is the legitimate shape of "I only change the pieces I named." Such a
layer's absent root is read as the empty tree rather than an error. A template with no `[extends]` keeps the
strict reading unchanged: nothing here loosens what an ordinary, non-inheriting template already had to satisfy.

### File layering

Each layer renders its own tree from its own `root`. The merge happens on the *pre-render, per-layer* path: a
child's `template/README.md.jinja` replaces a parent's file at the same pre-render path entirely, and a parent's
file the child does not mention is included unchanged. This is decided before path templating ever runs, so the
existing collision check in `render_entries` — two entries *within one layer* rendering to the same output path is
an error — is completely untouched; it is only extended to say "the nearest layer wins" when the collision is
between two *different* layers at the same pre-render path, which is the override, not an accident.

### The loader: flat merge, not per-origin scoping

Every layer's partials merge into **one namespace, keyed by name**, exactly like `[data]`: the nearest layer to
the child that declares a given name wins for a bare reference. `parent:name` means "the next declaration of that
same name, one layer further out" — the *shadowed* value a bare reference would have resolved to had the nearer
layer not overridden it.

This is deliberately the simpler of the two designs considered in Context. It is well-defined at any chain depth
with no per-file-origin tracking in the loader, and it is exactly the override rule already applied to
`[questions]`/`[computed]`/`[data]`/`[remotes]`, extended to files that live outside the render root instead of
invented fresh for them. The cost, accepted: a child that overrides `macros.jinja` also shadows an *unrelated*
ancestor file's own bare reference to a same-named `macros.jinja` two layers up, which the fully origin-scoped
design would have kept separate. This is a real, if narrow, gap — a template with two ancestors that happen to
both define a `macros.jinja` for unrelated purposes, where the nearer one overrides the further one's name by
accident, gets the nearer one everywhere, silently. It is judged an acceptable v1 trade-off: the fix is renaming
one file, which is exactly the fix an equivalent `[data]` name collision already requires today.

### Provenance

A `Template-Extends` trailer per ancestor, nearest-parent first, root ancestor last, each as `<source>@<sha>` —
positional rather than named like `Data-Source`, because an ancestor has no name a template author chose; its
position in the chain *is* its identity.

```
Template-Source: https://github.com/org/child-template
Template-Ref: v1.2.0
Template-Commit: 9c1e2f4a...
Template-Extends: https://github.com/org/base-template@a1b2c3d
Answers-Digest: sha256:...
Tpl-Version: X.Y.Z
```

`Template-Source`/`Template-Ref`/`Template-Commit` continue to describe the directly-configured template only, as
they always have — the chain above it is exactly what `Template-Extends` is for.

## Consequences

A template with no `[extends]` renders exactly as it did before this ADR: the merge of a chain of one layer is
the identity, and every existing manifest, test and rendered tree is unaffected.

An organisation can maintain one `base` template and extend it from as many child templates as it has, each
overriding only what is genuinely specific to it — the case that motivates this.

Every ancestor is cloned on every run, on top of the existing per-run clone of the template itself. A three-deep
chain costs three clones where extending nothing cost one. This is not solved here; it is the same "stays until
it actually hurts" cache posture `docs/adr/*` has taken for the template's own clone, now under more pressure.

The partial-loader trade-off above is a known, accepted limitation, not an oversight: a template hitting it gets
a same-name collision exactly as legible as today's `[data]` name collision, and the fix is the same — rename.

`docs/templates/format.md`, `docs/templates/questions.md`, `docs/templates/computed.md` and
`docs/data/reproducibility.md` are updated in the same change. New diagnostic codes
(`tpl::manifest::invalid_extends`, `tpl::manifest::extends_kind_collision`, `tpl::extends::cycle`,
`tpl::extends::depth`, `tpl::extends::unpinned`, `tpl::extends::remove_missing`) are added to
`docs/reference/diagnostics.md` and pinned by `tests/diagnostics.rs`.
