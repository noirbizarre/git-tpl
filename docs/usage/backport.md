# `git tpl backport`

Emit a patch that carries a local fix back to the template it came from.

```sh
git tpl backport [<pathspec>...] [--exclude <glob>]... [-o <file>] [--trust]
```

You fixed something in a generated project. The same fix belongs in the
template, so the next project gets it and every existing one gets it on the
next [`update`](update.md). `backport` finds which `.jinja` produced the file
you edited, works out the corresponding change to that source, and gives you a
patch.

## The loop

Start with a fix in the project. Here `ci.yml` is a file the template copies
byte-for-byte, and CI should run on pull requests too:

```console
$ sed -i 's/^on: push/on: [push, pull_request]/' ci.yml
$ git commit -qam "fix: run CI on pull requests"
```

Ask for the patch:

```console
$ git tpl backport
backport main (e754104)

  template/ci.yml <- ci.yml

1 file changed, 1 insertion(+), 1 deletion(-)

apply:       git tpl backport | git -C ../my-template am
```

The summary went to stderr; the patch itself went to stdout. So the command in
that last line is exactly what you run next:

```console
$ git tpl backport | git -C ../my-template am
Applying: tpl: backport from my-service
```

The template now has the fix. Back in the project, it arrives the ordinary way:

```console
$ git tpl update

Template:  ../my-template
Revision:  main (e754104) → main (937573e)

Updated refs/tpl/my-template

  modified  ci.yml

Your working tree was not modified.

$ git tpl merge
Merged refs/tpl/my-template into the current branch
Merge commit e91261a.
```

Git merges your change with the identical change now coming from upstream, and
there is nothing left to send:

```console
$ git tpl backport
Nothing to backport: the project matches the template's rendering.
```

That is the whole feature.

## Applying it

`git am` reads a mailbox on stdin, so the pipe needs nothing from git-tpl:

```sh
git tpl backport | git -C ../my-template am
```

To read the patch before applying it — recommended, since you are about to
change something every project shares — write it out first:

```sh
git tpl backport -o backport.mbox
less backport.mbox
git -C ../my-template am ../my-service/backport.mbox
```

Either way it is `git am` that applies the patch, in your clone, where
`git am --abort`, `git am -3` and `git am --skip` all work as usual.

**git-tpl never applies the patch, and never writes to the template.** There is
no `--to` flag and there will not be one. Two reasons, both structural:

- A template resolved from a remote is a throwaway clone in a temporary
  directory. Writing into it writes into a directory about to be deleted.
- Applying a patch is reconciliation, and git-tpl contributes none of its own —
  see [ADR-002](../adr/002-no-custom-reconciliation.md). `git am` already does
  it, better, in the repository where you can review the result.

git-tpl does print the exact command it declines to run, built from the source
in your [configuration](../configuration.md). If the template is a URL
rather than a path on this machine, there is no clone to name and the hint says
`<your-template-clone>` instead.

## What it compares

Not the project against the template — one is `.jinja` sources and the other is
output, so there is nothing to compare. `backport` compares two *rendered*
trees:

```
refs/tpl/<id>       the tree the template produces at the recorded revision
        ↓
   your worktree    the same tree, plus whatever you changed
        ↓
   the difference   your local divergence — the thing worth sending upstream
```

The baseline is the revision the project actually rendered, which is why there
is no `--ref`. Diffing against any other revision would fold the template's own
movement into the patch, and you would send upstream a revert of upstream.

For the same reason there is no `--answer`: the rendering exists to reproduce
the tree you were given, so it uses the answers in
[`.config/git.tpl.toml`](../configuration.md). Both flags are refused
by the parser rather than accepted and ignored.

Uncommitted work is included. You have just made the fix, and requiring a
commit before you can see the patch would be a step for its own sake.

## Paths in the patch

Patch paths are relative to the **template repository** root, so they carry the
render root as a prefix — `template/ci.yml`, not `ci.yml`:

```diff
diff --git a/template/ci.yml b/template/ci.yml
--- a/template/ci.yml
+++ b/template/ci.yml
```

That is what makes a plain `git am` correct: its default `-p1` strips the
`a/`, leaving `template/ci.yml`, which is where the file lives. You need
`--directory` only if your clone is rooted somewhere below the template
repository root, which is unusual.

The `.jinja` suffix is restored too, so a patch to `README.md` in your project
edits `template/README.md.jinja` in the template.

## Selecting what to send

Positional arguments are Git pathspecs, matched against the paths as you see
them in the project:

```sh
git tpl backport ci.yml .github/
```

`--exclude` removes paths, and is repeatable:

```sh
git tpl backport --exclude '*.lock' --exclude 'docs/**'
```

A single `*` does not cross a `/`; `**` does. A bare name like
`--exclude Cargo.lock` matches at any depth.

Three kinds of change are handled specially:

| Change | Default | Why |
|---|---|---|
| You edited a file the template produces | backported | The case the command exists for. |
| You **deleted** a file the template produces | reported, not backported | Removing a file from a template removes it from every project that renders it. Far too blunt to infer from one project's worktree. |
| You **added** a file the template does not produce | ignored, unless you name it | Otherwise every file your project owns would be a candidate. Named explicitly, it becomes a new template file — and *not* a `.jinja`, since nothing was substituted into a file the template has never seen. |

## When it refuses

A backport that guesses ships a broken template to every downstream project at
once. That is strictly worse than editing the template by hand, which is what
you would have done anyway — so `backport` refuses rather than guessing, and
every refusal names that fallback.

### The change is on a line the template renders

The commonest one. Here the heading comes from a question:

```jinja
# {{ project_name }}
```

If you rename the project by editing the rendered `README.md`, there is no
change to send: you changed an *answer*, not the template. Reversing `acme`
back into `{{ project_name }}` would rename the heading for everyone.

```console
$ git tpl backport
tpl::backport::substituted_region

  x `README.md` was changed where the template substitutes a value
  help: line 1 of `README.md` is produced by an expression in
        `README.md.jinja`, not copied from it, so there is no one-to-one
        change to send upstream. Edit `README.md.jinja` by hand, or restrict
        the backport with a pathspec.
```

Note that this is per *line*, not per file. A `.jinja` file backports fine as
long as your change lands on lines it copies verbatim — which most prose and
most configuration is:

```console
$ git tpl backport
backport main (937573e)

  template/README.md.jinja <- README.md

1 file changed, 1 insertion(+), 1 deletion(-)
```

```diff
--- a/template/README.md.jinja
+++ b/template/README.md.jinja
@@ -2,4 +2,4 @@

 A generated service.

-Run the tests before pushing.
+Run the tests and the linter before pushing.
```

The `{{ project_name }}` heading is untouched and still a placeholder.

### The patch does not render back to your file

Every patch is checked before it is emitted: the patched template source is
rendered, and the result must equal your file exactly. Because
[rendering is deterministic](../concepts/determinism.md), a successful check is
a proof rather than a guess. `tpl::backport::round_trip` means the check failed
— most often because the change landed against a region a `{% if %}` collapsed
— and sending it would change what the template produces for everyone.

### Other refusals

| Code | When |
|---|---|
| `tpl::backport::binary` | A changed file is binary. A text patch cannot carry it; copy it into the template by hand. |
| `tpl::backport::stale_rendering` | `.config/git.tpl.toml` was edited without re-rendering, so `refs/tpl/<id>` is not what the answers produce and every line would be measured against the wrong file. Run [`git tpl update`](update.md) first. |
| `tpl::backport::unknown_path` | A named path is neither produced by the template nor present in the project. |

The full list is in [Diagnostic codes](../reference/diagnostics.md#backport),
and the reasoning behind all of it is
[ADR-020](../adr/020-backport-is-a-patch.md).

## Options

| Option | Meaning |
|---|---|
| `<pathspec>...` | Limit the backport to these paths. A file the template does not produce is only considered when named here. |
| `--exclude <glob>` | Leave these paths out. Repeatable. `*` does not cross a `/`, `**` does. |
| `-o`, `--output <file>` | Write the patch here instead of to stdout. |
| `--trust` | Fetch the template's [remote data sources](../data/index.md) without confirming. Per invocation; nothing is recorded. |

There is deliberately no `--ref` and no `--answer`: both would change the
baseline the patch is measured against. See [What it compares](#what-it-compares).

## Machine-readable output

`git tpl --json backport` emits its outcome on stdout as a single JSON object,
with the prose on stderr. The patch travels *in* the payload as `patch`, rather
than beside it — stdout under `--json` is one JSON object, always. The payload
is described in [JSON output](../reference/json.md#backport).
