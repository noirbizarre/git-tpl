# ADR-026: A rendered path piece may be `.` or fan out across `/`; only `..` and a backslash are rejected

**Status:** accepted

**Relates to:**
[ADR-003](003-minijinja-only.md),
[ADR-006](006-no-runtime-context.md)

## Context

Issue #114: a template has no way to make one directory level conditionally absent while keeping what is nested
underneath it.

An empty path segment already exists, and prunes a whole subtree:

```
template/{% if ci %}.github{% endif %}/workflows/ci.yml
```

with `ci` false, the entire `.github/workflows/ci.yml` entry is skipped — segment, and everything beneath it. That
is the only conditional a path segment offered before this ADR.

A Python package that lives at `src/<package>` under a `src/` layout, at `<namespace>/<package>` for a namespace
package, or plain `<package>` otherwise needs a *different* shape: the file underneath the optional level still has
to render, just at a different depth. Without an alternative, the only git-tpl-native way to build a variable-depth
subtree is to duplicate it once per structural combination — 2ⁿ physical copies for *n* independent booleans, each
requiring hand-kept synchronisation. That is a standing maintenance cost, not a one-off migration.

Copier templates solve the same problem by rendering a single directory name to a string containing `/`, letting
the separator characters fan out into real nested directories at write time. git-tpl rejected that outright: a
rendered segment could not be `.`, could not be `..`, and could not contain `/` or `\`, all by the same check, for
the same stated reason ("a template repository is untrusted input").

Bundling all four into one check obscured that they are not one concern. Two are genuine dangers:

- **`..`** is a request to write outside the render root.
- **A backslash** is a separator Git itself treats differently across platforms, and a name containing one would be
  ambiguous depending on which OS later reads the same tree.

The other two were never dangers in themselves — they were forbidden only because, until now, a segment was always
exactly one path component, and a lone `.` or an embedded `/` had no way to mean anything within that model. Once a
segment is allowed to *mean something* at that finer grain, both become safe: a lone `.` can mean "nothing here,
promote my neighbours," and an embedded `/` can simply produce more pieces, each checked the same way `..` and a
backslash always were.

## Decision

A rendered path is built from segments (raw, `/`-delimited components of the template path) and, within each
segment's rendered value, **pieces** (that value split again on `/`). Each piece is validated independently,
regardless of which segment produced it:

- **Empty or `.`** — dropped. Contributes nothing to the output path, and unlike a segment that renders to the
  empty string as a whole (which still prunes the entire entry, unchanged), a piece disappearing does not skip
  anything nested under it: the rest of the path renders exactly as if that piece were never there.
- **`..` or containing a backslash** — rejected, unconditionally, at any position. This is `tpl::render::escapes_tree`,
  unchanged in spirit from before this ADR, narrowed to the two checks that were ever actually load-bearing.
- **Anything else** — kept, becoming one real path component.
- **The exception:** the very last piece of the very last segment — the file's own basename — has no "above" to
  promote its content to. Both the empty and `.` cases still reject there, same as `..` and a backslash always did.
  Allowing a file's name to vanish would land it at the path that should have been its own parent directory; if
  that directory holds other files, the result is a blob and a tree claiming the same path, a clash the Git backend
  would refuse with its own error rather than a named diagnostic — precisely the failure mode `EscapesTree` exists
  to give a name to.

Two idioms follow from this, and either solves #114:

```
template/{% if use_src %}src{% else %}.{% endif %}/{{ package_name }}/mod.rs
```

A `.`-transparent segment, one `{% if %}` per level. Reads well when there are a couple of independent booleans,
each toggling one directory.

```
template/{{ package_path }}pkg/mod.rs
```

with `package_path` computed as `"{{ 'src/' if use_src else '' }}"`. One expression fans out into as many real
directories as it renders, exactly the Copier idiom — now safe, because the actual invariant (`..`, backslash) is
checked per piece rather than assumed to hold by forbidding `/` outright. Reads well when the depth is genuinely
one computed value, or when there are more structural dimensions than are comfortable to nest as `{% if %}` blocks.

Neither is preferred; a template picks whichever reads more clearly for its own shape.

### Why this is not "Copier compatibility"

`docs/development/contributing.md` lists "Copier or Cruft compatibility" among the things this project declines,
and the second idiom above produces exactly what Copier's own path-templating trick produces. The distinction is
that the declined item is about *adopting Copier's engine* — its templating language, its task hooks, its
migration model, as a compatibility surface git-tpl commits to tracking. This is not that: it is git-tpl's own
existing segment-validation rule, generalized from "exactly one path component" to "one or more," using the engine
and the validation this project already had. Nothing about MiniJinja, code execution, or the render context changes
(ADR-003, ADR-006 are unaffected). A template author familiar with Copier's trick will recognize the result; a
template author who has never heard of Copier will find it documented here on its own terms.

### What deliberately still does not change

- **No manifest flag.** Neither idiom needed one before this ADR — nothing about a rendered value could previously
  express either shape, so there is nothing existing to opt into or preserve compatibility with.
- **`..` and a backslash are unconditional, at any position, no exception.** These are the only two checks that were
  ever about a genuine escape, and narrowing everything else does not touch them.
- **The basename still cannot vanish.** Whether via an empty piece, a `.` piece, or the last piece of a fanned-out
  value being empty (a trailing separator, e.g. a segment rendering to `"name/"`), the file's own name has nowhere
  to promote its content to, and all three still reject there.

## Consequences

`render_path` (`src/render.rs`) tracks, for each piece, whether it is the very last piece of the very last raw
segment — the basename — rather than only tracking which raw segment is last. Everything else about path
rendering — the whole-segment empty prune, the final collision check — is unchanged.

The `EscapesTree` diagnostic's `rendered` field now reports the *whole* segment's rendered value (e.g. `a/../b`),
not just the offending piece: showing a lone `..` or an empty string out of context would be less useful than
showing what was actually rendered.

`tpl::lint::degenerate_path` and `tpl::lint::collision` need no change. Both are purely static, textual scans of
`{% if %}...{% endif %}` structure — they never evaluate a branch's content and so never knew, before or after this
ADR, whether a branch renders `"y"`, `"."`, or `"a/b"`. `{% if use_src %}src{% else %}.{% endif %}` and
`{{ package_path }}` are both invisible to them in exactly the same way `{% if a %}x{% else %}y{% endif %}` already
was: a segment whose rendered value cannot be known without evaluating it does not get folded into the static
comparison, so `collision` under-detects collisions that only occur through such a segment. That gap predates this
ADR; it is not widened by it. `tpl::render::collision` still catches the real thing at render time, for whichever
answer set actually renders it.

No new diagnostic code. `tpl::render::escapes_tree` keeps meaning what it says for everything it still applies to;
its help text and the reference page describe the narrowed set of checks.
