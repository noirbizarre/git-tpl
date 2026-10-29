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

Review its provenance and ancestry before conditionally fast-forwarding the local rendered ref.
```

Fetching **never** moves your local `refs/tpl/<id>`.
It brings the remote copy into a remote-tracking ref and tells you how they relate.
What to do about it is your decision:

- **The remote is ahead** — someone else rendered a newer state.
  Review it before [conditionally fast-forwarding your local rendered ref](#fast-forwarding-an-existing-local-ref).
  Merging into your project branch alone does not advance the rendered ref used by update and push.
- **The remote is behind** — you have renderings they do not. `git tpl push`.
- **Diverged** — you both rendered independently. See [`git tpl push`](push.md#divergence).

## Restoring an absent local ref

Fetching does not restore `refs/tpl/<id>`: it only retrieves a remote-tracking candidate.
Before the first update in a fresh clone, restore the original rendering so the next commit keeps its ancestry.
Use full project history (`git fetch --unshallow` for a shallow clone).

For a shared rendering, fetch and select its candidate:

```sh
git tpl fetch
ref=refs/tpl/<id>
candidate=$(git rev-parse 'refs/remotes/origin/tpl/<id>^{commit}')
git show --no-patch --format=full "$candidate"
git log --oneline "$candidate"
```

Review the `Template-Source`, `Template-Commit` and `Answers-Digest` trailers against your attachment.
Check which rendering is already merged with `git merge-base --is-ancestor "$candidate" HEAD`;
a newer shared candidate may legitimately not yet be merged, so inspect its ancestry back to the applied rendering.
If the ref was local-only, find the original rendered commit in merged history instead:

```sh
git log HEAD --topo-order --format=full --grep='^Template-Source: <configured-source>$'
# Set candidate to the reviewed original rendered commit, not the project merge commit.
candidate=<original-rendered-commit>
git merge-base --is-ancestor "$candidate" HEAD
```

After reviewing the candidate, create the local ref only if it is still absent:

```sh
git update-ref "$ref" "$candidate" ''
git tpl update --defaults
```

The empty expected old value rejects an existing ref, including one created concurrently.
If it fails, inspect the existing ref before proceeding.
Re-rendering an equal tree cannot recreate the original commit or merge ancestry.
If you already created an orphan rendering, this absent-only procedure does not reconcile it;
preserve both histories and see [divergence](push.md#divergence).

## Fast-forwarding an existing local ref

After reviewing the remote rendering's provenance and answers, capture both tips and require ancestry:

```sh
ref=refs/tpl/<id>
old=$(git rev-parse "$ref^{commit}")
candidate=$(git rev-parse 'refs/remotes/origin/tpl/<id>^{commit}')
git merge-base --is-ancestor "$old" "$candidate" &&
  git update-ref "$ref" "$candidate" "$old"
```

The ancestry check permits only a fast-forward; the expected old tip rejects concurrent local movement.
If either command fails, stop and inspect both histories.
Use the [absent-only procedure](#restoring-an-absent-local-ref) when no local ref exists.
For genuine divergence, preserve both histories: [no automatic reconciliation is available](push.md#divergence).
After adoption, `git tpl diff` and `git tpl merge` preview and apply the rendering to your project branch.

## Options

| Option | Meaning |
|---|---|
| `--remote <name>` | Default `origin`, or `tpl.remote`. |
| `--dry-run` | Report the refspec and remote; transfer nothing. |

## Machine-readable output

`state: "absent"` means one or both compared refs are missing.
`localPresent` and `remotePresent` distinguish all presence combinations;
`relation` is null unless both the local ref and fetched remote-tracking copy exist.
When the local ref is missing but a shared copy was fetched, fetch reports that copy as available
and leaves the local ref absent.
The other states remain `synced`, `ahead`, `behind` and `diverged`.

`git tpl --json fetch` emits its outcome on stdout as a single JSON object, with the prose on stderr.
The payload is described in [JSON output](../reference/json.md#fetch).
