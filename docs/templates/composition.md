# Composing module templates

All final child output paths obey the same safety checks as parent output.
In particular, `.git` components are rejected in any letter case, including in composition paths.

`[[compose]]` renders sibling template repositories alongside a parent.
It is different from `[extends]`: a child provides its own files and questions, rather than overriding the parent.

Each child instance's actually loaded data sources are recorded separately from its revision lock:

```text
Template-Compose-Data: {"from":"sources","module":"jira","name":"catalog","location":"template:data.toml@8b3e7d1"}
```

The declaration, module and source name together identify the independent data namespace.
`location` includes the source's resolved revision or checksum using the ordinary data provenance format.
Historical commits without these trailers have no recorded child data evidence.
Enriching that evidence alone does not create a new rendered commit during an otherwise unchanged update.

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
# Optional; defaults to "modules" so existing projects keep their paths.
path = "modules"
```

An empty answer produces no modules and never fetches the child.
`modules = ["jira", "confluence"]` renders the child repository twice, with separate `module_name` answers
and `module_path` answers `modules/jira` and `modules/confluence`.
`path` names the directory beneath which each selected ID is rendered. A plugin with separate typed roots can use
`path = "sources"` for its `sources` list and `path = "integrations"` for its `integrations` list;
those render as `sources/pi` and `integrations/pi`, even if the IDs match.
It is a literal repository-relative directory (nested safe names are allowed), not an expression;
absolute paths, empty segments, `.`/`..`, `.git`, separators other than `/`, and control characters are rejected.
Without it, `module_path` remains `modules/<id>` for backwards compatibility.
The child must declare those two string questions.
If it also declares `version` or `memcastle`, an answer of the same name in the parent is forwarded.
All other child questions use their own defaults and do not pollute the parent's recorded answer namespace.

The source may be a local Git repository or a URL.
An expression can choose it from a parent question, which lets template tests use a local sibling repository
while normal projects use a published template URL.
The `ref` may be a branch, tag or commit.
Each child source requires its own network trust decision; trusting the parent does not trust the children.
Persistent parent trust covers only the parent's data requests.
Children and their ancestors need their own matching trust entries or explicit invocation-wide `--trust`.
IDs must be unique within each list, but separate lists may use the same ID.
For example, a source and an integration named `pi` may both write different files under `modules/pi/`.
If they write the same file path, composition refuses the collision rather than choosing a winner.
Repeated IDs within a list, unsafe IDs, missing answers, nested composition and output path collisions
are errors before the rendered tree or the destination directory is written.

Iterate the same list in a parent file:

```jinja
{% for id in modules %}
[[modules]]
id = "{{ id }}"
{% endfor %}
```

The parent manifest names each child ref; `.config/git.tpl.toml` records its resolved commit on `init` and `update`.
The rendered ref also records child commits so historical backport renders the same bytes after a branch moves.
New render commits additionally record each child's complete inherited source chain.
Backport routes a composed file to its actual child or ancestor owner, using that module's original answers and partials.
Mixed owners require separate mailboxes through `--output-dir` or `--json`;
see [backport](../usage/backport.md#several-owning-repositories) for the review and application workflow.
Project-free `render` reports those commits without creating a project config.
To add a module later, edit the `modules` answer in `.config/git.tpl.toml`, run `git tpl update --defaults`,
review `git tpl diff`, and run `git tpl merge`.
When a child branch advances without changing rendered files, `update` still records its commit and appends a
provenance-only rendered-ref commit; removing all modules drops the child's recorded commit without fetching it.
`update` still only moves `refs/tpl/*`; Git performs the merge into the worktree.
For a project with a manually overlaid integration, add its ID to the `integrations` answer and update as usual.
Git preserves project-specific files when the new child adds different paths; if both sides add the same path,
resolve the ordinary merge conflict before committing.

## One-shot module files

A child can initialize a user-owned placeholder using its own module context:

```toml
[files]
"{{ module_path }}/logo.svg" = { once = true }
```

Each selected module captures its own final path and initial Git entry in the parent's rendered history.
Later child changes, source or declaration removal, module deselection, and compose-declaration removal retain that entry.
Ordinary module files still disappear when deselected.
A retained entry can therefore outlive the module that originally supplied it.
Parent declarations may also capture exact final child output paths.
See the [`[files]` contract](format.md#files).
