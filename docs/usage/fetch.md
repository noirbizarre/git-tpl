# `git tpl fetch`

Retrieve template refs from a remote.

```sh
git tpl fetch [--remote <name>]
```

Fetches `refs/tpl/*` into `refs/remotes/<remote>/tpl/*`, using an explicit
refspec:

```
+refs/tpl/*:refs/remotes/<remote>/tpl/*
```

## Why this is a separate command

`git fetch` does not retrieve template refs, and `git tpl fetch` is how you ask
for them.

That is deliberate. Someone who clones the project to fix a typo should not
download template state to do it, and should not have to know what a template
ref is. Template refs are opt-in for the people who work with templates.

The refspec is passed per-invocation rather than written into `.git/config`, so
a plain `git fetch` stays plain — including for contributors who never run
git-tpl and never configured anything.

## After fetching

```console
$ git tpl fetch
From github.com:acme/my-project
 * [new ref]  refs/tpl/rawtools-rust-library -> origin/tpl/rawtools-rust-library

The remote copy is 2 commits ahead of your local ref.
Run `git tpl merge --from-remote` to use it, or `git tpl update` to render your own.
```

Fetching **never** moves your local `refs/tpl/<id>`. It brings the remote copy
into a remote-tracking ref and tells you how they relate. What to do about it is
your decision:

- **The remote is ahead** — someone else rendered a newer state. Merge from the
  remote copy to adopt it, or run `git tpl update` to render it yourself. Both
  are valid; rendering yourself is safest if you do not trust the other party's
  answers.
- **The remote is behind** — you have renderings they do not. `git tpl push`.
- **Diverged** — you both rendered independently. See
  [`git tpl push`](push.md#divergence).

## Options

| Option | Meaning |
|---|---|
| `--remote <name>` | Default `origin`, or `tpl.remote`. |
| `--all` | Fetch every `refs/tpl/*`, not just this project's. |
