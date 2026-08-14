# Configuration

There are two places configuration lives, and the split is deliberate.

```
.config/git.tpl.toml       →  shared project configuration, versioned in Git
.git/config, ~/.gitconfig  →  your local preferences
```

A freshly cloned repository is fully understandable from `.config/git.tpl.toml`
alone. Nothing in Git configuration is required for git-tpl to work.

## `.config/git.tpl.toml`

Versioned with the project. It contains **only** the template reference and the
answers used to render it.

```toml
[template]
source = "https://github.com/rawtools/rust-library-template"
ref = "main"

[answers]
project_name = "example"
license = "MIT"
ci = true
```

### `[template]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `source` | string | *required* | Any Git URL, or a local path. |
| `ref` | string | the remote's default branch | Branch, tag or commit SHA. |
| `id` | string | derived from `source` | The template identity. Determines the ref name. |
| `root` | string | from the manifest | Override the rendered subdirectory. |

**`ref`** takes anything Git does:

```toml
ref = "main"        # a branch — moves as the template moves
ref = "v1.4.0"      # a tag — pinned until you change it
ref = "8b3e7d1"     # a commit — fully pinned
```

A branch is the usual choice. `git tpl update` resolves it fresh each time, which
is the point: `update` is how you find out the template moved.

**`id`** is normally derived and normally left out. It determines the ref name,
`refs/tpl/<id>`:

| `source` | derived `id` |
|---|---|
| `https://github.com/rawtools/rust-library` | `github-com-rawtools-rust-library` |
| `git@github.com:rawtools/rust-library.git` | `github-com-rawtools-rust-library` |
| `../rust-library-template` | `rust-library-template` |

The SSH and HTTPS forms of the same repository derive the *same* id, so switching
between them does not orphan the ref.

Set it explicitly when the template moves address but is conceptually the same
template — see
[Pointing at a different template](templates/local-development.md#pointing-an-existing-project-at-a-different-template).

### `[answers]`

Every answered question, with its type preserved. Written by `init`, updated by
`update` when a template adds a question.

Editing this file by hand is supported and expected — change a value, run
`git tpl update`, and the ref advances with a new rendering. That is how you
change your mind about a choice you made at `init` time.

Questions skipped by a `when` condition have no entry.

### What does *not* go here

No sync state. No record of what has been merged. No local preferences. No
credentials. No paths that only exist on your machine.

The rendered ref is the state. Duplicating it into a file would create two
sources of truth that can disagree, and the file would be the one that is wrong.

## Git configuration

Local, per-user, per-repository behaviour. All keys are under `tpl.`.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `tpl.remote` | string | `origin` | Remote used by `git tpl fetch` and `git tpl push`. |
| `tpl.autoPush` | bool | `false` | Push the rendered ref after a successful `update`. |
| `tpl.interactive` | bool | `true` | Prompt for unanswered questions. `false` behaves as `--defaults`. |

```sh
git config tpl.remote upstream
git config --global tpl.interactive false
```

### Precedence

Highest wins:

```
1.  command-line flags        --remote upstream
2.  repository config          .git/config
3.  user config                ~/.gitconfig
4.  system config              /etc/gitconfig
5.  built-in default
```

This is Git's own precedence, unchanged, because git-tpl reads these keys through
libgit2's configuration snapshot rather than parsing files itself. `git config
tpl.remote` tells you what git-tpl will use.

### Why these are not in `.config/git.tpl.toml`

They are preferences, not project identity. `tpl.autoPush` is a statement about
how *you* work; committing it would impose it on every contributor. Conversely,
the template source is a property of the project, and putting it in
`.git/config` would mean a fresh clone had no idea where the project came from.

The test for which file a setting belongs in: **would a new contributor cloning
this repository need it to be true?** If yes, `.config/git.tpl.toml`. If it is
about your machine or your habits, Git config.
