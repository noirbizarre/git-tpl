# Remote data

!!! warning "Not yet implemented"

    Remote data sources are designed and specified, but the loader is not
    implemented in 0.1.0. Declaring one produces a clear error naming the
    source. The `DataSource` abstraction and the provenance format already
    account for them, so this is additive rather than a redesign.

    Track it in [PLAN.md](https://github.com/rawtools/git-tpl/blob/main/PLAN.md).

Data hosted independently of the template repository.

```toml
[data.licenses]
source = "https://example.com/licenses.toml"
```

## What it is for

- centrally maintained choice lists
- organisation-wide metadata
- language and platform matrices
- shared defaults across many templates
- external registries

The common thread: data that several templates share, and that changes on a
different schedule than any of them.

## What it costs

A remote source is the one thing that can make a rendering irreproducible
without the template changing. Two people rendering the same template revision
with the same answers can get different trees, because the URL served different
bytes.

git-tpl does not pretend otherwise. The rendered commit records every remote
source that contributed:

```
Data-Source: licenses = remote:https://example.com/licenses.toml
```

so the question "why did this tree change when nothing changed?" is at least
answerable. Making it *not happen* requires pinning — see
[Reproducibility](reproducibility.md).

## Rules

**Only `http` and `https`.** No `file://`, no `git://`, no arbitrary transport.

**Never executable.** The response is parsed as TOML or JSON into plain values.
There is no code path by which remote content is evaluated, rendered as a
template, or otherwise given meaning beyond "data".

**Fetched by the data layer, never by a template.** There is no `http_get()` in
the expression language, and there will not be one. A template declares the
source; it cannot construct a request.

**Fetched at most once per run**, no matter how many questions and files use it.

**Only if used.** A declared source that nothing references is never fetched, so
a template may offer remote-backed choices on a conditional branch without
imposing a network round-trip on everyone.

## Treated as untrusted

Remote data is input from a third party. It is parsed defensively, size-limited,
and a malformed response is an error that names the source rather than a panic or
a partial context.

## Failure stops the render

A remote source that cannot be loaded aborts the operation. It does not fall back
to a cached copy, an empty table, or the last known value — each of those
produces a plausible-looking tree that is quietly wrong, and that tree would be
committed.

```
Could not load template data source `licenses`.

  Template:  rawtools/rust-library
  Source:    https://example.com/licenses.toml
  Kind:      remote
  Reason:    HTTP request failed: connection timed out
```
