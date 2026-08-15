# ADR-013: User preferences live in `~/.config/git-tpl/config.toml`

**Status:** accepted

## Context

Three unrelated frustrations share a shape: they are facts about a *person*, not
about a project.

- Retyping your name and email into every project you generate.
- Typing `https://github.com/` twenty times a day.
- Confirming the same trusted template's network access on every single run.

None of them can live in `.config/git.tpl.toml`. That file is versioned and
shared, and a project cannot consent to network access on its reader's behalf,
nor decide what someone else's name is. None of them belong in `tpl.*` Git
configuration either: Git models remotes, branches and pushing, and it does not
model "what should this prompt be pre-filled with".

So there is a third owner, and a third file.

## Decision

```
$XDG_CONFIG_HOME/git-tpl/config.toml     (default: ~/.config/git-tpl/config.toml)
```

Three files, three owners, stated once:

```
.config/git.tpl.toml          →  the project. Versioned. Everyone gets it.
~/.config/git-tpl/config.toml →  you. Never committed, never read by anyone else.
.git/config, ~/.gitconfig     →  tpl.* preferences (ADR-010, unchanged)
```

Exactly three sections, and deliberately nothing else. Anything describing the
project belongs in `.config/git.tpl.toml`; anything Git already models belongs in
`tpl.*`.

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

An absent file is not an error — it is the normal case.

### A directory, not a flat file

`PLAN.md` originally recorded `~/.config/git-tpl.toml`. This ADR supersedes that
detail: the file is `~/.config/git-tpl/config.toml`.

XDG's `$XDG_CONFIG_HOME` is a directory of *application* directories. A tool that
puts a bare file there has to move it the first time it wants a second one, and
that move is a breaking change to a path users have written down. git-tpl already
has two candidates for a second file — a template cache and a trust store that
learns — so the directory costs one `mkdir` now and saves a migration later.

The naming asymmetry with `.config/git.tpl.toml` is still deliberate, and is
still the point: the project file is Git-shaped (`git.tpl`, mirroring
`git tpl`), the user file is named after the binary and follows XDG. Two shapes
means a stray copy of one is never mistaken for the other.

Resolution is `$XDG_CONFIG_HOME`, else `$HOME/.config`, and is hand-written
rather than taken from a crate — it is six lines, and it is the same rule
`src/git/libgit2.rs` already applies when it looks for SSH keys.

### `[defaults]` seeds prompts and nothing else

This is the whole design, and it is what keeps rendering deterministic
(invariant 2).

A key matching a question name becomes that question's **prompt default** — the
pre-filled text, exactly as an implemented `default_from = "git:user.name"`
already produces. The value the user accepts is recorded in
`.config/git.tpl.toml` like any other answer, so the project stays reproducible
for someone who has never seen your file.

If the question is **not asked** — `--defaults`, `tpl.interactive false`, CI —
the file is ignored entirely and the template's own `default` applies. Otherwise
the same template revision with the same answers renders two different trees on
two machines, and "an unchanged template produces no commit" becomes false.

Precedence, stated once:

```
--answer  >  --answers-from  >  answers in .config/git.tpl.toml
          >  [defaults]  >  default_from  >  the question's default
```

Keys naming no question are ignored and reported, exactly as for
`--answers-from`. There is no per-template namespacing in v1;
`[defaults."github.com/org/*"]` remains additive if it proves necessary.

### `[shortcuts]` expand at the CLI edge, and never leave the machine

Prefix substitution on a leading `<name>:`. One rule makes it safe:

> **The expanded URL is what gets written to `.config/git.tpl.toml`, and what
> derives the template id.**

Without it, a project created by someone with a `mine:` shortcut is unusable by
everyone else, and `refs/tpl/<id>` differs per contributor for the same
template — an invariant-3 problem wearing a convenience feature's clothes.

Expansion happens in `commands`, on the CLI argument, before `ops` is called.
`PLAN.md` said "in `ops`"; doing it one layer out is strictly stronger, because
it makes the rule structural: `ops` never sees an unexpanded source, so a
shortcut *cannot* be matched against a value read out of a repository, and no
test is needed to keep it that way.

Also decided:

- An unknown `foo:` is left alone — it may be a real scheme. Only names present
  in the file expand, and expansion is never recursive.
- `gh:` and `ghs:` are separate names rather than a scheme guessed from context.
  The reason for the SSH form is private repositories, and inferring which one
  you meant from whether a clone failed is exactly the retry logic that produces
  incomprehensible authentication errors.
- A name may not contain `/` and may not be a known scheme (`https`, `http`,
  `ssh`, `git`, `file`). Rejected when the file is read, not when it is used —
  a shortcut that shadows `https:` should fail on the day you write it.

### `[trust]` is a user-side fact, and grants everywhere

Patterns match the expanded source URL, normalised first: scheme, userinfo, port
and a trailing `.git` dropped, case folded, scp-style `host:path` written
`host/path`. One entry therefore covers `https://github.com/org/t`,
`git@github.com:org/t.git` and `gh:org/t`.

Globs only — `*` within a path segment, `**` across segments. No regex and no
negation, because a trust list that needs debugging is a trust list that will be
got wrong.

**A match grants even when nothing can be asked.** A pattern is prior consent,
written deliberately, and is no weaker than `--trust`. Refusing it in CI would
mean the only way to use a trusted template non-interactively is to pass
`--trust` on every invocation, which teaches people to pass `--trust`
unconditionally — the opposite of the intent.

An *unmatched* template is still refused loudly when there is nobody to ask,
naming what was refused and saying that `--trust` or a `[trust]` entry would
allow it. Never granted by omission: a CI runner is the worst possible place to
acquire a capability by silence.

The normalisation is written for this purpose and is not shared with
`refs.rs`, which normalises in order to slugify a ref name. Coupling the two
would mean a change to trust matching could change a ref name, and refs are
append-only.

## Consequences

Trust becomes a thing you can state once, which is the only way a confirmation
prompt stays meaningful: a prompt answered `yes` twenty times a day is not a
decision, it is a keystroke.

Rendering is untouched. Trust gates only what a template asks git-tpl to do on
its behalf — currently one thing, a remote data fetch. It grants no new
capability, and invariant 5 is unchanged: no subprocess, no shell, no eval, from
a trusted template or any other.

Determinism is preserved by construction rather than by care: the user file
reaches only the prompt seed channel, which `DefaultsOnly` ignores outright and
which is not even populated when nobody is being asked. Two guards, because one
value from this file reaching a rendered tree would end the model.

The file is only ever hand-written, so unknown keys are rejected rather than
ignored — the opposite of the project file's rule, and for the opposite reason.
Nothing generates this file, so a key that is not understood is a typo.

The cost is a third place to look when a render surprises someone. That is paid
down by `[defaults]` being visible in the prompt it filled, and by the expanded
URL being what is recorded — neither leaves an invisible trace.
