# ADR-018: A prompt seed may be derived from the repository, through a closed context

**Status:** accepted

**Relates to:** [ADR-006](006-no-runtime-context.md),
[ADR-003](003-minijinja-only.md), [ADR-013](013-user-configuration.md)

## Context

Nearly every template asks, in its first three questions, for something the
project already knows. A slug. A repository name. An owner. The person applying
the template types `git-tpl` into a prompt while standing in a directory called
`git-tpl`, having just cloned it from a URL ending in `git-tpl.git`.

[ADR-006](006-no-runtime-context.md) already sanctioned one escape hatch for
this class of value: `default_from = "git:user.name"`, which pre-fills a prompt
from the Git configuration and reaches nothing else. The hatch was scoped to a
single Git configuration key, because that was the only case anyone had.

The directory name and the remote URL are the same kind of fact — machine-
varying, useful only as a guess, worthless in the render context — but neither
is a configuration key, and the useful part of a remote URL (`git-tpl` out of
`git@github.com:me/git-tpl.git`) is not a value that exists anywhere to be read.
It has to be derived.

Three shapes were available:

1. **More prefixes.** `default_from = "dir"`, `"remote:name"`, `"remote:owner"`.
   Simple, but fixed: there is no way to say "the remote name, or the directory
   name if there is no remote", which is the case that actually occurs, and no
   way to slugify the result. Every combination anyone wanted would become
   another prefix or another key.
2. **A source plus a filter key.** `default_from = "remote:name"` with
   `default_filter = "{{ seed | slugify }}"`. Two keys, and still no fallback.
3. **An expression over a small named context.** One key, and both fallback and
   transformation are ordinary MiniJinja — `| default(...)` and `| slugify`,
   which template authors already know from every other field.

## Decision

`default_from` accepts either the `git:<key>` shorthand, unchanged, or an
expression:

```toml
[questions.project_slug]
type = "string"
default_from = "{{ remote.name | default(dir.name) | slugify }}"
default = "my-project"
```

The expression is evaluated against a **seed context** with exactly three
namespaces:

| Namespace | Contents |
|---|---|
| `git` | the Git configuration, dotted keys nested — `git.user.name` |
| `dir` | `dir.name`, the working directory's own name |
| `remote` | `url`, `host`, `owner`, `name`, `slug`, from the `tpl.remote` remote |

**The set is closed.** There is no `env`, no `now`, no `platform`, no `user`,
and there will not be. Every one of those is the runtime context ADR-006 refuses
and a per-namespace argument for adding one would be an argument for adding all
of them; the closed list is the only defensible boundary. Adding a namespace is
an ADR, not a patch.

Four properties make this an elaboration of ADR-006 rather than a hole in it:

- **The two guards are untouched.** The seed context is built only when a
  project repository is present *and* the run is interactive, and `DefaultsOnly`
  ignores a seed outright. A `--defaults` or CI run never reads the machine at
  all — which is why two developers with different remotes render byte-identical
  trees.
- **The seed context is a different type from the render context.** Not a
  namespace inside `Context` with a rule attached, but `seed::SeedContext`,
  which nothing downstream of the prompter can reach. The rule is enforced by
  the compiler and not by a comment.
- **The seed environment has no partials.** `{% import %}` would let machine
  values be laundered through arbitrary template code, which turns a narrow
  hatch into a general one. It is also chainable rather than lenient, so that a
  missing value is undefined and `| default(...)` fires — the property the whole
  design rests on.
- **A seed still becomes an answer.** The value reaches a tree only after a
  human has seen it at a prompt and accepted it, at which point it is recorded
  in `.config/git.tpl.toml` and shared with everyone who renders the project.

There is no `dir.path`. An absolute path is the value most likely to be pasted
into a rendered file, and a rendered file containing `/home/ada` differs on every
machine — invariant 2 is defended by not offering the footgun rather than by
hoping. It also puts the user's home directory on screen at a prompt, and has no
machine-independent type: `PathBuf` is not UTF-8 and separators differ by
platform. If a future need appears, the answer is another derived,
machine-independent field, never the path.

The remote described is the one `tpl.remote` names, defaulting to `origin`. The
`--remote` flag is deliberately *not* consulted: a flag about where template
refs are pushed must not silently change a prompt default.

Expressions are validated when the manifest is loaded — they must parse, and
every root they name must be one of the three namespaces. Without the second
check the most likely mistake, writing `{{ project_name }}` in a `default_from`,
would render to nothing under a chainable environment and produce an empty
prompt with no message at all.

`default_from` contributes no edge to the dependency graph, in either form. It
references the machine, not the context, so there is nothing for the graph to
order.

## Consequences

**Not a breaking change.** `git:<key>` takes a code path with no engine in it and
behaves exactly as before; the `string`-questions-only rule survives; precedence
is unchanged, so no recorded answer changes and no rendered tree moves. A
manifest using the new form fails on an older git-tpl with
`tpl::manifest::invalid_question`, which is the normal shape of adding a manifest
capability.

**Two slugifiers remain, and remain separate.** `eval::slugify` — the filter —
is what a seed expression pipes into. `refs::slugify` derives `refs/tpl/<id>` and
its output is frozen by invariant 3. Sharing them would mean an improvement to
the filter renamed every existing project's template ref.

**A remote URL is parsed by us.** `src/remote.rs` is a second URL splitter beside
`refs::normalise`, deliberately so: this one may be improved freely, because its
output only ever pre-fills a prompt a human then confirms. Credentials in a URL
are stripped, because a token echoed at a prompt would then be recorded as an
answer.

**One new pressure to watch.** The seed context makes it easy to answer "can a
template read X?" with "add a namespace". The answer stays no. The question a
proposal must survive is not "would this be useful" — every runtime value is
useful — but "can this reach a rendered file without a human accepting it
first". Nothing here can, and nothing added later may.
