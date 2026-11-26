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

The common thread: data that several templates share, and that changes on a different schedule than any of them.

## What it costs

A remote source is the one thing that can make a rendering irreproducible without the template changing.
Two people rendering the same template revision with the same answers can get different trees, because the URL
served different bytes.

git-tpl does not pretend otherwise.
The rendered commit records every remote source that contributed:

```
Data-Source: licenses = remote:https://example.com/licenses.toml@sha256:9f86d081…
```

The digest is recorded whether or not the template pinned one, so the question "why did this tree change when
nothing changed?" is always answerable.
For a composed child, the same digest appears in its owner-scoped `Template-Compose-Data` location.
Older commits may lack child data evidence.
Making it *not happen* requires pinning — see [Reproducibility](reproducibility.md).

## Rules

**Only `http` and `https`.** No `file://`, no arbitrary transport.
Data that lives in a Git repository is a [`git` source](git.md), which is cloned rather than fetched — and gated
the same way.

**Never executable.** The response is parsed as TOML, JSON or YAML into plain values.
There is no code path by which remote content is evaluated, rendered as a template, or otherwise given meaning
beyond "data".

**Fetched by the data layer, never by a template.** There is no `http_get()` in the expression language, and there
will not be one.
A template declares the source; it cannot construct a request.

**Fetched at most once per run**, no matter how many questions and files use it.
With a declared `sha256`, verified content is also reused across runs and URLs from the
[source cache](../usage/cache.md#http-content-identity-and-integrity).
Without a checksum, every invocation still needs network access.

**Only if used.** A declared source that nothing references is never fetched, so a template may offer
remote-backed choices on a conditional branch without imposing a network round-trip on everyone.

**The URL must be visible in the declaration.** A `source` that only becomes a URL once an answer is substituted
must say `kind = "remote"`:

```toml
[data.registry]
source = "{{ registry_base }}/languages.json"
kind = "remote"
format = "json"
```

Without it the fetch is refused, because the confirmation below lists every remote source *before* anything is
evaluated, and it can only do that from the declaration.
A URL that appeared later would slip past the list, which would make the list a half-truth.

## Confirmation

Fetching is one of the things a template asks git-tpl to do on its behalf — a [`git` source's
clone](git.md) is another, and cloning an [`[extends]`](../templates/format.md#extends) ancestor is a third — so
it is shown in full before it happens. Rendering itself never requires trust: no template can execute anything,
confirmed or not.

An `[extends]` ancestor is confirmed the same way, for the same reason (its `source` is chosen by the template
author, not typed on the command line) — but not at the same *time*: every `[data]` source is known from the
leaf's own manifest before anything is evaluated, so all of them are listed together, once. An `[extends]` chain
is only discoverable one clone at a time — an ancestor's own parent isn't readable until that ancestor has
already been cloned — so each remote ancestor is confirmed individually, immediately before it is cloned, rather
than as one combined list.

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

Nothing is remembered.
The next run asks again.

`--trust` accepts every source for one invocation, without prompting or recording a trust decision:

```sh
git tpl init https://github.com/org/template --trust
```

### Saying it once

A prompt answered `yes` twenty times a day is not a decision, it is a keystroke.
Templates you have already made up your mind about go in your own [`[trust]` list](../configuration.md#trust):

```toml
# ~/.config/git-tpl/config.toml
[trust]
templates = [
  "github.com/noirbizarre/*",
  "github.com/myorg/**",
]
```

A match accepts every source without a prompt, exactly as `--trust` would — including when there is nobody to
ask.
The entry *is* the prior consent, and it is no weaker for having been written yesterday.

The list lives on your machine and never in `.config/git.tpl.toml`.
A project cannot consent to network access on its reader's behalf.

When there is nobody to ask — `--defaults`, `tpl.interactive false`, CI — and the template matches no `[trust]`
entry, every remote source is **refused**, loudly, naming what was refused:

```
  x data source `licenses` was not loaded, because the template is not trusted
    help: source: https://example.com/licenses.toml
          pass `--trust` to allow this template's network data sources for this
          run, or answer the confirmation interactively
```

Never silently accepted: a CI runner is the worst possible place to grant a capability by omission.

## Treated as untrusted

Remote data is input from a third party.
It is parsed defensively, and:

| Bound | Value |
|---|---|
| Response size | 5 MiB |
| Total time per request | 30 seconds |
| Redirects followed | 5 |
| Retries | none |

The size limit is enforced while reading the body, never taken from `Content-Length` — that header is a claim
made by the party being bounded.
A malformed response is an error that names the source rather than a panic or a partial context.
Cached bodies are bounded by the same size limit and checksum-verified before every use.

## Checksum-pinned reuse and offline use

Declare `sha256` to retain verified raw bytes across invocations; see [pinning](reproducibility.md#pinning).
An existing verified object satisfies the pin without a request, including with `--offline`.
Each declaration still requires trust and is parsed in its own format.
Provenance records its requested URL and actual checksum, irrespective of cache state.

Offline cache misses fail with instructions to populate the same checksum online.
Corrupt objects fail even online and require explicit `git tpl cache clear sha256:<digest>` before retrying.
Unpinned sources never consult persistent HTTP storage and cannot be used offline.

## Failure stops the render

A remote source that cannot be loaded aborts the operation.
An acquisition failure does not fall back to an old response, an empty table, or the last known value — each produces a
plausible-looking tree that is quietly wrong, and that tree would be committed.
Verified checksum-addressed content is an exact match for an immutable pin, not a failure fallback.

```
  x could not load template data source `licenses`
    help: source: https://example.com/licenses.toml
          kind:   remote
          reason: timed out reading response
```
