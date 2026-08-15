# `git tpl questions`

List a template's questions and their schema, without asking any of them.

```sh
git tpl questions ./my-template
git tpl --json questions ./my-template
```

`init --dry-run` lists question *names*, on stderr, and needs a repository and
a network fetch to do it. That is enough to reassure a human and not enough to
write an answers file with — which is what anything driving git-tpl
non-interactively needs first.

## Generating an answers file

```sh
git tpl --json questions ./tpl \
  | jq -r '.questions[] | select(.default != null and .defaultIsExpression == false)
           | "\(.name) = \(.default | tojson)"' > answers.toml
```

## Three things that are not in the manifest

### Resolution order

Questions come out in the order they are asked, not the order they are
declared. When a `when` or a `default` references an earlier answer, that is
the order they must be answered in — and it is what the dependency graph
already computes for prompting.

### `defaultIsExpression`

A default may be an expression:

```toml
[questions.bin_name]
type = "string"
default = "{{ crate }}"
```

The schema reports the raw string *and* that it is derived. A caller that took
it literally would write `{{ crate }}` into the answers file.

### `choicesResolved`

When `choices_from` points at a data file inside the template repository, the
values are resolved and included, so a caller does not have to fetch and parse
the file itself.

Only for template-local sources. A remote one would mean this command silently
acquired a network fetch, which a command that reads a manifest should not do.

## See also

- [`git tpl context`](context.md) — the resolved values, rather than the
  declaration.
- [Answers from a file](answers.md) — precedence and file formats.
