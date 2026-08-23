# `git tpl fetch`

Retrieve template refs from a remote.

```sh
git tpl fetch [--remote <name>] [--dry-run]
```

Fetches `refs/tpl/*` into `refs/remotes/<remote>/tpl/*`, using an explicit refspec:

```
+refs/tpl/*:refs/remotes/<remote>/tpl/*
```

## Why this is a separate command

`git fetch` does not retrieve template refs, and `git tpl fetch` is how you ask for them.

That is deliberate.
Someone who clones the project to fix a typo should not download template state to do it, and should not have to
know what a template ref is.
Template refs are opt-in for the people who work with templates.

The refspec is passed per-invocation rather than written into `.git/config`, so a plain `git fetch` stays
plain — including for contributors who never run git-tpl and never configured anything.

## After fetching

```console
$ git tpl fetch
From github.com:acme/my-project
 * [new ref]  refs/tpl/github-com-noirbizarre-rust-library-template -> origin/tpl/github-com-noirbizarre-rust-library-template

The remote copy is 2 commit(s) ahead of your local ref.

Adopt it, or render your own:
  git merge refs/remotes/origin/tpl/github-com-noirbizarre-rust-library-template
  git tpl update
```

Fetching **never** moves your local `refs/tpl/<id>`.
It brings the remote copy into a remote-tracking ref and tells you how they relate.
What to do about it is your decision:

- **The remote is ahead** — someone else rendered a newer state. `git merge` the remote-tracking ref to adopt it,
  or run `git tpl update` to render it yourself. Both are valid; rendering yourself is safest if you do not trust
  the other party's answers.
- **The remote is behind** — you have renderings they do not. `git tpl push`.
- **Diverged** — you both rendered independently. See [`git tpl push`](push.md#divergence).

## Options

| Option | Meaning |
|---|---|
| `--remote <name>` | Default `origin`, or `tpl.remote`. |
| `--dry-run` | Report the refspec and remote; transfer nothing. |

## Machine-readable output

`git tpl --json fetch` emits its outcome on stdout as a single JSON object, with the prose on stderr.
The payload is described in [JSON output](../reference/json.md#fetch).
