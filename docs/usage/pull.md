# `git tpl pull`

Re-render the attached template, then merge its rendered ref into your current branch.
Invoking `pull` explicitly requests both steps:

```sh
git tpl pull --defaults
git tpl pull --ref v2 --answer project_name=my-project
git tpl pull --no-commit
```

For an inspect-first workflow, use [`update`](update.md), [`diff`](diff.md), then [`merge`](merge.md).
`pull` has no `--dry-run`; use `update --dry-run` to preview rendering without publication.

## Phase ordering

1. Verify that the local `refs/tpl/<id>` exists.
2. Run update with the supplied answers and options.
3. If explicitly enabled by `--push` or `tpl.autoPush`, push a newly created rendering.
4. Merge the current local rendered-ref tip, even if update created no new commit.

An up-to-date rendering can still be waiting to be merged from an earlier update.
If both the rendering and its integration are up to date, no commit is made.
Rendered history remains append-only, including migration and provenance-only commits.

The steps are not transactional.
An update failure prevents integration; a push failure also stops before integration.
A failed merge leaves the successfully updated rendered ref and any saved configuration edits in place.
Inspect `git status` and `git tpl diff`, then retry integration with `git tpl merge` when ready.
Retry a failed publication with `git tpl push`; an unchanged update does not retry pushing.

`--push` publishes the rendered ref, not your project branch.
`pull` does not implicitly fetch shared rendered refs; fetching those remains an explicit `git tpl fetch` operation.
Template sources resolve and refresh as they do for update.
Global `--offline` with push enabled is refused before rendering publication.

## Configuration and local files

Changed answers or composed child locks are saved in `.config/git.tpl.toml` as an **unstaged edit**,
just as with update, even if the rendered output is unchanged.
They are not automatically included in the merge commit.
When answers and child locks have no semantic change, configuration bytes and formatting are preserved exactly.

Integration uses the same backend as `git tpl merge`, with its existing local-edit protections and limitations.
Configured external merge drivers are not executed;
use update followed by native `git merge refs/tpl/<id>` when those drivers are required.

Unlike update alone, pull refuses a missing local rendered ref with `tpl::ops::no_rendered_ref`.
Restore the original rendered history before retrying; see [recovering a missing ref](fetch.md).
Fetching a remote-tracking copy alone does not adopt it as the local ref.

## Conflicts and recovery

Conflicts leave ordinary Git conflict markers and merge state.
Resolve them with `git status`, your editor or `git mergetool`, then `git add` and `git commit`.
Abort with `git merge --abort` or `git tpl merge --abort`.
Aborting integration does not roll back the completed update or any push.

As with merge, a conflict is a reported outcome with exit status 0.
JSON callers must inspect `merge.result` and `merge.conflicts`.

## Options

| Option | Meaning |
|---|---|
| `--ref <REF>` | Render this source revision for this invocation; does not save the reference. |
| `--answer <KEY=VALUE>` | Supply an answer; repeatable. |
| `--answers-from <PATH>` | Load answers from TOML, JSON or YAML; repeatable. |
| `--defaults` | Accept defaults without prompting. |
| `--strict-answers` | Reject supplied answers that name no question. |
| `--dirty` | Render the template's working tree; conflicts with `--ref`. |
| `--trust` | Allow remote data access without confirmation for this invocation. |
| `--push` | Push if update creates a rendered commit. |
| `--remote <NAME>` | Override the remote used for that push. |
| `--no-commit` | Leave a non-fast-forward merge staged; fast-forwards still advance `HEAD`. |
| `-m`, `--message <MSG>` | Override the merge commit message. |

Global flags, including `--json`, `--offline` and `--cache-dir`, work as usual.
`--json` alone does not disable prompting; supply answers and/or `--defaults` for unattended runs.

## Machine-readable output

One JSON envelope contains separate `update` and `merge` objects using their existing result tags.
See [JSON output](../reference/json.md#pull) for the schema and failure behavior.
