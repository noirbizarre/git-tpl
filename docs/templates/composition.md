# Composing module templates

`[[compose]]` renders sibling template repositories alongside a parent.
It is different from `[extends]`: a child provides its own files and questions, rather than overriding the parent.

```toml
[questions.modules]
type = "list"
default = []
item_pattern = "^[a-z0-9]+(-[a-z0-9]+)*$"

[questions.module_template]
type = "string"
default = "https://github.com/org/module-template"

[[compose]]
source = "{{ module_template }}"
ref = "main"
from = "modules"
```

An empty answer produces no modules and never fetches the child.
`modules = ["jira", "confluence"]` renders the child repository twice, with separate `module_name` answers
and `module_path` answers `modules/jira` and `modules/confluence`.
The child must declare those two string questions.
If it also declares `version` or `memcastle`, an answer of the same name in the parent is forwarded.
All other child questions use their own defaults and do not pollute the parent's recorded answer namespace.

The source may be a local Git repository or a URL.
An expression can choose it from a parent question, which lets template tests use a local sibling repository
while normal projects use a published template URL.
The `ref` may be a branch, tag or commit.
Each child source requires its own network trust decision; trusting the parent does not trust the children.
IDs must be unique within each list, but separate lists may use the same ID.
For example, a source and an integration named `pi` may both write different files under `modules/pi/`.
If they write the same file path, composition refuses the collision rather than choosing a winner.
Repeated IDs within a list, unsafe IDs, missing answers, nested composition and output path collisions
are errors before the rendered tree or the destination directory is written.

Iterate the same list in a parent file:

```jinja
{% raw %}{% for id in modules %}
[[modules]]
id = "{{ id }}"
{% endfor %}{% endraw %}
```

The parent manifest names each child ref; `.config/git.tpl.toml` records its resolved commit on `init` and `update`.
The rendered ref also records child commits so historical backport renders the same bytes after a branch moves.
Project-free `render` reports those commits without creating a project config.
To add a module later, edit the `modules` answer in `.config/git.tpl.toml`, run `git tpl update --defaults`,
review `git tpl diff`, and run `git tpl merge`.
When a child branch advances without changing rendered files, `update` still records its commit and appends a
provenance-only rendered-ref commit; removing all modules drops the child's recorded commit without fetching it.
`update` still only moves `refs/tpl/*`; Git performs the merge into the worktree.
For a project with a manually overlaid integration, add its ID to the `integrations` answer and update as usual.
Git preserves project-specific files when the new child adds different paths; if both sides add the same path,
resolve the ordinary merge conflict before committing.
