# ADR-008: Provenance lives in commit trailers, not in the tree

**Status:** accepted

## Context

A rendered commit needs to record what produced it: the template source, the resolved revision, the answers, which
data sources contributed, which git-tpl version rendered it.
`git tpl status` needs to read it back.

Three places it could go:

1. a file in the rendered tree (`.git-tpl/lock.toml`)
2. `.config/git.tpl.toml`
3. the commit message, as trailers

## Decision

Trailers on the rendered commit.

```
tpl: render rust-library at v1.4.0

Template-Source: https://github.com/noirbizarre/rust-library-template
Template-Ref: v1.4.0
Template-Commit: 4f2c1a9e6b3d8f05a1c7e2b94d6f8a03c5e17b29
Answers-Digest: sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
Data-Source: licenses = template:data/licenses.toml@4f2c1a9
Tpl-Version: 0.2.0
```

## Consequences

The rendered tree is *only* rendered files.
`git diff HEAD refs/tpl/<id>` shows real differences and nothing the user must learn to ignore.
A provenance file in the tree would appear in every diff and every merge, and would conflict on every update — it
changes on every render by definition.

Provenance is attached to the thing it describes.
The tree and its provenance move together and cannot be separated, which is not true of a file in a different
commit.

It is readable with plain Git, and trailers are a Git convention with existing tooling (`git interpret-trailers`,
`%(trailers)` in `--format`):

```sh
git show --no-patch refs/tpl/github-com-noirbizarre-rust-library-template
git log --format='%(trailers:key=Template-Commit,valueonly)' refs/tpl/github-com-noirbizarre-rust-library-template
```

The provenance history is queryable.
`git log` on the ref gives every rendering and what produced it, which is how `status` reports the previous
revision.

Option 2 was rejected because `.config/git.tpl.toml` is the user's *input* — hand-editable, reviewed, and
containing only the template reference and the answers.
Mixing generated state into it means a file that is partly authored and partly machine-written, and users editing
the wrong half.

The cost: trailers are strings, so parsing needs care and a round-trip test.
That is one unit test.
