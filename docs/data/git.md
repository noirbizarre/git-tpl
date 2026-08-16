# Git data

Data that already lives in a Git repository, read from that repository's tree at
a revision. It reuses the mechanism the template itself uses, so the pin is a
commit SHA and there is no raw-content URL and no separate checksum to maintain.

```toml
[data.licenses]
source = "https://github.com/acme/tpl-data"
ref    = "v2.1.0"
path   = "licenses.toml"
```

Or, the same thing in one line:

```toml
[data.licenses]
source = "https://github.com/acme/tpl-data@v2.1.0:licenses.toml"
```

The two are the same source, and everything downstream — the confirmation, the
cache, the trailer — sees one shape. Write whichever reads better.

## What it is for

Convenience, not reproducibility. A `sha256` pin already covers the latter. The
value here is that a shared `data/` repository stops needing a raw-content URL
and a separate pin: the ref is the pin, and it is the same kind of pin the
template's own `ref` is.

## The shorthand

`<scheme>://<repo>@<ref>:<path>`, and **the scheme is required**.

That requirement is the whole reason the form can be parsed at all.
`git@github.com:acme/data` is a perfectly ordinary scp-style Git URL containing
both an `@` and a `:`, and no heuristic can tell it from a shorthand reliably. So
git-tpl does not try: without a `://` it is not a shorthand, and an scp-style
repository must be written with the three keys.

| Written | Read as |
|---|---|
| `https://host/acme/data@v1:licenses.toml` | repo `https://host/acme/data`, ref `v1`, path `licenses.toml` |
| `ssh://git@host:22/acme/data@main:l.toml` | the port stays in the repository; the split is on the *last* colon |
| `https://host/acme/data@release/2.x:l.toml` | a ref may contain `/` |
| `git@host:acme/data@v1:l.toml` | **not** a shorthand — no scheme. Use `source`, `ref` and `path` |
| `https://host/acme/data` | not a shorthand — a plain remote URL |

Two limits follow from the grammar, and both have the same escape hatch:

- a path containing a `:` cannot be written in the shorthand;
- an scp-style repository cannot either.

Write `source`, `ref` and `path` instead. Nothing is lost — the shorthand is a
spelling.

## Rules

**Any ref, and the resolved commit is recorded.** A branch, a tag or a SHA. A
moving ref is allowed and is *not* reproducible; see below.

**The path may not leave the repository.** A `..` component is refused rather
than resolved. A data repository is untrusted input, and a path that climbs out
of it is a request to read a file the declaration does not name.

**Cloned by the data layer, never by a template.** There is no `clone()` in the
expression language, and there will not be one.

**Never executable.** The file is parsed as TOML, JSON or YAML into plain values.
No hook, no submodule, no `.gitattributes` filter in the data repository is run.

**Cloned at most once per render, and never cached between runs.** Two sources
naming the same `repo@ref` share one clone. Nothing survives the process: a
stale cache silently rendering yesterday's data is a far worse failure than a
slow clone.

**Bare, temporary, and discarded.** The clone lands in a temporary directory and
is removed when the render ends.

## Confirmation

A clone is a network access, so it goes through the same gate a remote fetch
does — listed before evaluation, alongside every other source that leaves the
machine, and refused by default under `--defaults` or in CI.

This is not a formality. A clone is performed with **your Git credentials**: your
SSH agent and your credential helper, against whatever host the template names.
That is more capability than an anonymous HTTP fetch, not less, and it is why a
`git` source cannot opt out of the confirmation by being spelled differently.

Granting it:

- answer the prompt;
- pass `--trust` for one run;
- add the template to `[trust]` in `~/.config/git-tpl/config.toml` — see
  [Configuration](../reference/configuration.md). A `[trust]` entry names the
  *template*, and covers everything that template declares.

A refused source is [`tpl::data::untrusted`](../reference/diagnostics.md), and
the render stops. It does not proceed with the data missing.

## Provenance

```
Data-Source: licenses = git:https://github.com/acme/tpl-data@v2.1.0:licenses.toml@4f2c1a9
```

The trailing short oid is the commit the ref actually resolved to. For a tag or
a SHA it says the same thing the declaration did; for a branch it is the only
record of which bytes were used. Read it back with:

```sh
git show --no-patch refs/tpl/<id>
```

## Reproducibility

A `ref` naming a branch makes the same template revision render two different
trees on two different days. git-tpl records which commit it used, so the
difference is always *explainable* — but explaining a difference is not the same
as not having one.

Pin a tag or a SHA when the rendering has to be reproducible. See
[Reproducibility](reproducibility.md).

## Failures

| What happened | Code |
|---|---|
| The declaration does not name a file — a missing `ref` or `path`, an scp-style source with no keys, a path that leaves the repository | `tpl::data::invalid_git_source` |
| The repository could not be cloned, the ref did not resolve, or the path is not in the tree | `tpl::data::load` |
| The file is not valid in its format | `tpl::data::parse` |
| The clone was not authorised | `tpl::data::untrusted` |

A failure stops the render. There is no fallback to a cached copy, an empty
table, or the last known value — the rendered tree becomes a commit, and a
plausible tree built from data the template did not get is worse than no tree.
