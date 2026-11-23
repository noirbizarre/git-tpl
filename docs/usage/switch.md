# `git tpl switch`

Change the template attached to an initialized project while reusing compatible answers:

```sh
git tpl switch https://github.com/acme/service.tpl --defaults
git tpl diff
git tpl merge
```

The required template argument accepts a Git URL, local path, or user-configured shortcut.
Local paths are relative to the invocation directory and are stored relative to the project root.
Use `--ref <REF>` to persistently select a branch, tag or commit for subsequent updates.
Without it, the target's default reference is selected.

## Configuration and answers

When the target inherits directly or transitively from the current source, switch starts from the existing
`.config/git.tpl.toml`, changes its source/ref, and preserves other configuration values and recorded answers.
New answers replace their old values; inactive or obsolete recorded keys remain available.

For an unrelated template, switch builds fresh configuration with the target's root and resolved answers.
Old root overrides, obsolete answers and composed-child locks are discarded.
Both paths render the target's currently selected composed children and record their new locks.

Inheritance compares canonical local paths or identical remote source spellings against the target's ancestor chain.
Different SSH/HTTPS spellings are conservatively treated as unrelated sources.
The configured source's revision need not equal the ancestor's pinned revision.

In both cases, old answers are candidates matched by **exact question name**.
They are reused automatically only when their typed values satisfy the target's type, choices, selection bounds,
patterns and list rules.
Dynamic choices and conditions are resolved before reuse is decided.
Different names are never inferred from prompt text, position or similar values.
Incompatible candidates use the normal prompt/default flow, as do newly unanswered questions.

Explicit `--answer` or `--answers-from` input takes precedence and must pass validation; invalid input is an error.
`--defaults` accepts target defaults instead of prompting, and fails if a remaining question has no default.
`--strict-answers` checks explicit inputs, not obsolete recorded keys.
The command reports reused and skipped keys without exposing answer values.

## History and working files

Initialized [one-shot paths](../templates/format.md#files) remain captured on the same rendered ref,
even when the replacement source has no corresponding file or declaration.

Switch keeps the existing `refs/tpl/<id>` and explicitly records that ID in configuration.
The target rendering descends from the previous rendering even when the sources are unrelated.
This preserves Git's merge base, so a later ordinary merge can distinguish project customizations from old output
and apply template file removals.
A changed attachment records new provenance even if the rendered files are identical.

By default, switch leaves `HEAD`, the index and rendered worktree files untouched.
Configuration changes are saved **unstaged**; commit them with your normal project changes.
Saving changed configuration can normalize TOML formatting and comments.
Inspect with [`diff`](diff.md), then apply with [`merge`](merge.md) or native `git merge refs/tpl/<id>`.

The existing local rendered ref is required.
If it is missing, `tpl::ops::no_rendered_ref` points to recovering the original history;
see [restoring a missing ref](fetch.md#restoring-an-absent-local-ref).
Switch never starts an orphan or implicitly fetches shared rendered refs.

Cross-source migrations are skipped: the target rendering establishes the baseline for its subsequent updates.
Switch does not display init-only notes or add declared remotes.
Trust confirmation, offline policy and source caching work as with other renders.

## Opt-in merging

```sh
git tpl switch ./service-template --defaults --merge
git tpl update --defaults --merge
```

`--merge` requires an existing rendered ref, no active Git operation, and a clean index and worktree before rendering.
It runs ordinary integration after rendering, including when there is already pending unchanged output.
Integration uses [native Git](merge.md#it-is-a-normal-merge), including configured merge drivers, hooks and signing.
The configuration edit created by rendering remains unstaged and is not included in the merge commit.
For custom merge messages or staged-only integration, render first and use `git tpl merge` separately.

These are two phases: a merge failure leaves the completed rendering and configuration change in place.
Conflicts are reported with normal Git merge state; resolve and commit, or run `git tpl merge --abort`.
Aborting integration does not undo the switch.

## Options and JSON

| Option | Meaning |
|---|---|
| `--ref <REF>` | Persist the target branch, tag or commit. |
| `--answer <KEY=VALUE>` | Supply an explicit answer; repeatable. |
| `--answers-from <PATH>` | Read TOML, JSON or YAML answers; repeatable. |
| `--defaults` | Accept defaults without prompting. |
| `--strict-answers` | Reject unknown explicitly supplied question names. |
| `--dirty` | Render the local template worktree; conflicts with `--ref`. |
| `--trust` | Allow remote data access without confirmation for this invocation. |
| `--merge` | Integrate the rendered ref after switching. |

Global flags such as `--json`, `--offline` and `--cache-dir` work as usual.
Switch does not automatically publish the rendered ref; use `git tpl push` explicitly.
See [JSON output](../reference/json.md#switch) for the success payload and combined merge envelope.
