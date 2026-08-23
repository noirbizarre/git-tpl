# Configuration

There are three places configuration lives, and the split is deliberate.
Each has exactly one owner.

```
.config/git.tpl.toml           →  the project. Versioned in Git. Everyone gets it.
~/.config/git-tpl/config.toml  →  you. Never committed, never read by anyone else.
.git/config, ~/.gitconfig      →  tpl.* preferences
```

A freshly cloned repository is fully understandable from `.config/git.tpl.toml` alone.
Neither of the other two is required for git-tpl to work, and nothing in either can change what a template
renders.

The test for which file something belongs in: **would a new contributor cloning this repository need it to be
true?**
If yes, `.config/git.tpl.toml`.
If it is about your machine or your habits, one of the other two — Git config if Git already models it, the user
configuration otherwise.

## `.config/git.tpl.toml`

Versioned with the project.
It contains **only** the template reference and the answers used to render it.

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

A branch is the usual choice.
`git tpl update` resolves it fresh each time, which is the point: `update` is how you find out the template moved.

**`id`** is normally derived and normally left out.
It determines the ref name, `refs/tpl/<id>`:

| `source` | derived `id` |
|---|---|
| `https://github.com/noirbizarre/rust-library-template` | `github-com-noirbizarre-rust-library-template` |
| `git@github.com:noirbizarre/rust-library-template.git` | `github-com-noirbizarre-rust-library-template` |
| `../rust-library-template` | `rust-library-template` |

The SSH and HTTPS forms of the same repository derive the *same* id, so switching between them does not orphan
the ref.

Set it explicitly when the template moves address but is conceptually the same template — see
[Pointing at a different template](templates/local-development.md#pointing-an-existing-project-at-a-different-template).

Changing `id` or `source` changes the ref name, so the next `git tpl update` finds no ref to advance and writes an
orphan commit instead: a new history, sharing no merge base with what your branch already merged.
That is usually what you want when you deliberately repoint a project, and it is never what you want when you
have simply not fetched the ref yet.
`update` says which one it thinks happened — see
[When there is no ref to advance](usage/update.md#when-there-is-no-ref-to-advance).

### `[answers]`

Every answered question, with its type preserved.
Written by `init`, updated by `update` when a template adds a question.

Editing this file by hand is supported and expected — change a value, run `git tpl update`, and the ref advances
with a new rendering.
That is how you change your mind about a choice you made at `init` time.

Questions skipped by a `when` condition have no entry.

### What does *not* go here

No sync state.
No record of what has been merged.
No local preferences.
No credentials.
No paths that only exist on your machine.

The rendered ref is the state.
Duplicating it into a file would create two sources of truth that can disagree, and the file would be the one
that is wrong.

## `~/.config/git-tpl/config.toml`

Yours.
Never committed, and never read by anyone else — so nothing in it may change what a template renders.
See [ADR-013](adr/013-user-configuration.md).

The path follows XDG: `$XDG_CONFIG_HOME/git-tpl/config.toml`, falling back to `~/.config/git-tpl/config.toml` —
see [the environment](#the-environment) for what counts as set.
An absent file is the normal case, not an error.
Unknown keys *are* an error — nothing generates this file, so a key that is not understood is a typo.

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
| `[trust]` | Templates whose network data is fetched or cloned without a confirmation. |

### `[defaults]`

A key matching a question's name pre-fills that question's prompt.
Press Enter and it becomes your answer, recorded in `.config/git.tpl.toml` like any other.

```toml
[defaults]
author = "Axel Haustant"
email = "axel@example.com"
license = "MIT"
```

**It seeds a prompt and nothing else.** If the question is not asked — `--defaults`, `tpl.interactive false`, CI —
this file is ignored entirely and the template's own `default` applies.
Anything else would mean the same template rendered two different trees on two machines, and then an unchanged
template would no longer produce no commit.
It is the same rule [`default_from`](templates/questions.md#machine-seeded-defaults) already follows.

Any question type may be seeded, not only `string`.

Precedence, highest first:

```
1.  --answer
2.  --answers-from                    (the last file given wins)
3.  answers in .config/git.tpl.toml
4.  [defaults]                        ← this file
5.  default_from
6.  the question's own default
```

`[defaults]` sits above `default_from` because the two are different kinds of statement: `default_from` is the
*template author* guessing where the answer usually comes from, and `[defaults]` is you saying it outright.

A key naming no question — or naming one of another type — is skipped in silence.
That is deliberate, and differs from `--answers-from`: you write this file once for every template you will ever
generate, so it is *expected* to overshoot, and warning about `author` on every template that has no `author`
question is how a warning stops being read.

### `[shortcuts]`

A prefix substitution on a leading `<name>:` in a template URL you type.

```toml
[shortcuts]
gh = "https://github.com/"
ghs = "ssh://git@github.com/"
mine = "https://github.com/noirbizarre/"
```

```sh
git tpl init gh:org/rust-library-template
git tpl init mine:rust-library-template
```

!!! warning "The expanded URL is what gets recorded"

    `.config/git.tpl.toml` receives `https://github.com/org/...`, and the template id — and so `refs/tpl/<id>` —
    is derived from that.
    A shortcut never leaves your machine.
    If it did, a project you created would be unusable by anyone without your file, and every contributor would
    derive a different ref for the same template.

The rules, all of them:

- Only the URL you type on the command line is expanded. A source read out of a repository never is, because
  expansion happens before git-tpl's internals see the argument at all.
- An unknown `foo:` is left alone — it may be a real scheme. Only names present in this file expand.
- Expansion happens once, never recursively. `ghs = "ssh://git@github.com/"` does not then expand as `ssh:`.
- A name may not contain `/`, and may not be `https`, `http`, `ssh`, `git` or `file`. Those are refused when the
  file is read, not when a shortcut is used.

`gh` and `ghs` are separate names rather than one name whose scheme git-tpl guesses.
The reason to want the SSH form is a private repository, and inferring which one you meant from whether a clone
failed is exactly the retry logic that produces incomprehensible authentication errors.

### `[trust]`

Templates whose network data sources are reached without asking you first — both [remote](data/remote.md) URLs
and [Git](data/git.md) repositories.
An entry names a *template*, and covers everything that template declares.

```toml
[trust]
templates = [
  "github.com/noirbizarre/*",
  "github.com/myorg/**",
]
```

Patterns are matched against the template's source URL, normalised first: the scheme, any userinfo, the port, a
trailing `.git` and a trailing slash are dropped, backslashes are read as path separators, and what is left is
folded to lower case.
One entry therefore covers every way of writing the same repository:

```
github.com/org/t   matches   https://github.com/org/t
                             git@github.com:org/t.git
                             ssh://git@github.com:22/org/t/
                             gh:org/t          (shortcuts expand first)
```

You may write a pattern as a full URL if that is what you have to hand — both sides are normalised the same way.

Globs only, over `/`-separated segments:

| | |
|---|---|
| `*` | any run of characters **within** one segment |
| `**` | any number of segments |

No regular expressions and no negation.
This list decides whether a fetch happens, and a trust list that needs debugging is a trust list that will be got
wrong.

**A match grants even when nothing can be asked** — under `--defaults`, in CI, anywhere.
The entry is prior consent, deliberately written, and no weaker than `--trust`.
A template the list does *not* name is still refused, loudly, when there is nobody to ask: nothing is granted by
omission.

**What trust gates.** Only what a template asks git-tpl to do on its behalf, which today means any network access
its data sources require: a [remote](data/remote.md) fetch, and a [Git](data/git.md) source's clone.
Rendering never requires trust, because a template cannot execute anything, trusted or not.

Note the name: the project file is `git.tpl.toml`, mirroring `git tpl`; this one is named after the binary.
Two shapes, so a stray copy of one is never mistaken for the other.

## Git configuration

Local, per-user, per-repository behaviour.
All keys are under `tpl.`.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `tpl.remote` | string | `origin` | Remote used by `git tpl fetch` and `git tpl push`. |
| `tpl.autoPush` | bool | `false` | Push the rendered ref after a successful `update`. |
| `tpl.interactive` | bool | `true` | Prompt for unanswered questions. `false` behaves as `--defaults`. |

```sh
git config tpl.remote upstream
git config --global tpl.interactive false
```

One family of keys outside `tpl.` is read: a question declaring
[`default_from`](templates/questions.md#machine-seeded-defaults) has the keys it names — `git:user.name`, or
`git.user.name` inside an expression — read, read-only, to pre-fill its prompt.
The same applies to the project's remote URL and directory name.
None of it is read when nobody is being asked, and a value reaches the project only as an answer you accepted.

### Precedence

Highest wins:

```
1.  command-line flags        --remote upstream
2.  repository config          .git/config
3.  user config                ~/.gitconfig
4.  system config              /etc/gitconfig
5.  built-in default
```

This is Git's own precedence, unchanged, because git-tpl reads these keys through libgit2's configuration
snapshot rather than parsing files itself.
`git config tpl.remote` tells you what git-tpl will use.

### Why these are not in `.config/git.tpl.toml`

They are preferences, not project identity.
`tpl.autoPush` is a statement about how *you* work; committing it would impose it on every contributor.
Conversely, the template source is a property of the project, and putting it in `.git/config` would mean a fresh
clone had no idea where the project came from.

### Why these are not in the user configuration either

Git already models remotes, and it already has a precedence chain across system, user and repository files that
people know.
Reimplementing either would mean `git config tpl.remote` no longer told you the truth.
The user configuration holds the three things Git has no opinion about: what a prompt should be pre-filled with,
what a URL prefix abbreviates, and which templates you have already agreed to let reach the network.

## The environment

git-tpl reads five environment variables and writes none.
Everything else it knows comes from a file you can point at.

| Variable | Read for |
|---|---|
| `XDG_CONFIG_HOME` | Where `git-tpl/config.toml` and the global ignore file live. Honoured only when **absolute**; an empty or relative value is unset, per the XDG specification. |
| `HOME` | The fallback for the above: `$HOME/.config`. |
| `USERPROFILE` | The Windows fallback for `HOME`, and the one libgit2 itself uses. Git for Windows usually exports `HOME`, but nothing guarantees it. |
| `NO_COLOR` | [no-color.org](https://no-color.org). Presence is the signal, whatever the value. |
| `CLICOLOR_FORCE` | [force-color.org](https://force-color.org). Any value but `0` forces colour. |
| `TERM` | `dumb` means no colour. |

Colour precedence, highest first — `--color` decides outright when it is not `auto`, and only then does the
environment get a say:

```
1.  --color always | never
2.  CLICOLOR_FORCE       set and not `0` → colour, even when not a terminal
3.  NO_COLOR             set at all → no colour
4.  TERM=dumb            → no colour
5.  is stderr a terminal?
```

`CLICOLOR_FORCE` outranks `NO_COLOR` and outranks not being a terminal, which is deliberate: that is what CI
systems set to get coloured logs, and a build that asked for colour explicitly has said something a heuristic has
not.

None of these reach a template.
A rendered tree cannot depend on where it was rendered — that is [invariant 2](concepts/determinism.md).
