# ADR-012: Partials are `.jinja` files outside the render root

**Status:** accepted

## Context

Any template much above fifty files repeats itself: the same badge block, the
same licence header, the same workflow step. Jinja's answer is
`{% import %}` and `{% include %}`, and MiniJinja supports both — but they
resolve names through a *loader*, and git-tpl registered none. Every import
failed with "tried to include non-existing template".

Three questions had to be answered together, because the answer to each
constrains the others.

**Where does a loader read from?** Not the filesystem. A template revision is
pinned, and a loader that read `./macros.jinja` relative to the process's
working directory would break invariant 2 the first time two people rendered
from different directories. It must read the Git tree, at the resolved
revision — the same bytes everything else renders from.

**Is a loader an extension point?** ADR-003 closes the filter set and states
there is no plugin point, because templates are untrusted input. A loader looks
superficially like a hole in that. It is not: it resolves a name to *bytes
already committed in the template repository* and hands them to the same
parser that already reads every `.jinja` file. It executes nothing, and it
reaches nothing a template could not already put in a file. A template that can
`{% import %}` its own `macros.jinja` can do exactly what it could do by pasting
the macro into every file. Invariant 5 is untouched.

**How does a macro definition avoid being rendered into the project?** This is
the question that shaped the decision. `template/macros.jinja` would be walked
like any other file and written out as `macros` — a stray file in every
generated project. Avoiding that needs a rule about which files are output and
which are not.

The obvious candidates were a manifest key (`partials = [...]`), a naming
convention (`_macros.jinja`), and a convention directory
(`template/_partials/`). Each adds a concept, a skip rule in the tree walk, and
a way to get it wrong — a file that is silently neither rendered nor loadable.

## Decision

**A partial is any `.jinja` blob in the template repository that lives outside
the render root**, named by its path relative to the repository root.

```
template.toml
macros.jinja          importable as "macros.jinja"
macros/rust.jinja     importable as "macros/rust.jinja"
data/licenses.toml    not importable — not .jinja; `[data]` reads this
README.md             the template's own readme
template/             the render root
  README.md.jinja     {% import "macros.jinja" as m %}
```

The render root already separates output from not-output: `root` in
`template.toml` names the subtree that gets rendered, and everything else in the
repository — the manifest, the data files, the template's own README — is
already never emitted. Partials join that set. The tree walk only ever sees the
root subtree, so **there is no skip rule**: a partial cannot reach the project
because the renderer never sees it.

The `.jinja` restriction bounds what is read eagerly, and keeps parsing data
files the business of `[data]`, which knows their formats.

A file *inside* the root is output, never a partial. Giving one file both
meanings is the ambiguity this decision exists to avoid.

Names are plain paths. The `<prefix>:` namespace is deliberately left free for
template inheritance, where a child must be able to name its parent's file
explicitly (`{% extends "parent:base.html.jinja" %}`) rather than resolve to
itself.

Partials are collected once per run, into an owned map, before anything renders.
MiniJinja's loader must be `Send + Sync + 'static`, which the libgit2 backend's
repository handle is not, so borrowing the repository into the loader is not
possible. Reading up front is also what makes the set enumerable, which
the diagnostics need.

The one environment constructor in `src/eval.rs` takes the partials, so a macro
importable from a `.jinja` file is importable from a `computed` expression too.
Two environments would be two sets of rules for the same syntax.

A miss returns `Ok(None)` from the loader rather than an error, because that is
what `{% include "x" ignore missing %}` is defined against. The names that *do*
exist are appended to the diagnostic instead — the failure is nearly always a
typo, or a path written relative to the render root.

## Consequences

Templates can share macros. The repeated badge block is written once.

Nothing about the on-disk format changed. No new manifest key, no reserved
filename, no new directory. A template that has no partials behaves exactly as
before, and the loader is never consulted.

Determinism holds: the loadable set comes from `list_tree`, which is in
Git-canonical order, into a `BTreeMap`. A partial is pinned to the same revision
as the files that import it, `--dirty` included, so editing a macro changes the
rendered tree and advances the ref — which is the behaviour that makes the
change visible at all.

Every `.jinja` file outside the root is now read on every run, whether or not
anything imports it. They are small and there are few of them; the alternative
is a lazy loader, which the borrow rules above rule out.

A binary file named `.jinja` outside the root is now an error
(`tpl::render::partial_not_utf8`) rather than being ignored. It was already an
authoring mistake; it now says so.

The static graph analysis in `src/graph.rs` does not follow imports. It is a
parse, and `undeclared_variables` never crosses a template boundary — a name a
macro references is resolved inside the macro's own scope, not in the manifest's
dependency graph. A missing partial is a render-time failure with a render-time
diagnostic.

This is a prerequisite for template inheritance, which needs the same loader
backed by layered trees.
