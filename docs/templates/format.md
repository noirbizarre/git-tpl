# Template format

A template is a normal Git repository. Nothing registers it, nothing packages it,
and any Git URL that libgit2 can clone will do — including a local path.

## Layout

```
rust-library-template/
├── template.toml          ← the manifest
├── data/                  ← optional structured data
│   ├── licenses.toml
│   └── defaults.toml
├── template/              ← everything here is rendered
│   ├── Cargo.toml.jinja
│   ├── README.md.jinja
│   ├── src/
│   │   └── lib.rs.jinja
│   └── .github/
│       └── workflows/
│           └── ci.yml
├── README.md              ← the template's own README, not rendered
└── LICENSE                ← likewise
```

Only `template/` is rendered. The rest of the repository — the template's own
README, its CI, its license — is invisible to the projects that use it.

The rendered subdirectory is configurable:

```toml
root = "src"
```

## The manifest

`template.toml` at the repository root.

```toml
name = "rust-library"
description = "A small Rust library"
root = "template"

[data.licenses]
source = "data/licenses.toml"

[questions.project_name]
type = "string"
prompt = "Project name"

[computed]
package_name = "{{ project_name | lower | replace(' ', '-') }}"
```

### Top level

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | *required* | The template's name. Used in output, and as the default template id. |
| `description` | string | — | One line, shown when prompting. |
| `root` | string | `"template"` | The subdirectory that gets rendered. |

`[questions]`, `[computed]` and `[data]` are covered in
[Questions](questions.md), [Computed values](computed.md) and
[Data sources](../data/index.md).

### `[data.<name>]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `source` | string | *required* | Where the data comes from. May be an expression. |
| `kind` | string | inferred | `template`, `local` or `remote`. Required when `source` only becomes a URL after interpolation. |
| `format` | string | inferred | `toml`, `json` or `yaml`. Inferred from the extension. |
| `sha256` | string | — | The expected digest of the content, as 64 hex characters. A mismatch stops the render. |

## Rendering rules

### `.jinja` files are templates

`Cargo.toml.jinja` is rendered and written as `Cargo.toml`. The suffix is
stripped, and only that suffix — `a.jinja.jinja` becomes `a.jinja`.

### Everything else is copied byte-for-byte

`.github/workflows/ci.yml` lands unchanged. This matters more than it sounds:
GitHub Actions files are full of `${{ }}`, and a tool that rendered every file
would mangle them.

If you *want* to template a workflow, name it `ci.yml.jinja` and escape the
Actions syntax with `{% raw %}`.

### Paths are templates too

Every path segment is itself rendered:

```
template/src/{{ package_name }}/mod.rs.jinja
```

A segment that renders to the empty string causes that entry — and, for a
directory, everything beneath it — to be skipped:

```
template/{% if ci %}.github{% endif %}/workflows/ci.yml
```

That is how you make a whole subtree conditional.

!!! warning "A rendered path may not escape the tree"

    A path segment that renders to `..`, to an absolute path, or to something
    containing a `/` is an error, not a traversal. The rendered tree is built
    directly as a Git tree, so this is caught before anything is written, but it
    is rejected explicitly rather than left to chance.

### Binary files

A file with a NUL byte in its first 8 KiB is treated as binary and copied
verbatim, even if it is named `.jinja`. Rendering a PNG would corrupt it, and the
failure would be silent.

### Permissions

The executable bit is preserved. Git records nothing else about a file's mode, so
nothing else can be.

### Determinism

Byte-identical output for identical inputs, always. See
[Determinism](../concepts/determinism.md) for what that rules out.

## The template context

Inside a `.jinja` file, these names are available:

| Name | What it is |
|---|---|
| *(answers, by name)* | Every answered question, at the top level. `{{ project_name }}`. |
| *(computed, by name)* | Every computed value, at the top level. `{{ package_name }}`. |
| `data` | Loaded data sources. `{{ data.licenses }}`. |
| `template` | Template metadata: `template.name`, `template.description`. |

Answers and computed values are at the top level because that is what templates
actually read, and `{{ answers.project_name }}` is noise. They share one
namespace, and a computed value may not reuse an answer's name — that is an
error at load time, not a silent shadow.

Full detail: [Template context](context.md).
