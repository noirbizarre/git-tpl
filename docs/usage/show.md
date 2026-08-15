# `git tpl show`

The template's version of one file.

```sh
git tpl show <path>
```

It reads `refs/tpl/<id>` and writes what it finds to standard output. Nothing
else: no header, no colour, no summary line.

```console
$ git tpl show Cargo.toml
[package]
name = "demo"
version = "0.1.0"
license = "MIT"
```

The moment it exists for is a conflicted merge. The worktree holds the merge
markers, `HEAD` holds your side — and this holds the template's, in one command,
without a checkout and without remembering the ref name.

```sh
git tpl show src/lib.rs > /tmp/theirs.rs
```

## The plain Git equivalent

```sh
git show refs/tpl/github-com-noirbizarre-rust-library-template:Cargo.toml
```

Identical. `git tpl show` looks up the ref name for you; that is the whole of
what it adds.

## Reading it

Standard output is the file's bytes, verbatim, so it redirects and pipes
cleanly. Anything that goes wrong is a diagnostic on standard error and a
non-zero exit.

Paths are relative to the **repository root**, not to your current directory —
the same paths `git tpl diff --name-only` prints.

## Directories

A path naming a directory lists what is under it, one root-relative path per
line, recursively:

```console
$ git tpl show src
src/lib.rs
```

`git tpl show .` therefore lists the whole rendering, which is a shorter way of
asking what the template actually produces.

!!! note "It reads the ref, never the template repository"

    `show` never clones, never fetches and never contacts the network. It reads
    the rendering that `git tpl update` already committed, so it works offline
    and in the middle of a merge. If the ref is not there, run `git tpl update`
    — or `git tpl fetch`, if the template ref is shared, since template refs are
    never pushed automatically.

## Errors

| Code | Meaning |
|---|---|
| `tpl::ops::no_rendered_ref` | `refs/tpl/<id>` does not exist. Run `git tpl update` or `git tpl fetch`. |
| `tpl::ops::no_such_path` | The path is not in the rendering. `git tpl diff --name-only` lists what is. |
| `tpl::ops::invalid_argument` | The path was absolute, or left the rendering with `..`. |
