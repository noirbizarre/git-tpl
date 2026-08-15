# Local template development

Developing a template against a real project is a first-class workflow, not an
afterthought. You should never have to tag a release to find out whether a change
renders correctly.

## The loop

```sh
cd my-project
git tpl init ../my-template
```

A local path is a template source like any other. From then on:

```
edit the template
       ↓
git tpl update       ← re-renders, advances refs/tpl/<id>
       ↓
git tpl diff         ← exactly what changed
       ↓
git tpl merge        ← take it, or don't
```

Nothing about this differs from using a published template. The same code path
resolves a local path and a remote URL.

## Uncommitted changes in the template

By default, git-tpl renders the template's **committed** `HEAD`. A change you
have not committed does not appear.

That is the right default because the template revision is recorded in the
rendered commit's trailers, and "the state of a directory at some past moment" is
not a revision anyone can resolve later.

While iterating, that is exactly the wrong default, so:

```sh
git tpl update --dirty
```

reads the template's **working tree** instead. The resulting commit is marked:

```
Template-Ref: <worktree>
Template-Commit: 8b3e7d1cd4a...      ← the HEAD it was based on
Template-Dirty: true
```

`git tpl status` reports it too:

```console
Template:  ../my-template
Revision:  8b3e7d1 (+ uncommitted changes)
Ref:       refs/tpl/my-template
```

!!! warning "`--dirty` renderings are not reproducible"

    Nobody else can reproduce a tree rendered from your uncommitted working
    directory, and neither can you once you amend it. Use it while iterating;
    commit the template before an update you intend to keep.

    Because template refs are [append-only](../concepts/git-model.md#append-only),
    a `--dirty` commit you no longer want stays in the ref's history. It is
    harmless — the next clean update supersedes it — but it is there.

## Testing a template in a scratch project

The fastest loop does not involve your real project at all:

```sh
cd "$(mktemp -d)"
git init demo && cd demo
git tpl init ~/src/my-template --defaults
```

`--defaults` takes every default without prompting, so you can re-run this as
often as you like. Delete the directory when done.

To exercise a specific answer combination:

```sh
git tpl init ~/src/my-template \
  --answer project_type=application \
  --answer cli=true \
  --defaults
```

## Checking what a template will ask

```sh
git tpl init ~/src/my-template --dry-run
```

resolves the manifest, builds the dependency graph and reports the questions in
the order they would be asked — without creating anything. This is the cheapest
way to find a cycle or a typo in an expression, since both are caught at graph
construction.

## Pointing an existing project at a different template

Edit `[template] source` in `.config/git.tpl.toml` and run `git tpl update`.

The rendered ref is keyed by the template id, so pointing at a *renamed* source
whose id differs starts a new ref with no shared history — and because the first
merge from it has no common ancestor, everything that differs between the two
renderings conflicts, including files you customised that neither template
changed. If the two templates are genuinely the same template at a new address,
keep the id stable:

```toml
[template]
source = "https://github.com/noirbizarre/rust-library-template"  # moved here
id = "gitlab-com-noirbizarre-rust-library-template"              # derived from the old address
ref = "main"
```

Without the `id` line, the new `source` would derive
`github-com-noirbizarre-rust-library-template` and the ref would start over.

See [Configuration](../configuration.md#template).
