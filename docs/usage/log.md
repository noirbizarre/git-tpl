# `git tpl log`

Read the template's source commits, formatted as a compact changelog.

```sh
git tpl log [REF] [-n COUNT | --all] [--no-relative-date]
```

Without a reference, show the latest **20 source commits up to the template version applied to your current branch**.
An update you rendered but have not merged does not advance that boundary.

With a reference, show commits reachable from that reference but absent from the applied version:

```sh
git tpl log main                  # applied..main in the template repository
git tpl log v2.0.0                # a template tag
git tpl log a17b0b2               # a template commit
git tpl log --max-count 50        # more applied history; -n 50 is equivalent
git tpl log main --all            # the complete selected range
```

`REF` is a single branch, tag or commit in the **template repository**, not your project.
No rendering or merge is needed to inspect an available version.

## Output

```console
$ git tpl log main

Template:  https://github.com/org/rust-template
Applied:   v1.0.0 (8b3e7d1)
Target:    main (4f2c1a9)

  4f2c1a9  feat(ci): add a release workflow  (2 days ago)
  a17b0b2  fix!: correct the package layout  (1 week ago)
  6e8c124  Update installation instructions  (2 weeks ago)
```

Conventional Commit subjects have their `type(scope)!:` prefix highlighted when color is enabled.
Unknown types and non-conventional subjects are retained in their original order; nothing is grouped or filtered.
The parser recognizes the subject prefix, not breaking-change footers in the commit body.
Hashes are abbreviated for display, and dates use the commit's **committer time**.
Descendants appear before ancestors, with commit time used to prioritize the walk.
Merge commits and their merged ancestry are included.

Human output goes to stderr and respects `--quiet` and `--color`.
Use [`--json`](../reference/json.md#log) for full commit IDs, subjects, parsed prefixes and absolute timestamps.
Success exits zero, including an empty range or `-n 0`.
`--all` and an explicit `-n`/`--max-count` are mutually exclusive.

### Dates

Relative ages are the default, including future ages such as `in 2 minutes` when source clocks are ahead.
To use absolute dates and times, to the minute:

```sh
git tpl log --no-relative-date
git config tpl.log.relativeDate false          # this project
git config --global tpl.log.relativeDate false # all projects
```

Absolute dates use UTC, for example `2026-10-29 14:35 UTC`.
The flag overrides the [Git configuration setting](../configuration.md#git-configuration).
An out-of-range calendar timestamp is displayed as Unix seconds instead.
Date display choices do not change the JSON payload.

## Which history is applied?

The applied rendering is the newest commit along `refs/tpl/<id>`'s first-parent history that is reachable from `HEAD`.
Its provenance identifies the original template repository and source commit.
The log follows that recorded source even if the project configuration has since been edited to name another source.
It does not use the newest unmerged rendering reported by [`status`](status.md).

Application is a Git ancestry relationship, not a comparison of file contents.
Squash merges and cherry-picks do not preserve it; reverting files does not remove it.
An attachment made with `init --no-merge` needs to be merged before it has an applied history.
If the rendered ref is absent, [restore the original rendered ref](fetch.md#restoring-an-absent-local-ref).
Fetching a shared ref alone does not restore the local ref.
Unusable provenance and dirty-worktree renderings are diagnosed rather than replaced with an older guessed baseline.
Commit the template changes, update, and merge a clean rendering to inspect its exact source history.

An explicit target uses Git's two-dot reachability semantics, `applied..target`.
An equal or older target normally has no commits to show.
A divergent target shows its target-only commits, with a message explaining that it does not descend from the applied version.
This is not a symmetric difference or a list of removals when downgrading.

## Source access and scope

A local source is opened in place; a remote source is cloned into a temporary directory.
Inspection can therefore require network access.
Missing historical source objects, including rewritten history or an incomplete local clone, produce an actionable error.

The project branch, index, worktree, configuration and refs are unchanged.
No manifest is loaded, so history can include commits before `template.toml` existed or while it was invalid.
No template commands or data sources are run.

This log covers the main template source repository.
It does not combine inherited/composed repositories or explain changes caused by answers or remote data.
For the actual effect on your project, render with [`update`](update.md) and inspect [`diff`](diff.md).
