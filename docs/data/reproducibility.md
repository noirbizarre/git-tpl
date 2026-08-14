# Reproducibility and pinning

The rendered output becomes a Git commit, so "what produced this tree?" must have
an answer. This page is about how much of that answer git-tpl can give.

## What is already pinned

**The template.** `ref` resolves to a commit SHA, recorded in the trailers as
`Template-Commit`. Rendering that revision again gives the same template files.

**Template data.** Read from the template's Git tree at that same commit, so it
is pinned by the template revision. There is no second thing to pin, and no way
for a template's files and its data to drift apart.

**The answers.** Recorded in `.config/git.tpl.toml`, and digested into
`Answers-Digest` so a change is detectable without reading the file.

**The engine.** `Tpl-Version` records which git-tpl produced the tree, so a
rendering difference caused by a git-tpl change is attributable.

Given those four, a rendering is reproducible — with one exception.

## What is not: remote data

A remote URL can serve different bytes tomorrow. Nothing about the template
revision constrains that.

git-tpl's current answer is honesty rather than prevention: every contributing
source is recorded.

```
Data-Source: licenses = template:data/licenses.toml@8b3e7d1
Data-Source: registry = remote:https://example.com/registry.json
```

Read it back from Git:

```sh
git show --no-patch refs/tpl/rust-library
```

So you can determine *which* external sources contributed to a tree, and rule
them in or out when a rendering changes unexpectedly. You cannot yet guarantee
they will not change.

## Planned: pinning

The design accommodates these; none is implemented in 0.1.0.

=== "Checksum"

    ```toml
    [data.licenses]
    source = "https://example.com/licenses.toml"
    sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    ```

    A mismatch is an error. Exact, verifiable, and the strongest option — at the
    cost of a manual update whenever the source legitimately changes.

=== "Immutable URL"

    ```toml
    [data.licenses]
    source = "https://example.com/licenses/v3/licenses.toml"
    ```

    Available today, requires nothing from git-tpl, and is the right answer when
    you control the host. A version in the path is a pin.

=== "Git-hosted data"

    ```toml
    [data.shared]
    source = "https://github.com/acme/tpl-data"
    ref = "v2.1.0"
    path = "licenses.toml"
    ```

    Reuses the mechanism the template itself uses, so the pin is a commit SHA and
    the provenance format already describes it.

## Determinism is the wider property

Pinning addresses external data. The rest of the determinism guarantee — traversal
order, line endings, permissions, no timestamps, no environment, no runtime
context — is covered in [Determinism](../concepts/determinism.md).

The two together are what make it safe for the rendered output to be a Git ref:
an `update` that produces no change makes no commit, so the ref only grows when
something real changed.
