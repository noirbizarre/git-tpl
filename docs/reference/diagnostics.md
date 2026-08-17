# Diagnostic codes

Every failure carries a code of the form `tpl::<area>::<kind>`. Under
[`--json`](json.md) it is the `error.code` field; in text output it is the first
line of the diagnostic.

**The codes are the stable surface.** Messages are not, and are expected to
improve. A caller that matches on prose will break the next time one does;
a caller that matches on a code will not. Removing or renaming a code is a
breaking change, and a test pins the set so it cannot happen by accident.

## Reading a failure

```console
$ git tpl --json render ./template --output ./out --defaults
{"ok":false,"error":{
  "code":"tpl::render::content",
  "message":"failed to render `Cargo.toml.jinja`",
  "causes":[{"code":"tpl::eval::expression","message":"undefined value","help":"..."}]}}
```

`causes` is where the actionable detail lives. The outer error names the file;
the one beneath it names the expression and the reason. Branch on the outer
code to decide *what kind* of failure it is, and read the innermost one to say
*why*.

## Templates

Something is wrong with the template itself.

| Code | Meaning |
|---|---|
| `tpl::manifest::missing` | No `template.toml` at the source. |
| `tpl::manifest::parse` | `template.toml` is not valid TOML, or a key it declares has the wrong type. An unknown key is ignored, not diagnosed, so a misspelled one reads as unset. |
| `tpl::manifest::name_collision` | A question and a computed value share a name. |
| `tpl::manifest::invalid_question` | A question declaration is not coherent — see the message. Covers a `default_from` naming no source, one whose expression does not parse, and one referencing something that is not a [seed namespace](../templates/questions.md#machine-seeded-defaults). |
| `tpl::manifest::conflicting_note` | Both `note` and `note_file` are declared. Keep one. |
| `tpl::manifest::invalid_remote` | A `[remotes]` entry has no name, no URL, or a name Git will not accept. |
| `tpl::graph::invalid_expression` | An expression in the manifest does not parse. |
| `tpl::graph::unknown_reference` | An expression names something the template never declares. Carries a suggestion. |
| `tpl::graph::cycle` | Questions, computed values or data sources depend on each other in a loop. |
| `tpl::resolve::missing_root` | The render root does not exist in the template. |
| `tpl::resolve::dirty_needs_local` | `--dirty` was used on a remote template, which has no working tree. |
| `tpl::resolve::cache` | The temporary clone could not be created. |

## Rendering

| Code | Meaning |
|---|---|
| `tpl::render::content` | A file failed to render. The cause names the expression. |
| `tpl::render::path` | A path segment failed to render. |
| `tpl::render::escapes_tree` | A segment rendered to `.`, `..`, or something containing a separator. |
| `tpl::render::collision` | Two template files render to the same output path. |
| `tpl::render::partial_not_utf8` | A `.jinja` file outside the render root is not text. |

## Linting

Reported by [`git tpl lint`](../usage/lint.md) as findings rather than raised
as errors, so they arrive in the `diagnostics` array rather than in `error`.
Only `severity: "error"` fails the command, unless
[`--deny`](../usage/lint.md#choosing-what-fails) names a warning or the whole
severity.

| Code | Severity | Meaning |
|---|---|---|
| `tpl::lint::degenerate_path` | error | A conditional segment leaves a literal suffix outside the block, so it renders to something like `.yaml` instead of being skipped. |
| `tpl::lint::collision` | error | Two paths can collapse to the same name for some answer set. |
| `tpl::lint::syntax` | error | A `.jinja` file does not parse, including in branches no answer set reaches. |
| `tpl::lint::foreign_expression` | warning | A `${{ ... }}` MiniJinja will consume, rendering it to `$`. |
| `tpl::lint::undeclared` | warning | A file body uses a name the template does not declare. Renders empty unless `strict = true`. |
| `tpl::lint::missing_note_file` | error | `note_file` names a path the template repository does not contain. Reported without a repository, before an `init` refuses. |

These two are about the flags rather than the template, so they are raised as
errors before anything is checked:

| Code | Meaning |
|---|---|
| `tpl::lint::unknown_code` | A `--deny` or `--allow` names something that is neither `warnings` nor a code above. |
| `tpl::lint::conflicting_level` | The same code, or `warnings`, was both denied and allowed. |

## Answers and evaluation

| Code | Meaning |
|---|---|
| `tpl::answers::read` | An answers file could not be read. |
| `tpl::answers::parse` | An answers file is not valid TOML, JSON or YAML. |
| `tpl::answers::shape` | An answers file is not a table of values. |
| `tpl::answers::unknown_key` | A supplied answer names no question, under `--strict-answers`. Carries a suggestion. |
| `tpl::eval::expression` | An expression failed to evaluate. The location names it — `computed.<name>`, `questions.<name>.default`, or `questions.<name>.default_from`. |
| `tpl::eval::bad_choices` | `choices_from` did not resolve to an array. |
| `tpl::eval::wrong_type` | An answer is not of the declared type. |
| `tpl::eval::invalid_choice` | An answer is not one of the choices. |
| `tpl::eval::pattern_mismatch` | An answer does not match the question's `pattern`. |
| `tpl::eval::unanswered` | A question has no default and no answer was supplied. Usual cause of a failing `--defaults` run. |
| `tpl::eval::cancelled` | The user interrupted the questionnaire. |
| `tpl::value::type_mismatch` | A value is not the type it was used as. |
| `tpl::value::parse` | A value could not be parsed as its declared type. |

## Data sources

| Code | Meaning |
|---|---|
| `tpl::data::load` | A data source could not be read. |
| `tpl::data::parse` | A data source is not valid in its declared format. |
| `tpl::data::escapes_root` | A `local` path leaves the project root. |
| `tpl::data::needs_project` | A `local` source was reached by a command with no project — `render`, `lint`, `context`. Use a `template` source, or run inside a project. |
| `tpl::data::unknown_setting` | An unknown `kind` or `format`. |
| `tpl::data::invalid_git_source` | A `git` source's `source`, `ref` and `path` do not name a file. Write all three keys, or a `<scheme>://<repo>@<ref>:<path>` source. |
| `tpl::data::untrusted` | A network source — remote or `git` — was not authorised. Pass `--trust`, or add the template to `[trust]`. |
| `tpl::data::undeclared_remote` | A source that reached the network only after interpolation, so it could not be confirmed beforehand. Declare its kind. |
| `tpl::data::cancelled` | The user declined a fetch or a clone. |
| `tpl::data::checksum` | A remote source did not match its `sha256`. |

## Configuration

| Code | Meaning |
|---|---|
| `tpl::config::missing` | No `.config/git.tpl.toml`. The repository has no template attached. |
| `tpl::config::parse` | `.config/git.tpl.toml` is not valid TOML. |
| `tpl::config::io` | It could not be read or written. |
| `tpl::config::serialise` | It could not be written back. |
| `tpl::userconfig::io` | `~/.config/git-tpl/config.toml` could not be read. |
| `tpl::userconfig::parse` | It is not valid TOML, or has an unknown key. |
| `tpl::userconfig::shortcut` | A `[shortcuts]` name cannot be used. |
| `tpl::refs::underivable` | A template id could not be derived from the source. Pass `--id`. |
| `tpl::refs::invalid` | An explicit `--id` is not usable in a ref name. |

## Operations

| Code | Meaning |
|---|---|
| `tpl::ops::already_initialised` | A template is already attached. Use `update`, or `init --force` to re-ask. |
| `tpl::ops::invalid_argument` | An argument is not usable — see the message. |
| `tpl::ops::missing_note_file` | `note_file` names nothing at the template revision. The path is relative to the repository root, not the render root. |
| `tpl::ops::note_file_not_utf8` | `note_file` names a binary file. A note is text. |
| `tpl::ops::no_rendered_ref` | `refs/tpl/<id>` does not exist yet. Run `init` or `update`. |
| `tpl::ops::no_such_path` | The path is not in the rendering. |
| `tpl::ops::write_failed` | Generated output could not be written — see the reason. |

## Backport

Raised by [`git tpl backport`](../usage/backport.md). Every one of these is a
refusal, never a wrong patch, and every one names editing the template by hand
as the fallback — which is the status quo, so a refusal never leaves you worse
off than not having run the command. The reasoning is
[ADR-020](../adr/020-backport-is-a-patch.md).

| Code | Meaning |
|---|---|
| `tpl::backport::substituted_region` | A change lands on a line the template renders rather than copies, so there is no one-to-one change to send upstream. The expected refusal. |
| `tpl::backport::round_trip` | The patched template source did not render back to your file, so sending it would change what the template produces for everyone. |
| `tpl::backport::binary` | A changed file is binary, and a text patch cannot carry it. |
| `tpl::backport::stale_rendering` | The recorded answers no longer reproduce `refs/tpl/<id>`, so every line of the patch would be measured against the wrong file. Run `update` first. |
| `tpl::backport::unknown_path` | A named path is neither produced by the template nor present in the project. |
| `tpl::backport::output_write` | The patch could not be written to `--output`. |

## Template tests

Raised by [`git tpl test`](../usage/test.md) when the *run* cannot proceed. A
case that simply fails its expectations is not one of these: it arrives in the
report's `cases[].failures` array, and the command exits `1`.

The area is `testing` rather than `test`, which is reserved for the diagnostic
fixtures in `src/report.rs`.

| Code | Meaning |
|---|---|
| `tpl::testing::no_tests` | The tests directory does not exist at the resolved revision, or holds no case files. |
| `tpl::testing::no_such_case` | A named case does not exist. Carries a suggestion and the available names. |
| `tpl::testing::case_parse` | A case file is not valid TOML, JSON or YAML. |
| `tpl::testing::case_shape` | A case file parses but is not a coherent case — an unknown key, a wrong type, contradictory expectations, or two files claiming one case name. |
| `tpl::testing::write_needs_local` | `--write` was used on a template with no working tree to write a snapshot into. |
| `tpl::testing::snapshot_read` | A recorded snapshot is unreadable, or its `MANIFEST` contradicts the files beside it. |
| `tpl::testing::snapshot_write` | A snapshot could not be written to the working tree. |

## Git

| Code | Meaning |
|---|---|
| `tpl::git::not_a_repository` | The command needs a repository and there is none. |
| `tpl::git::no_such_revision` | The requested branch, tag or SHA does not exist. |
| `tpl::git::auth` | Authentication failed. |
| `tpl::git::network` | The remote could not be reached. The help text carries libgit2's own reason. |
| `tpl::git::clone` | The remote answered, but the clone could not be written — usually no space or no permission on the temporary directory. |
| `tpl::git::remote_exists` | A remote of that name already exists. git-tpl never repoints one. |
| `tpl::git::dirty_worktree` | The operation merges, and the worktree has uncommitted changes. |
| `tpl::git::diverged` | The remote template ref has diverged. Nothing is force-pushed. |
| `tpl::git::no_identity` | Git has no `user.name` or `user.email` configured. |
| `tpl::git::backend` | Anything else libgit2 reported. |
