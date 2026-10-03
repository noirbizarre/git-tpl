# AI agent skill

git-tpl ships an [agent skill](https://github.com/noirbizarre/git-tpl/blob/main/skills/git-tpl/SKILL.md) — a
`SKILL.md` file that teaches an AI coding agent (Claude, opencode, or any tool honouring the same convention) how
to drive `git tpl` for the whole consumer lifecycle: bootstrapping a new project from a template, adopting a
template into an existing project, synchronizing it with its template — listing and merging updates, then
backporting the local changes you choose upstream — and resolving the conflicts that leave behind.

It lives at `skills/git-tpl/SKILL.md` in this repository, and is written for an agent working in *another*
project — one that uses, or wants to use, a git-tpl template.
It is not needed to work on git-tpl itself.

## Installing it

Two ways to put the skill in place, both global — available whenever *any* project turns out to use git-tpl,
without touching that project's own files.

### With `gh skill`

```sh
gh skill install noirbizarre/git-tpl git-tpl --agent universal --scope user
```

- `--scope user` installs it once, for every project, instead of only the current repository.
- `--agent universal` places it under the generic `.agents/skills` layout most tools already read, rather than
  duplicating it per tool.
- `gh skill list` shows what's installed; `gh skill update --all` refreshes it later.

`gh skill` is a preview feature of the GitHub CLI — confirm it exists with `gh skill --help` before relying on it
in a script.

### Manual, generic install

Without `gh`, or on a `gh` version that predates `gh skill`, fetch the file directly into the same generic
location:

```sh
mkdir -p ~/.agents/skills/git-tpl
curl -fsSL https://raw.githubusercontent.com/noirbizarre/git-tpl/main/skills/git-tpl/SKILL.md \
  -o ~/.agents/skills/git-tpl/SKILL.md
```

The skill file changes independently of git-tpl's own releases — re-run the command above to pick up updates.

## Content

The skill is self-contained — it does not assume access to this repository's own docs, since it is read from
inside someone else's project.
It links out to the hosted reference pages ([JSON output](../reference/json.md),
[Diagnostic codes](../reference/diagnostics.md)) for depth instead of duplicating them.

## Synchronizing

"Synchronize" is a workflow of the skill, not a `git tpl` command.
It runs two halves in order:

1. **Pull.**
   The agent updates the template ref, shows you the list of incoming changes — grouped by purpose, with migration
   notes and conflicts called out — and merges only once you agree.
2. **Push.**
   The agent lists your local changes as hunks ([`backport --list-hunks`](backport.md#naming-hunks-in-advance)),
   groups them into atomic changes, and asks which to send with a multiple-choice prompt.
   Each chosen change becomes its own patch.

When the template lives on GitHub, each chosen change then becomes its own branch and its own pull request,
opened with the [GitHub CLI](https://cli.github.com/) (`gh`) — which the skill needs only for this step.
For any other upstream the agent leaves a branch in a clone of the template and tells you where.

Nothing is pushed, and no pull request is opened, without your confirmation each time.
git-tpl itself still never writes to the template repository: the cloning, the `git am`, the push, and the pull
request are the agent's, through `git` and `gh`.
