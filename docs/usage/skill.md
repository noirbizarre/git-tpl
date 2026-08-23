# AI agent skill

git-tpl ships an [agent skill](https://github.com/noirbizarre/git-tpl/blob/main/skills/git-tpl/SKILL.md) —
a `SKILL.md` file that teaches an AI coding agent (Claude, opencode, or any tool honouring the same convention)
how to drive `git tpl` for the whole consumer lifecycle:
bootstrapping a new project from a template,
adopting a template into an existing project,
checking for and merging updates,
and backporting a local fix upstream.

It lives at `skills/git-tpl/SKILL.md` in this repository,
and is written for an agent working in *another* project —
one that uses, or wants to use, a git-tpl template.
It is not needed to work on git-tpl itself.

## Installing it

There is no install command:
an agent skill is just a file the agent's tool discovers at a known path.
Fetch it once into one of those paths, project-local or global.

=== "opencode, project-local"

    ```sh
    mkdir -p .opencode/skills/git-tpl
    curl -fsSL https://raw.githubusercontent.com/noirbizarre/git-tpl/0.7.0/skills/git-tpl/SKILL.md \
      -o .opencode/skills/git-tpl/SKILL.md
    ```

=== "opencode, global"

    ```sh
    mkdir -p ~/.config/opencode/skills/git-tpl
    curl -fsSL https://raw.githubusercontent.com/noirbizarre/git-tpl/0.7.0/skills/git-tpl/SKILL.md \
      -o ~/.config/opencode/skills/git-tpl/SKILL.md
    ```

=== "Claude Code, project-local"

    ```sh
    mkdir -p .claude/skills/git-tpl
    curl -fsSL https://raw.githubusercontent.com/noirbizarre/git-tpl/0.7.0/skills/git-tpl/SKILL.md \
      -o .claude/skills/git-tpl/SKILL.md
    ```

=== "Claude Code, global"

    ```sh
    mkdir -p ~/.claude/skills/git-tpl
    curl -fsSL https://raw.githubusercontent.com/noirbizarre/git-tpl/0.7.0/skills/git-tpl/SKILL.md \
      -o ~/.claude/skills/git-tpl/SKILL.md
    ```

=== "generic (.agents), project-local"

    ```sh
    mkdir -p .agents/skills/git-tpl
    curl -fsSL https://raw.githubusercontent.com/noirbizarre/git-tpl/0.7.0/skills/git-tpl/SKILL.md \
      -o .agents/skills/git-tpl/SKILL.md
    ```

=== "generic (.agents), global"

    ```sh
    mkdir -p ~/.agents/skills/git-tpl
    curl -fsSL https://raw.githubusercontent.com/noirbizarre/git-tpl/0.7.0/skills/git-tpl/SKILL.md \
      -o ~/.agents/skills/git-tpl/SKILL.md
    ```

Pin the ref (here `0.7.0`) to a released tag rather than `main`,
the same reason you would pin any dependency —
a moving target is a surprise later, not a convenience now.

A global install makes the skill available whenever *any* project turns out to use git-tpl,
without touching that project's own files.

## Content

The skill is self-contained —
it does not assume access to this repository's own docs, since it is read from inside someone else's project.
It links out to the hosted reference pages
([JSON output](../reference/json.md), [Diagnostic codes](../reference/diagnostics.md))
for depth instead of duplicating them.
