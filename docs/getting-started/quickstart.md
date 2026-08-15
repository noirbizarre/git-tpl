# Quick Start

This builds a template from scratch and uses it, so that every piece is visible.
If you already have a template, skip to [Use it](#use-it).

## Write a template

A template is a normal Git repository with a `template.toml` at its root and the
files to render in a `template/` directory.

```sh
mkdir rust-library-template && cd rust-library-template
git init
```

`template.toml`:

```toml
name = "rust-library"
description = "A small Rust library"

[questions.project_name]
type = "string"
prompt = "Project name"

[questions.license]
type = "choice"
prompt = "License"
choices = ["MIT", "Apache-2.0"]
default = "MIT"

[questions.ci]
type = "boolean"
prompt = "Enable CI?"
default = true

[computed]
package_name = "{{ project_name | lower | replace(' ', '-') }}"
```

`template/Cargo.toml.jinja`:

```jinja
[package]
name = "{{ package_name }}"
version = "0.1.0"
edition = "2024"
license = "{{ license }}"
```

`template/README.md.jinja`:

```jinja
# {{ project_name }}

Licensed under {{ license }}.
```

And a file rendered only when asked for. Path segments are rendered too, and a
segment that comes out empty skips that entry — and, for a directory,
everything beneath it. So put the condition in the path:

```
template/{% if ci %}.github{% endif %}/workflows/ci.yml
```

Answer `Enable CI?` with no and the whole `.github` subtree is left out.

Commit it:

```sh
git add -A && git commit -m "feat: initial template"
```

## Use it

```sh
mkdir my-project && cd my-project
git init

git tpl init ../rust-library-template
```

git-tpl asks the questions, writes `.config/git.tpl.toml`, renders the template
into `refs/tpl/rust-library-template`, and merges it into your branch.

This works on a project that already has files too — the merge reconciles the
two sides. See [an existing project](../usage/init.md#an-existing-project).

```console
Template:  ../rust-library-template
Revision:  main (4f2c1a9)

Created refs/tpl/rust-library-template

  added     Cargo.toml
  added     README.md
  added     src/lib.rs

Merged into main.

Answers recorded in .config/git.tpl.toml and committed.
```

Look at what happened:

```sh
git log --oneline --graph
git show --no-patch refs/tpl/rust-library-template
```

## Change the template, then update

Back in the template, edit `template/README.md.jinja` and commit.

Then, in your project:

```sh
git tpl status
```

```console
Template:  ../rust-library-template
Ref:       refs/tpl/rust-library-template

Revision:  main (4f2c1a9) → main (8b3e7d1)   template has moved
Rendered:  1 rendering
Merged:    yes
Worktree:  clean

The template has moved. Run:
  git tpl update
```

```sh
git tpl update
```

```console
Template:  ../rust-library-template
Revision:  main (4f2c1a9) → main (8b3e7d1)

Updated refs/tpl/rust-library-template

  modified  README.md

Your working tree was not modified.

Run:
  git tpl diff
  git tpl merge
```

Note "Your working tree was not modified." `update` moved a ref and nothing
else — `git status` is still clean, `HEAD` has not moved.

```sh
git tpl diff     # exactly what merging would bring in
git tpl merge    # bring it in
```

If you had also edited `README.md` yourself, that merge behaves like any other
Git merge: it combines the changes, or it stops with conflict markers and you
resolve them. `git merge --abort` works, because it *is* a merge.

## Where to go next

* [The Git model](../concepts/git-model.md) — what is actually happening
* [Template format](../templates/format.md) — the full manifest
* [Questions](../templates/questions.md) — conditionals, dynamic defaults
* [Local template development](../templates/local-development.md) — the tight loop
