# Reproducibility and pinning

The rendered output becomes a Git commit, so "what produced this tree?" must have an answer.
This page is about how much of that answer git-tpl can give.

## What is already pinned

**The template.** `ref` resolves to a commit SHA, recorded in the trailers as `Template-Commit`.
Rendering that revision again gives the same template files.

**Template data.** Read from the template's Git tree at that same commit, so it is pinned by the template
revision.
There is no second thing to pin, and no way for a template's files and its data to drift apart.

**Git data pinned to a tag or a SHA.** Read from another repository's tree at a resolved commit, which is
recorded.
A `ref` naming a *branch* is the exception, and belongs in the next section.

**The answers.** Recorded in `.config/git.tpl.toml`, and digested into `Answers-Digest` so a change is detectable
without reading the file.

**The engine.** `Tpl-Version` records which git-tpl produced the tree, so a rendering difference caused by a
git-tpl change is attributable.

Given those five, a rendering is reproducible — with one exception.

## What is not: remote data, or a moving ref

A remote URL can serve different bytes tomorrow, and so can a branch.
Nothing about the template revision constrains either.

Every contributing source is recorded, and for a remote source the digest of the bytes actually received is
recorded with it — whether or not the template pinned one.

```
Data-Source: licenses = template:data/licenses.toml@8b3e7d1
Data-Source: registry = remote:https://example.com/registry.json@sha256:9f86d081…
```

Read it back from Git:

```sh
git show --no-patch refs/tpl/github-com-noirbizarre-rust-library-template
```

So you can always determine *which* external sources contributed to a tree, and whether the bytes changed between
two renderings.
Guaranteeing they will not change is the next section.

## Pinning

=== "Checksum"

    ```toml
    [data.licenses]
    source = "https://example.com/licenses.toml"
    sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    ```

    A mismatch is an **error**, never a warning: the point of a pin is that the render stops rather than
    producing a plausible tree from content the template did not vouch for.
    Both digests are reported, so "the source changed" is distinguishable from "the pin is wrong".

    Exact, verifiable, and the strongest option — at the cost of a manual update whenever the source legitimately
    changes.
    It is accepted on any kind of data source, though a template file is already pinned by the template revision.

    The digest of a source you already trust is in the trailers of the last rendering, so recording a pin does
    not mean computing one by hand.

=== "Immutable URL"

    ```toml
    [data.licenses]
    source = "https://example.com/licenses/v3/licenses.toml"
    ```

    Requires nothing from git-tpl, and is the right answer when you control the host.
    A version in the path is a pin.

=== "Git-hosted data"

    ```toml
    [data.shared]
    source = "https://github.com/acme/tpl-data"
    ref = "v2.1.0"
    path = "licenses.toml"
    ```

    Reuses the mechanism the template itself uses, so the pin is a commit SHA and the provenance format already
    describes it.
    A tag or a SHA is a pin; a branch is not, and is as irreproducible as a URL — the trailer records the commit
    either way, so a difference is at least always explainable.
    See [Git data](git.md).

## Determinism is the wider property

Pinning addresses external data.
The rest of the determinism guarantee — traversal order, line endings, permissions, no timestamps, no
environment, no runtime context — is covered in [Determinism](../concepts/determinism.md).

The two together are what make it safe for the rendered output to be a Git ref: an `update` that produces no
change makes no commit, so the ref only grows when something real changed.
