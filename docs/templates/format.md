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
├── macros.jinja           ← a shared partial, not rendered
├── macros/
│   └── rust.jinja         ← likewise
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

The exception, and the only one, is a `.jinja` file outside `template/`: it is
still never rendered into a project, but it *is* importable as a
[shared partial](#shared-partials).

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

### Shared partials

A `.jinja` file **outside** the rendered subdirectory is a partial. It is never
written into a project, and it can be imported by name from any file that is:

```
macros.jinja                             ← the partial
template/README.md.jinja                 ← imports it
```

```jinja
{% import "macros.jinja" as m %}
{{ m.badge(project_name) }}
```

`{% include %}` works the same way. A partial is named by its path relative to
the **repository root**, not to the rendered subdirectory, so a partial in a
directory is `{% import "macros/rust.jinja" %}`.

Being outside the rendered subdirectory is the whole rule, and it is what keeps
a macro definition from landing in every generated project. There is no manifest
key and no reserved filename.

Two consequences worth stating:

- A `.jinja` file **inside** the rendered subdirectory is an output file and is
  *not* importable. One file, one meaning.
- Only `.jinja` files are loadable. `{% include "data/licenses.toml" %}` does not
  work; declare a [data source](../data/index.md) instead, which knows how to parse it.

Partials are read from the same pinned revision as everything else, so editing
one changes the rendered tree and advances the ref. They are available to
manifest expressions too — a `computed` value may `{% import %}` the same macro
a file does.

!!! tip "Getting the name wrong"

    A failed import lists the partials that do exist. The usual cause is a path
    written relative to `template/` instead of the repository root.

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

## Publishing

There is nothing to package and nowhere to register. Publishing a template is
`git push`, and sharing it is sharing the URL someone passes to `git tpl init`.

What a visitor lands on is the repository itself, so the template's own
`README.md` and `LICENSE` — the files outside `template/` that are never
rendered — are the ones worth writing. They are the only description anyone
gets before cloning.

On GitHub, add the `git-tpl` repository topic:

```sh
gh repo edit --add-topic git-tpl
```

or Settings → *About* → *Topics*. The template then appears at
[github.com/topics/git-tpl](https://github.com/topics/git-tpl), which is how
templates are found.

The topic is a discovery convention between humans, nothing more. git-tpl never
queries GitHub, has no registry and resolves no names: `git tpl init` takes a
Git URL and clones it. A template without the topic works exactly as well — it
is just harder to come across.
