# Local data

## Template data — the common case

Data files stored in the template repository. This is the case that matters most,
and the default interpretation of a relative `source`.

```
rust-library-template/
├── template.toml
├── data/
│   ├── licenses.toml
│   └── ci.toml
└── template/
```

```toml
[data.licenses]
source = "data/licenses.toml"
```

The path is relative to the **template repository root**, not to `template/` and
not to your project.

### Read from the template's Git tree

Template data is read from the template repository *at the resolved revision* —
from the Git tree, not from a checkout on disk.

That is what makes the template repository a self-contained, pinned data source:
`ref = "v1.4.0"` pins the template files *and* its data, together, to one commit.
There is no way for them to drift apart, and no separate pinning mechanism to
get wrong.

It also means a template's data is versioned with the template. A change to
`data/licenses.toml` is a template change, appears in `git tpl status` as the
template having moved, and produces a new rendering — exactly as editing a
`.jinja` file would.

!!! note "`--dirty` applies here too"

    `git tpl update --dirty` reads template data from the template's working
    tree, along with everything else. See
    [Local template development](../templates/local-development.md).

## Project data

Data from the *project* being rendered, rather than from the template. Explicit,
and rare.

```toml
[data.overrides]
source = "./config/tpl-data.toml"
kind = "local"
```

A leading `./` or `../` marks it as project-local; `kind = "local"` states it
outright and is clearer.

The path is relative to the project root — the directory containing
`.config/git.tpl.toml`.

### When to use it, and when not to

Use it for data that genuinely belongs to the project and would be absurd as
answers — a long list of service endpoints, a table of environments, anything
structured enough that flattening it into `[answers]` would be painful.

Do not use it to work around a template that should have asked a question. An
answer is recorded, typed, validated, prompted for, and visible in one obvious
file. Project data is none of those things.

!!! warning "Project data breaks the self-contained property"

    A template that requires a project-local file will fail on `git tpl init`
    for anyone who does not already have that file — which is everyone, since
    `init` runs before the project exists.

    Template data has no such problem: it ships with the template.

    A template intended for general use should read its data from its own
    repository. Project data is for the case where a specific project feeds a
    specific template, and both are yours.

### Paths may not escape the project

`../../../etc/passwd` is rejected, not resolved. A local data path must stay
within the project root.

## Provenance

Template data records the template commit it was read from:

```
Data-Source: licenses = template:data/licenses.toml@8b3e7d1
```

Project data records the path only — the project's own commit is the containing
commit, so recording it would be circular:

```
Data-Source: overrides = local:config/tpl-data.toml
```
