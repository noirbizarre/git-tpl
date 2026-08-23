# ADR-017: `.gitignore` is evaluated by us, not by libgit2

**Status:** accepted

**Relates to:** [ADR-011](011-git-backend-isolation.md)

## Context

`--dirty` renders a template's working tree rather than a commit.
Its contract is one sentence: the result is what `git add -A` would have staged.
That is what makes `--dirty` safe to reason about — a `--dirty` render and a render of the same tree once
committed must agree, or the flag changes the output rather than just the input.

libgit2 breaks that contract in one case.
`git_ignore_path_is_ignored` will not let a negation in a repository `.gitignore` override a rule that came from a
*lower-precedence* ignore file — `core.excludesFile` or `.git/info/exclude`.
Git will. Measured against git2 0.21 with the vendored libgit2 1.9.6:

| repository `.gitignore` | `core.excludesFile` | `is_path_ignored` | `git add -An` |
|---|---|---|---|
| `!a` | — | `false` | staged |
| `!a.toml` | — | `false` | staged |
| `!mise.toml` | `mise.toml` | **`true`** | **staged** |

Which is not an exotic configuration.
A widespread global ignore hides `mise.toml` and `mise.lock` on the assumption that mise configuration is
personal, and a project that commits them re-includes them explicitly.
Any template rendering a `mise.toml` therefore ships `!mise.toml` in its `.gitignore`, and `--dirty` silently
dropped the file.

It surfaced through `git tpl test` (issue #51), where the asymmetry makes it particularly hard to see: in the
template the file is `mise.toml.jinja`, which the rule does not match, so rendering looks fine.
Only once a snapshot is recorded does the *rendered* `.gitignore` come to govern the snapshot's own `files/`
directory — and `--write` writes through the filesystem while read-back goes through the walk, so the `MANIFEST`
survived and the file it lists did not.

Three options:

1. **Post-correct libgit2's answer.** Impossible: libgit2 reports a boolean, not the rule that decided it. There
   is nothing to correct against.
2. **Hand-roll a matcher.** No new dependency, but gitignore semantics — anchoring, `**`, directory-only rules,
   character classes, precedence between files — are exactly the kind of thing that is subtly wrong for years.
3. **Use an existing correct matcher.**

## Decision

Evaluate the ignore stack in `src/git/ignore.rs`, using `ignore::gitignore::GitignoreBuilder` — ripgrep's matcher,
which agrees with `git add -An` on this and every other case tested.

`IgnoreStack` holds one matcher per ignore file, weakest first: `core.excludesFile`, `.git/info/exclude`, then
every `.gitignore` from the working-tree root down.
The innermost layer with an opinion wins; within a layer the last matching pattern wins.
That layering *is* the fix.

Only the matcher is used.
The crate's parallel walker is not: it reports nothing about what it skipped, and `--dirty` has to be able to
name the files a global rule removed.
The walk in `collect_workdir` stays ours, keeping its deterministic ordering, its `.git` skipping and its record
of what it dropped.

`core.excludesFile` is still resolved through libgit2's config chain, so a repository-local override means here
what it means to Git.

The walk stays repository-wide — the tree is needed whole, for partials and for `lint` — but the *report* is
narrowed in `resolve` to what a render reads: the tree under `root`, the partials outside it, and the files
declared data sources name.
Warning about a path that was never a candidate for the rendering is a warning nothing in the template can
silence, printed above every run.

## Consequences

`--dirty` renders what `git add -A` stages.
Verified by diffing the two file lists over a tree mixing global rules, negations, nested `.gitignore` files and
ignored directories — that comparison, not a unit test, is the acceptance criterion, because the contract is
stated in terms of Git's behaviour.

Two things fall out.
Directory-only rules such as `build/` now work: the walk stats before it asks, so it can say whether the path is
a directory.
And a template source *below* its repository root is handled correctly — the previous code handed libgit2 a path
relative to the template root rather than to the working tree, so outer rules matched the wrong thing or nothing
at all.

Precedence is now unit-testable without a repository, which it was not while the answer came from libgit2.

The cost is a dependency, and it brings `globset` and the full `regex-automata`.
That is a real weight in a tree that chose `regex-lite` elsewhere, and it is accepted here because the alternative
is option 2: gitignore globs are not regexes, so `regex-lite` would not have helped, and hand-translation is the
failure this buys out of.

ADR-011 is narrowed, not violated.
`GitBackend` still hides every `git2` type, and `src/git/ignore.rs` names none — the hook is satisfied.
What changes is the scope of what the backend *decides*: it is now asked for objects, refs and config, and no
longer for a policy judgement it gets wrong.
A future backend inherits the correct behaviour instead of having to reimplement it.
