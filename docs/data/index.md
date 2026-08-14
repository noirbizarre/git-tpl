# Data sources

Templates often need reference data: a list of licences, the language versions a
CI matrix should cover, an organisation's project categories. Data sources load
that data once and make it available to both the question engine and the
renderer.

```toml
[data.licenses]
source = "data/licenses.toml"

[questions.license]
type = "choice"
prompt = "License"
choices_from = "data.licenses.ids"
```

```jinja
{% for id in data.licenses.ids %}
{{ id }}: {{ data.licenses.names[id] }}
{% endfor %}
```

## Why a subsystem, and not a template function

Because a `load_file()` or `http_get()` available to expressions would be a
security hole and a reproducibility hole at the same time. A template could read
anything on your disk, or fetch anything, at render time, invisibly.

Instead, a template *declares* the data it wants. The data layer owns:

- resolution — turning a `source` into something concrete
- loading — reading a file, or making a request
- parsing — TOML, JSON and YAML, into structured values
- caching — one fetch per source per run, no matter how many things use it
- validation — a parse failure names the source
- provenance — recording what contributed to a rendering
- error handling

Expressions consume the result. They cannot cause a load.

## Kinds

```
DataSource
    ├── TemplateFile   a path in the template repository   (the common case)
    ├── LocalFile      a path in the project               (rare, deliberate)
    └── Remote         an http(s) URL
```

The kind is inferred from `source`:

| `source` | Kind |
|---|---|
| `data/licenses.toml` | TemplateFile — relative to the template root |
| `https://example.com/licenses.toml` | Remote |
| `./project-data.toml` | LocalFile — relative to the project root |

An explicit `kind` disambiguates when needed:

```toml
[data.overrides]
source = "config/tpl-data.toml"
kind = "local"
```

See [Local data](local.md) and [Remote data](remote.md).

## Formats

TOML, JSON and YAML, chosen by the file extension, overridable:

```toml
[data.registry]
source = "https://example.com/registry"
format = "json"
```

| Extension | Format |
|---|---|
| `.toml`, anything else | TOML |
| `.json` | JSON |
| `.yaml`, `.yml` | YAML |

Three formats is a deliberate limit, and adding a fourth needs a real reason.
All three have exact type mappings, and whichever you pick, the same file
produces the same context.

### About YAML

YAML is accepted as **YAML 1.2**, which is not the YAML that earned the
reputation. Under the older 1.1 rules `no` resolved to `false`, `on` to `true`
and `12:30:00` to the integer 45000 — the kind of thing that changes a rendered
tree without changing a character of the template. None of that happens here:

```yaml
country: no       # the string "no", not false
at: 12:30:00      # the string "12:30:00"
mode: 0755        # the string "0755"
enabled: true     # a boolean, because it was written as one
```

Two further points, because a data source is untrusted input:

- **Duplicate keys, multiple documents and unbounded alias expansion are
  refused**, rather than resolved to whichever answer the parser happened to
  reach first.
- **A tag is not an instruction.** `!!python/object:os.system` is the classic
  YAML exploit; here the tag is dropped and the scalar kept, because git-tpl
  constructs nothing from data.

One thing YAML 1.2 removed that you may miss: the **merge key `<<` is not
merged**. Anchors and aliases work, but `<<: *base` leaves a literal `<<` key in
the mapping rather than folding its contents in. If you want shared fragments,
alias the whole node.

## Types are preserved

Data enters the context as structured values. A table is a table, an array is an
array, `8080` is an integer:

```toml
# data/ci.toml
[versions]
rust = ["1.88", "stable"]
timeout = 30
strict = true
```

```jinja
{% if data.ci.versions.strict %}
timeout-minutes: {{ data.ci.versions.timeout }}
{% endif %}
```

Nothing is stringified on the way in.

## Dynamic sources

A `source` may itself be an expression:

```toml
[data.frameworks]
source = "data/frameworks/{{ project_type }}.toml"
```

It participates in the dependency graph like everything else, so
`project_type` is asked before the source is resolved, and a question depending
on `data.frameworks` is asked after it is loaded. Cycles are caught up front.

## Loading order

Data sources are loaded lazily, in dependency order, and each is loaded at most
once per run. Three questions using `data.licenses` cause one read.

A source nothing references is never loaded at all — which is why a template can
declare a remote source used only by one branch of a conditional without making
every user pay for it.

## Failures

A data source that cannot be loaded is an error that stops the render. Rendering
with a partially-loaded context would produce a tree that looks plausible and is
wrong, and that tree would become a commit.

```
Could not load template data source `licenses`.

  Template:  rawtools/rust-library
  Source:    https://example.com/licenses.toml
  Kind:      remote
  Reason:    HTTP request failed: connection timed out
```

## Provenance

Each data source that contributed to a rendering is recorded in the rendered
commit's trailers:

```
Data-Source: licenses = template:data/licenses.toml@8b3e7d1
Data-Source: registry = remote:https://example.com/registry.json
```

So you can always answer "what produced this tree?" from Git alone. See
[Reproducibility](reproducibility.md).
