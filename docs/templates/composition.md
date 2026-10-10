# Composing module templates

`[[compose]]` renders sibling template repositories alongside a parent.
It is different from `[extends]`: a child provides its own files and questions, rather than overriding the parent.

```toml
[questions.modules]
type = "string"
default = ""

[questions.module_template]
type = "string"
default = "https://github.com/org/module-template"

[[compose]]
source = "{{ module_template }}"
rev = "a4d771783cd2b6e86576ee0bb6011ce292ac8ff1"
from = "modules"
```

An empty answer produces no modules and never fetches the child.
`modules = "jira,confluence"` renders the pinned child repository twice, with separate `module_name` answers
and `module_path` answers `modules/jira` and `modules/confluence`.
The child must declare those two string questions.
If it also declares `version` or `memcastle`, an answer of the same name in the parent is forwarded.
All other child questions use their own defaults and do not pollute the parent's recorded answer namespace.

The source may be a local Git repository or a URL.
An expression can choose it from a parent question, which lets template tests use a local sibling repository
while normal projects use a published template URL.
The `rev` must be a tag or commit, never a branch.
Each child source requires its own network trust decision; trusting the parent does not trust the children.
Unpinned revisions, repeated module IDs, unsafe IDs, missing answers, nested composition and output path collisions
are errors before the rendered tree or the destination directory is written.

Use the pure `split` filter to iterate the same list in a parent file:

```jinja
{% raw %}{% for id in modules | split(',') if id %}
[[modules]]
id = "{{ id }}"
{% endfor %}{% endraw %}
```

The parent manifest owns the pinned revisions.
Its own Git commit and the recorded answer digest reproduce the complete child selection.
To add a module later, edit the `modules` answer in `.config/git.tpl.toml`, run `git tpl update --defaults`,
review `git tpl diff`, and run `git tpl merge`.
`update` still only moves `refs/tpl/*`; Git performs the merge into the worktree.
