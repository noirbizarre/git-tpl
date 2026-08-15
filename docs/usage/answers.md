# Answers from a file

`--answers-from <path>` supplies answers from a TOML, JSON or YAML file, on
both `git tpl init` and `git tpl update`.

```console
$ git tpl init https://github.com/noirbizarre/rust-library-template \
    --answers-from answers.toml
```

It exists because four unrelated things need the same thing:

- migrating from Copier, Cruft, or anything else that recorded its answers
- creating many projects from one set of house values
- rendering in CI, where there is no terminal to prompt at
- checking a template's fixtures in, so the template can have tests

A tool-specific importer would have bought only the first, and would have put
someone else's file format in the CLI surface permanently.

## The file

Either a flat table of `name = value`:

```toml
project_name = "my-thing"
license = "Apache-2.0"
with_ci = true
port = 8080
```

…or those same pairs under an `answers` key, so a template's fixture file can
carry other tables beside them:

```toml
[answers]
project_name = "my-thing"

[expect]
files = ["pyproject.toml"]
```

Nothing else is a valid shape. A document that is not a table is refused
(`tpl::answers::shape`) rather than silently supplying nothing.

## Formats

The same three the [data sources](../data/index.md#formats) take, read by the
same parsers, chosen by the file extension:

| Extension | Format |
|---|---|
| `.toml`, anything else | TOML |
| `.json` | JSON |
| `.yaml`, `.yml` | YAML 1.2 |

YAML matters here specifically: `.copier-answers.yml` and most hand-written
house-defaults files are YAML, and requiring a `yq` step first would have
undercut the point of the flag. It is YAML 1.2, so `no` is the string `"no"` —
see [About YAML](../data/index.md#about-yaml).

## Types are preserved

This is the difference between the file and `--answer`. A flag can only carry
text, so `--answer port=8080` is a string that the question's declared type
turns into an integer. A file carries the type it was written with.

A value that does not match the question's declared type is an **error**
(`tpl::eval::wrong_type`), never a silent coercion. `port = "eighty"` for an
integer question fails; it does not become `0`.

## Unknown keys are ignored, and reported

A key naming no question in the template is skipped, and named on stderr:

```console
warning: answers ignored: they name no question in this template
  _src_path
  _commit
```

Both halves are deliberate. Erroring would make the flag useless for the case
that motivated it — a `.copier-answers.yml` carries `_src_path` and `_commit`,
and any long-lived template has dropped a question at some point. Staying silent
would make a typo'd key look exactly like an answer that had no effect.

Nothing about the ignored keys is recorded: `.config/git.tpl.toml` holds the
answers to questions the template actually asked.

## A Copier answers file works unedited

```console
$ git tpl init https://github.com/example/rust-library \
    --answers-from .copier-answers.yml
```

This is not Copier compatibility, and is not meant to grow into it. The flag
maps names to names: a template whose questions were renamed between tools needs
its answers edited. Shipping a mapping language to avoid that would cost more
than the editing does.

## Precedence

`--answers-from` is repeatable, and later files win over earlier ones — house
defaults first, the specific file on top:

```console
$ git tpl init <template> \
    --answers-from house-defaults.toml \
    --answers-from this-project.toml \
    --answer project_name=thing
```

The whole chain, highest first:

```text
--answer
  >  the last --answers-from
  >  earlier --answers-from
  >  answers recorded in .config/git.tpl.toml   (update only)
  >  the question's default
```

A question covered by none of them is asked as usual, unless `--defaults` or
`tpl.interactive false` is in force — in which case its default is taken, and a
question with no default is an error (`tpl::eval::unanswered`).

## Failures

| Code | Meaning |
|---|---|
| `tpl::answers::read` | The file could not be read. The path is in the help. |
| `tpl::answers::parse` | It is not valid TOML, JSON or YAML. |
| `tpl::answers::shape` | It is not a table of answers. |
| `tpl::eval::wrong_type` | A value does not match the question's declared type. |

The path is resolved relative to your working directory, and is used as given.
It is deliberately not restricted to the project — you named it yourself, unlike
a [local data source](../data/local.md) path, which comes out of a template
repository and is therefore untrusted input.
