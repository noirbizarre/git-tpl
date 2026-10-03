---
name: git-tpl
description: |
  Bootstrap a new project from a git-tpl template,
  adopt git-tpl into an existing project,
  synchronize a project with its template —
  list and merge template updates, then backport chosen local changes upstream,
  one pull request per change on GitHub —
  by driving the `git tpl` CLI.
  Use when the user mentions git-tpl, refs/tpl, a template's `template.toml`,
  or asks to scaffold, sync, update, or backport from a template repository.
license: MIT
metadata:
  author: Axel Haustant
  version: "1.2"
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
- Sending local changes back to the template they came from (backport).
- **Synchronizing** a project with its template: both directions, in order —
  pull the template's changes in, then push the project's own improvements back out.
  This is a workflow of this skill, not a `git tpl` command; see [Synchronize](#synchronize-a-project-with-its-template).

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
5. **A migration can ride along with an `update`.** `migrations[]` is empty on almost every update; when it isn't,
   the template crossed a version boundary declared under its own `migrations/` directory (ADR-024). Read each
   entry's `message` before merging — it is meant to be shown once, the same as `init`'s `note`. `movedCommit`,
   when present, is an internal rename commit the merge machinery needs; it is not something to inspect or act on.
   A migration always reports `"result": "updated"`, never `"upToDate"`, even if nothing else changed.
6. **`git tpl status --json` exits `2`**, not `0` or `1`,
   when a template update is pending (`"templateMoved": true` or `"merged": false`).
   Treat that as a normal branch, not a failure.
7. **`"ok": true` describes the command, not the outcome**, for two commands specifically:
   `lint` (check `errors`/`denied` in its `diagnostics[]`, not just `ok`)
   and `test` (check `summary.failed`).
8. **Conflicts are resolved with plain Git, never invented tooling:**
   `git status`, `git tpl show <path>` (the template's/"theirs" side, without checking out the ref),
   edit, `git add`, `git commit`.
   `git merge --abort` bails out entirely, same as any Git merge.
9. **`backport` never applies its own patch.** It only ever emits one, to stdout or `--output <file>`.
   Hand it to `git -C <template-clone> am` yourself (or give it to the human to review first).
   Under `--json`, `-p`/`--patch` (interactive hunk selection) is refused with `tpl::backport::not_interactive` —
   pass `--unsubstitute` instead of trying to force interactivity.
   To send some hunks and not others, list them with `--list-hunks` and name the ones to send with
   `--hunk <path>:<id>`; ids are opaque, so copy each `spec` from the listing rather than building one.
   Once any `--hunk` is given, a file with no `--hunk` of its own sends nothing.
   Narrowing by whole file is still `<pathspec>` / `--exclude`.
10. **Sharing renderings across a team is explicit.**
    `git tpl fetch` and `git tpl push` move `refs/tpl/*`; nothing else does.
    `push` never forces — a diverged remote must be fetched and merged first, same as any ref.
11. **Never amend, rebase, or force anything onto `refs/tpl/*`.**
    It is append-only by design; treat it as a read-only history you inspect and merge from, not one you rewrite.
12. **Templates cannot execute code.**
    git-tpl runs nothing over a rendering — no hooks, no scripts, no post-render commands.
    If a template's note or README documents a `scripts/bootstrap.sh`, running it is a separate, deliberate step
    for you or the user, never something `init`/`update` does on your behalf.
13. **Show the user what changes before it lands, and let them choose what leaves.**
    Before `merge`, present the list of incoming template changes and wait for a go-ahead.
    Before any backport, present the candidate changes and let the user pick which to send.
    Never merge, backport, push, or open a pull request on your own judgement of what is "obviously fine".
14. **Never push a branch or open a pull request without explicit confirmation**, each time.
    One backported change is one branch and one pull request — never bundle several, never stack them.

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
| Synchronize (pull, then push back) | [Synchronize](#synchronize-a-project-with-its-template) |
| List the local changes that could go upstream | `git tpl backport --list-hunks --json` |
| Send chosen hunks upstream | `git tpl backport --unsubstitute --json --hunk <path>:<id>... -o x.patch` |
| Send every local change upstream | `git tpl backport --unsubstitute --json` |
| Share a rendering with the team | `git tpl push` / `git tpl fetch` |
| Run a template's own test suite | `git tpl test --json` (check `summary.failed`, not just `ok`) |

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
```

**Stop here and show the user the list of changes before running `merge`.**
Build it from the `diff` payload (`changes[]{path,kind,insertions,deletions}`; the `update` payload's `changes[]`
is the same rendering-to-rendering view), and present it grouped by purpose rather than as a flat file dump:

- what the template changed, a one-line summary per group, with the files each touches and their `+`/`-` counts;
- every `migrations[]` entry's `message`, verbatim (agent rule 5);
- every path in `conflicts[]`, flagged as needing a decision.

Then ask whether to proceed. Only on a yes:

```sh
git tpl merge --json
```

If the user declines, leave the ref where it is — `update` already advanced it, which changes nothing they can see —
and say so.

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

### Send local changes back to the template (backport)

Backport measures the project's working tree (uncommitted work included) against the rendering the ref records,
and emits a patch for the template.
It never decides *which* changes are worth sending, and never touches the template repository:
choosing, grouping, and opening pull requests are your job, in the steps below.

**1. Make the baseline fresh.**
Run the update workflow first.
`tpl::backport::stale_rendering` means `.config/git.tpl.toml` was edited without re-rendering — run `git tpl update`.

**2. List the candidates.**

```sh
git tpl backport --list-hunks --json
```

`plan[]` has one entry per changed file (`rendered`, `source`, `added`), each with `hunks[]`:
`spec` (what `--hunk` takes), `header`, `insertions`, `deletions`, and `lines` (the hunk body, prefixed ` `/`-`/`+`).
Nothing is produced yet, and nothing is refused yet — a file git-tpl would not be able to backport is still listed.
A file the template never produced appears only if you name it as a `<pathspec>`.
`skipped[]` holds what cannot be backported at all (a locally deleted file, a binary file).
Tell the user about those, once.

**3. Group the hunks into changes.**
A *change* is one atomic, scope-consistent unit: one purpose that a maintainer would review, merge, or revert as a
whole — "pin the CI action versions", "fix the README install command".
It may span several files, and a single file's hunks may belong to different changes.
Read the `lines`, not just the paths.
Give each change a short title, a one-sentence summary, and the list of `spec`s it contains.
Do not merge unrelated hunks to keep the number down, and do not split one purpose across two changes.
A hunk you cannot place in a coherent change gets a change of its own, titled honestly.

**4. Ask the user which changes to send.**
Use a multiple-choice prompt — your agent's structured question tool with several selections allowed.
One option per change: its title and summary, with the files it touches, and `+`/`-` totals.
If you have no such tool, print a numbered list and ask for comma-separated numbers.
Select nothing by default.
No selection means stop here: say that nothing was sent.

**5. Build one patch per selected change.**

```sh
git tpl backport --unsubstitute --json \
  --hunk README.md:3f9a1c0be2d4 --hunk ci.yml:a07c55d1e9b3 -o /tmp/pin-ci-actions.patch
```

One invocation per change, each against the same baseline, so the patches are independent of each other.
`--unsubstitute` is what makes this safe for a non-interactive caller;
read `unsubstituted[]` in the payload and carry every entry into the change's description for the reviewer —
a reversed substitution changes what the template produces for every project.

A change can be refused even though the listing showed its hunks:
`tpl::backport::hunk_refused` (the `help` names the hunk) wrapping `substituted_region`, or `round_trip`.
Report it to the user as not backportable automatically — the fix is editing the template's `.jinja` by hand —
and carry on with the other changes.
`tpl::backport::unknown_hunk` means the project changed since step 2: list again.

**6. Deliver each patch.**
Find the template with `template` in the payload (`config.template.source` in `.config/git.tpl.toml`).
Never run `applyCommand` blindly: for a URL it contains the placeholder `<your-template-clone>`.

- **The upstream is on GitHub** (`https://github.com/<owner>/<repo>`, `git@github.com:<owner>/<repo>.git`):
  one pull request *per change*, never stacked.
  With `gh` available and authenticated, in a scratch directory outside the project:

  ```sh
  gh repo clone <owner>/<repo> /tmp/tpl-upstream && cd /tmp/tpl-upstream   # or `gh repo fork --clone` without push rights
  git switch -c backport/pin-ci-actions "$(git symbolic-ref --short refs/remotes/origin/HEAD)"
  git am /tmp/pin-ci-actions.patch
  ```

  Reword the commit to the upstream's own convention (`git log` shows it) so the change reads as one commit with a
  real title.
  Then **show the user the branch's diff and the intended title and body, and ask before pushing and before opening
  the pull request**:

  ```sh
  git push -u origin backport/pin-ci-actions
  gh pr create --title "<title>" --body "<summary, then any un-substituted lines>"
  ```

  Branch from the default branch each time, so every pull request is independent.
  Report the pull request URLs when done.
  If `git am` refuses, report that change as failed, `git am --abort`, and continue with the next.
- **Anything else** (a local path, GitLab, a bare Git server): apply the patch on a fresh branch in a clone of the
  template (`git -C <path> switch -c ... && git -C <path> am <patch>`), and tell the user the branch name.
  Do not push, and do not open a merge request.
- **No `gh`, or not authenticated:** leave the patches and tell the user; do not fall back to another way of
  opening a pull request.

### Synchronize a project with its template

Synchronize is two halves in a fixed order, because the second one is measured against the first:

1. **Pull** — [Check for and apply a template update](#check-for-and-apply-a-template-update):
   show the incoming changes, get a go-ahead, merge, and resolve any conflict.
   Verify with the project's own tools before moving on.
2. **Push** — [Send local changes back](#send-local-changes-back-to-the-template-backport):
   list, group, let the user choose, then one patch and (on GitHub) one pull request per chosen change.

Tell the user at the start that it is two steps and that nothing leaves the machine without their say-so.
Either half can be run alone; skipping the second when the user has no local improvements to share is normal.
A merge that left conflicts unresolved stops the sync: backporting from a half-merged tree measures nothing useful.

## Commands used above

| Command | Synopsis | Key JSON fields |
|---|---|---|
| `questions` | `git tpl --json questions <template>` | `questions[]{name,default,defaultIsExpression,when,choices,defaultWhenSkipped}` |
| `init` | `git tpl init <template> [<dir>] [--init] [--answer k=v]... [--answers-from f]... [--defaults] [--dry-run]` | `id`,`ref`,`revision`,`commit`,`changes[]`,`merge{result,...}` |
| `status` | `git tpl status` | `templateMoved`,`merged`,`availableReferenceDescription`,`availableCommit`,`renderedReference`,`worktreeClean`,`remote{ahead,behind}` |
| `update` | `git tpl update [--answer k=v]... [--defaults] [--dry-run] [--push]` | `result`(`upToDate`\|`updated`\|`wouldUpdate`),`previousRevision`,`revision`,`changes[]`,`migrations[]`,`movedCommit` |
| `diff` | `git tpl diff [--stat] [--name-only] [--exit-code] [-- <path>...]` | `conflicts[]`,`changes[]{path,kind,insertions,deletions}` |
| `merge` | `git tpl merge [--no-commit] [-m <msg>]` / `git tpl merge --abort` | `result`(`upToDate`\|`fastForward`\|`merged`\|`staged`\|`conflicted`\|`aborted`),`commit`,`conflicts[]` |
| `show` | `git tpl show <path>` | no envelope — stdout **is** the file's bytes |
| `backport` | `git tpl backport [<pathspec>...] [--exclude g]... [-o file] [--unsubstitute] [--list-hunks \| --hunk <path>:<id>...]` | `result`(`patched`\|`nothingToBackport`\|`plan`),`template`,`patch`,`files[]`,`skipped[]`,`unsubstituted[]`,`plan[]{rendered,source,added,hunks[]{id,spec,header,insertions,deletions,lines}}` |
| `fetch` | `git tpl fetch [--remote name] [--dry-run]` | `state`(`absent`\|`synced`\|`diverged`\|`behind`\|`ahead`),`relation{ahead,behind}` |
| `push` | `git tpl push [--remote name] [--dry-run]` | `remote`,`ref` |
| `test` | `git tpl test [CASE...] [--ref R] [--shard I/T] [--record-durations] [--write] [--skip-commands]` | `summary{total,passed,failed,durationsRecorded,...}`,`shard{index,total,casesTotal,balanced}`,`cases[]{name,path,passed,durationMs,failures[]}` |

`revision`/`previousRevision` above are always the object `{reference, commit, dirty}` —
`reference`/`commit` independently optional, never a formatted string.

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
  Cloning the template, applying the patch, pushing, and opening pull requests are steps *you* take,
  with the user's confirmation — through `git` and `gh`, never through git-tpl.
- Grouping hunks into atomic changes is your judgement, not something git-tpl checks.
  Show the user each change's title, files, and size so they can catch a bad grouping before it becomes a pull request.
- Hunks are the *project's* edits against the rendering (what `git add -p` would show), not hunks of the template's
  source. A hunk's id stops matching as soon as its file changes.
