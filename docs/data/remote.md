# Remote data

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
Data-Source: licenses = remote:https://example.com/licenses.toml@sha256:9f86d081…
```

The digest is recorded whether or not the template pinned one, so the question
"why did this tree change when nothing changed?" is always answerable. Making it
*not happen* requires pinning — see [Reproducibility](reproducibility.md).

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

**The URL must be visible in the declaration.** A `source` that only becomes a
URL once an answer is substituted must say `kind = "remote"`:

```toml
[data.registry]
source = "{{ registry_base }}/languages.json"
kind = "remote"
format = "json"
```

Without it the fetch is refused, because the confirmation below lists every
remote source *before* anything is evaluated, and it can only do that from the
declaration. A URL that appeared later would slip past the list, which would
make the list a half-truth.

## Confirmation

Fetching is the one thing a template asks git-tpl to do on its behalf, so it is
shown in full before it happens. Rendering itself never requires trust: no
template can execute anything, confirmed or not.

```console
$ git tpl init https://github.com/org/template

This template wants to fetch 1 remote data source:

  licenses  https://example.com/licenses.toml

Each response is limited to 5120 KiB and is parsed as data — never executed.

? Fetch `licenses`?
> Fetch it
  Skip it — the render will fail if it is needed
  Abort
```

Nothing is remembered. The next run asks again.

`--trust` accepts every source for one invocation, without prompting and without
writing anything anywhere:

```sh
git tpl init https://github.com/org/template --trust
```

When there is nobody to ask — `--defaults`, `tpl.interactive false`, CI — every
remote source is **refused**, loudly, naming what was refused:

```
x data source `licenses` was not fetched, because the template is not trusted
help: source: https://example.com/licenses.toml
      pass `--trust` to allow this template's remote data sources for this run,
      or answer the confirmation interactively
```

Never silently accepted: a CI runner is the worst possible place to grant a
capability by omission.

## Treated as untrusted

Remote data is input from a third party. It is parsed defensively, and:

| Bound | Value |
|---|---|
| Response size | 5 MiB |
| Total time per request | 30 seconds |
| Redirects followed | 5 |
| Retries | none |

The size limit is enforced while reading the body, never taken from
`Content-Length` — that header is a claim made by the party being bounded. A
malformed response is an error that names the source rather than a panic or a
partial context.

## Failure stops the render

A remote source that cannot be loaded aborts the operation. It does not fall back
to a cached copy, an empty table, or the last known value — each of those
produces a plausible-looking tree that is quietly wrong, and that tree would be
committed.

```
x could not load template data source `licenses`
help: source: https://example.com/licenses.toml
      kind:   remote
      reason: timed out reading response
```
