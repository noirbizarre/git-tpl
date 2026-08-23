---
name: git-tpl
description: |
  Bootstrap a new project from a git-tpl template,
  adopt git-tpl into an existing project,
  check for and merge template updates,
  and backport a local fix upstream — by driving the `git tpl` CLI.
  Use when the user mentions git-tpl, refs/tpl, a template's `template.toml`,
  or asks to scaffold, sync, or update a project from a template repository.
license: MIT
metadata:
  author: Axel Haustant
  version: "1.0"
---

# git-tpl

git-tpl renders a template into a dedicated Git ref (`refs/tpl/<id>`).
Updating the template advances that ref with a normal commit.
You incorporate the changes with an ordinary `git merge` —
there is no reconciliation engine, no patch replay, and no `.rej` file.

```
template  →  rendered Git ref  →  normal Git merge  →  updated project
```

`git tpl update` never touches `HEAD`, the index, or the worktree — only the ref moves.
Applying the result is always a separate, explicit `git tpl merge` (or a plain `git merge refs/tpl/<id>`).
Template refs are never pushed or fetched automatically either — `git push`/`git pull` ignore `refs/tpl/*`.

Full model: <https://noirbizarre.github.io/git-tpl/concepts/git-model/>.

## When to use this skill

- Scaffolding a new repository from a git-tpl template.
- Adopting git-tpl retroactively in a project that already has files.
- Checking whether a template has moved, and merging the update in.
- Resolving a conflict left by a git-tpl merge.
- Sending a local fix back to the template it came from (backport).

## Prerequisites

Verify the binary is on `PATH` before doing anything else:

```sh
git tpl --version   # or: git-tpl --version
```

Both spellings run the same program;
`git-tpl` is handy when you would rather not depend on Git's subcommand resolution.
If it is missing, point the user at <https://noirbizarre.github.io/git-tpl/getting-started/installation/> —
do not assume it is preinstalled.

## Agent rules

1. **Always pass `--json`** before deciding what to do next, and branch on `error.code`, never on `error.message` —
   messages are free to change.
   The full catalogue is at <https://noirbizarre.github.io/git-tpl/reference/diagnostics/>.
2. **Never run `init`/`update`/`show --dirty` interactively.**
   Build the answer set first with `git tpl --json questions <template>`,
   then supply it with repeated `--answer k=v`, one or more `--answers-from file.toml`, and/or `--defaults`.
   Add `--strict-answers` to catch a typoed key instead of a silent warning.
3. **Preview before committing.**
   `init`, `update`, `fetch`, and `push` all accept `--dry-run` (the JSON payload gains `"dryRun": true`) —
   use it on an unfamiliar template or repository before acting for real.
4. **`update` alone changes nothing you can see.** It only advances `refs/tpl/<id>`.
   Follow it with `git tpl diff` to preview and `git tpl merge` to apply — skipping the merge leaves the update inert.
5. **`git tpl status --json` exits `2`**, not `0` or `1`,
   when a template update is pending (`"templateMoved": true` or `"merged": false`).
   Treat that as a normal branch, not a failure.
6. **`"ok": true` describes the command, not the outcome**, for two commands specifically:
   `lint` (check `errors`/`denied` in its `diagnostics[]`, not just `ok`)
   and `test` (check `summary.failed`).
7. **Conflicts are resolved with plain Git, never invented tooling:**
   `git status`, `git tpl show <path>` (the template's/"theirs" side, without checking out the ref),
   edit, `git add`, `git commit`.
   `git merge --abort` bails out entirely, same as any Git merge.
8. **`backport` never applies its own patch.** It only ever emits one, to stdout or `--output <file>`.
   Hand it to `git -C <template-clone> am` yourself (or give it to the human to review first).
   Under `--json`, `-p`/`--patch` (interactive hunk selection) is refused with `tpl::backport::not_interactive` —
   pass `--unsubstitute` instead of trying to force interactivity,
   or narrow the backport with pathspecs / `--exclude`.
9. **Sharing renderings across a team is explicit.**
   `git tpl fetch` and `git tpl push` move `refs/tpl/*`; nothing else does.
   `push` never forces — a diverged remote must be fetched and merged first, same as any ref.
10. **Never amend, rebase, or force anything onto `refs/tpl/*`.**
    It is append-only by design; treat it as a read-only history you inspect and merge from, not one you rewrite.
11. **Templates cannot execute code.**
    git-tpl runs nothing over a rendering — no hooks, no scripts, no post-render commands.
    If a template's note or README documents a `scripts/bootstrap.sh`, running it is a separate, deliberate step
    for you or the user, never something `init`/`update` does on your behalf.

## Quick reference

| Task | Command |
|---|---|
| See a template's questions before answering them | `git tpl --json questions <template>` |
| Scaffold a new project | `git tpl init <template> <dir> --init --answers-from a.toml --defaults --json` |
| Adopt git-tpl in an existing project | `git tpl init <template> --answers-from a.toml --defaults --json` |
| Check whether the template moved | `git tpl status --json` (exit `2` = pending) |
| Advance the rendered ref | `git tpl update --defaults --json` |
| Preview what merging would change | `git tpl diff --json --stat` |
| Apply the update | `git tpl merge --json` |
| See the template's side of a conflict | `git tpl show <path>` |
| Send a local fix upstream | `git tpl backport --unsubstitute --json` |
| Share a rendering with the team | `git tpl push` / `git tpl fetch` |

## Workflows

### Bootstrap a new project

```sh
git tpl --json questions https://github.com/org/some-template \
  | jq -r '.questions[] | select(.default != null and .defaultIsExpression == false)
           | "\(.name) = \(.default | tojson)"' > answers.toml
# edit answers.toml with the real values

git tpl init https://github.com/org/some-template my-project \
  --init --dry-run --json   # preview: nothing is created yet

git tpl init https://github.com/org/some-template my-project \
  --init --answers-from answers.toml --defaults --json
```

`--init` creates the directory and the Git repository if they do not exist.
On success `init` has already merged the rendered commit into the branch —
there is nothing further to apply, unlike `update`.

### Adopt git-tpl in an existing project

Same command, run inside the existing repository, **without** `--init`:

```sh
cd my-existing-project
git tpl init https://github.com/org/some-template --answers-from answers.toml --defaults --json
```

There is no separate "migrate" command because there is no separate behavior:
the template's rendering is merged with unrelated histories allowed, and Git reconciles by content —
a file identical to the rendered one merges silently,
a file that differs conflicts **only on the differing lines**,
and a file the template adds is staged.
Expect small conflicts and resolve them with plain Git (`git status`, edit, `git add`, `git commit`).
This first merge is the only awkward one:
afterward the rendered commit is an ancestor of the branch,
so every later `update` is a small diff against a real merge base.

### Check for and apply a template update

```sh
git tpl status --json          # exit 2 / "templateMoved": true → work to do
git tpl update --dry-run --json
git tpl update --defaults --json
git tpl diff --json --stat     # preview the merge; "conflicts" array if any
git tpl merge --json
```

### Resolve a merge conflict

```sh
git status                     # which files conflicted
git tpl show <path>            # the template's side, without a checkout
# edit the file to reconcile both sides
git add <path>
git commit
# or, to bail out entirely:
git merge --abort
```

Conflicts here mean exactly what they mean anywhere else:
both sides changed the same region since the last time they agreed.
There is no git-tpl-specific resolution step.

### Send a local fix back to the template (backport)

```sh
# after fixing something in the project and committing it:
git tpl backport --unsubstitute --json   # --unsubstitute: no prompts, safe for CI/agents
```

Read the `patch` field (or use `-o file.patch` for a plain-file version),
then apply it in the template's own clone — git-tpl never does this for you:

```sh
git tpl backport --unsubstitute | git -C ../some-template am
```

If it refuses with `tpl::backport::substituted_region`,
the change landed on a line the template computes from an answer (e.g. a project name), not one copied verbatim —
there is no fix to send; edit the template's `.jinja` source by hand instead.

## Commands used above

| Command | Synopsis | Key JSON fields |
|---|---|---|
| `questions` | `git tpl --json questions <template>` | `questions[]{name,default,defaultIsExpression,when,choices}` |
| `init` | `git tpl init <template> [<dir>] [--init] [--answer k=v]... [--answers-from f]... [--defaults] [--dry-run]` | `id`,`ref`,`revision`,`commit`,`changes[]`,`merge{result,...}` |
| `status` | `git tpl status` | `templateMoved`,`merged`,`availableRevision`,`renderedRevision`,`worktreeClean`,`remote{ahead,behind}` |
| `update` | `git tpl update [--answer k=v]... [--defaults] [--dry-run] [--push]` | `result`(`upToDate`\|`updated`),`previousRevision`,`revision`,`changes[]` |
| `diff` | `git tpl diff [--stat] [--name-only] [--exit-code] [-- <path>...]` | `conflicts[]`,`changes[]{path,kind,insertions,deletions}` |
| `merge` | `git tpl merge [--no-commit] [-m <msg>]` / `git tpl merge --abort` | `result`(`upToDate`\|`fastForward`\|`merged`\|`staged`\|`conflicted`),`commit`,`conflicts[]` |
| `show` | `git tpl show <path>` | no envelope — stdout **is** the file's bytes |
| `backport` | `git tpl backport [<pathspec>...] [--exclude g]... [-o file] [--unsubstitute]` | `result`(`patched`\|`nothingToBackport`),`patch`,`unsubstituted[]` |
| `fetch` | `git tpl fetch [--remote name] [--dry-run]` | `state`(`absent`\|`synced`\|`diverged`\|`behind`\|`ahead`),`relation{ahead,behind}` |
| `push` | `git tpl push [--remote name] [--dry-run]` | `remote`,`ref` |

Every command's full flag set and payload shape: <https://noirbizarre.github.io/git-tpl/reference/json/>.

## Output conventions

stdout carries the machine-readable payload — one JSON object under `--json`,
or raw file bytes for `show`/`completion`/`man`
(which have no JSON envelope at all, even under `--json`, because their stdout already *is* the payload).
stderr carries human prose and warnings, which are never suppressed, even by `--json` or `--quiet`.

## Exit codes and error recovery

| Code | Meaning | Agent action |
|---|---|---|
| `0` | Success | Continue. |
| `1` | Failure | Read `error.code` from the JSON envelope, look it up in the diagnostics reference, and act on the `help` text. |
| `2` | Only from `status`: an update is pending (`templateMoved` or not `merged`) | Not a failure — decide whether to `update`/`merge` now. |

Diagnostic codes: <https://noirbizarre.github.io/git-tpl/reference/diagnostics/>.

## Known limitations

- git-tpl runs nothing over a rendering — no build, no lint, no test.
  After `update`/`merge`, verify the result with the project's own tools (e.g. `cargo build`, `npm test`, `actionlint`).
- There are no custom merge strategies.
  A conflict is an ordinary Git conflict; git-tpl contributes no reconciliation logic of its own.
- `backport` only ever produces a patch;
  it never writes to the template repository, and there is no flag that will make it do so.
