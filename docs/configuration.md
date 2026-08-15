# Configuration

There are three places configuration lives, and the split is deliberate. Each
has exactly one owner.

```
.config/git.tpl.toml           →  the project. Versioned in Git. Everyone gets it.
~/.config/git-tpl/config.toml  →  you. Never committed, never read by anyone else.
.git/config, ~/.gitconfig      →  tpl.* preferences
```

A freshly cloned repository is fully understandable from `.config/git.tpl.toml`
alone. Neither of the other two is required for git-tpl to work, and nothing in
either can change what a template renders.

The test for which file something belongs in: **would a new contributor cloning
this repository need it to be true?** If yes, `.config/git.tpl.toml`. If it is
about your machine or your habits, one of the other two — Git config if Git
already models it, the user configuration otherwise.

## `.config/git.tpl.toml`

Versioned with the project. It contains **only** the template reference and the
answers used to render it.

```toml
[template]
source = "https://github.com/noirbizarre/rust-library-template"
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
| `https://github.com/noirbizarre/rust-library-template` | `github-com-noirbizarre-rust-library-template` |
| `git@github.com:noirbizarre/rust-library-template.git` | `github-com-noirbizarre-rust-library-template` |
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

## `~/.config/git-tpl/config.toml`

Yours. Never committed, and never read by anyone else — so nothing in it may
change what a template renders. See
[ADR-013](adr/013-user-configuration.md).

The path follows XDG: `$XDG_CONFIG_HOME/git-tpl/config.toml`, falling back to
`~/.config/git-tpl/config.toml`. An absent file is the normal case, not an
error. Unknown keys *are* an error — nothing generates this file, so a key that
is not understood is a typo.

```toml
[defaults]
author = "Axel Haustant"
license = "MIT"

[shortcuts]
gh = "https://github.com/"
ghs = "ssh://git@github.com/"

[trust]
templates = ["github.com/noirbizarre/*"]
```

Three sections, and deliberately nothing else.

| Section | What it does |
|---|---|
| `[defaults]` | Pre-fills a prompt whose question has the same name. |
| `[shortcuts]` | Expands a leading `<name>:` in a template URL you type. |
| `[trust]` | Templates whose remote data is fetched without a confirmation. |

Note the name: the project file is `git.tpl.toml`, mirroring `git tpl`; this one
is named after the binary. Two shapes, so a stray copy of one is never mistaken
for the other.

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

One key outside `tpl.` is read: a question declaring
[`default_from = "git:<key>"`](templates/questions.md#git-seeded-defaults) has
that key read, read-only, to pre-fill its prompt. It is never read when nobody
is being asked, and its value reaches the project only as an answer you
accepted.

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

### Why these are not in the user configuration either

Git already models remotes, and it already has a precedence chain across
system, user and repository files that people know. Reimplementing either would
mean `git config tpl.remote` no longer told you the truth. The user
configuration holds the three things Git has no opinion about: what a prompt
should be pre-filled with, what a URL prefix abbreviates, and which templates
you have already agreed to let reach the network.
