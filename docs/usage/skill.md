# AI agent skill

git-tpl ships an [agent skill](https://github.com/noirbizarre/git-tpl/blob/main/skills/git-tpl/SKILL.md) — a
`SKILL.md` file that teaches an AI coding agent (Claude, opencode, or any tool honouring the same convention) how
to drive `git tpl` for the whole consumer lifecycle: bootstrapping a new project from a template, adopting a
template into an existing project, checking for and merging updates, and backporting a local fix upstream.

It lives at `skills/git-tpl/SKILL.md` in this repository, and is written for an agent working in *another*
project — one that uses, or wants to use, a git-tpl template.
It is not needed to work on git-tpl itself.

## Installing it

Two ways to put the skill in place, both global — available whenever *any* project turns out to use git-tpl,
without touching that project's own files.

### With `gh skill`

```sh
gh skill install noirbizarre/git-tpl git-tpl --agent universal --scope user --pin 0.7.0
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
curl -fsSL https://raw.githubusercontent.com/noirbizarre/git-tpl/0.7.0/skills/git-tpl/SKILL.md \
  -o ~/.agents/skills/git-tpl/SKILL.md
```

Pin the ref (here `0.7.0`) to a released tag rather than `main` — the same reason you would pin any dependency: a
moving target is a surprise later, not a convenience now.

## Content

The skill is self-contained — it does not assume access to this repository's own docs, since it is read from
inside someone else's project.
It links out to the hosted reference pages ([JSON output](../reference/json.md),
[Diagnostic codes](../reference/diagnostics.md)) for depth instead of duplicating them.
