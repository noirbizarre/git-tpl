# ADR-033: A test case may ask for an isolated Git sandbox

**Status:** accepted

**Relates to:** [ADR-016](016-template-tests-are-data.md), [ADR-027](027-test-case-commands.md),
[ADR-011](011-git-backend-isolation.md)

**Narrows:** ADR-027's "`before` sees nothing but what it creates itself" to a case that has not also set `git`.

## Context

`[commands]`'s sandbox (ADR-027) is a plain scratch directory — never a Git repository. Most tooling a case's
`[commands]` might run does not care, but anything that reads Git for its own identity does: `pdm-backend`'s
`source = "scm"` — the *generated* project's own version source, not the template's — has nothing to read at all
in a directory with no `.git`, and fails before installing a single dependency. So does every other
`setuptools_scm`-alike source, and `git describe`, and `git rev-parse --verify HEAD`.

A template with several cases that all need `pdm install` to succeed ends up repeating the same five lines,
verbatim, in every one of their `before` lists:

```toml
[commands]
before = [
  "git init -q",
  "git config user.email smoke@test",
  "git config user.name smoke",
  "git config commit.gpgsign false",
  "git commit -q --allow-empty -m init",
]
```

`commit.gpgsign false` is there because a case cannot assume the machine running `git tpl test` has no ambient
signing configuration — without it, a real `git commit` in `before` can hang waiting on a GPG agent with no TTY to
prompt on, which is worse than a failure: it hangs the run with no error at all.

That line only defuses the one ambient setting most likely to bite. `HOME`, `GIT_CONFIG_GLOBAL` and
`GIT_CONFIG_SYSTEM` are untouched, so a case's result can still depend on whatever else is sitting in whichever
machine's ambient Git configuration happens to be running the suite — a credential helper, a custom hook, an
alias — which sits oddly next to `git tpl` having a whole "Concepts > Determinism" page. Every case that needs a
repository has to individually rediscover and defend against the next ambient setting that leaks through, the way
`commit.gpgsign false` already is one case author's own discovery rather than something the tool guarantees.

## Decision

A case may set `git`, a sibling of `answers`/`trust`/`expect`/`commands`/`snapshot`:

```toml
git = true
```

or, overriding the identity:

```toml
[git]
user.name = "Someone"
user.email = "someone@example.com"
```

parsed exactly like `snapshot` — a plain boolean, or (unlike `snapshot`) a table overriding two fixed leaves —
defaulting to `false`/absent for the same reason `snapshot` does: an isolated repository is an extra assertion a
case opts into, not a side effect of anything else it wrote.

When set, the sandbox is seeded — before `before` runs its first entry — with one empty commit, authored with the
chosen identity, or a built-in synthetic default (`git-tpl test <test@git-tpl.invalid>`, `.invalid` per RFC 2606,
so nobody mistakes it for a real address) when the case wrote `git = true` rather than overriding it.

### Seeding never spawns a process

The repository is created and seeded entirely through git-tpl's own Git backend (`LibGit2::init_isolated`,
`set_config_str`, `build_tree`, `create_commit`, `set_ref` — all pre-existing, `src/git/mod.rs`), never a spawned
`git`. This is what makes the seed commit's identity unconditional rather than merely likely: it is written to the
sandbox's own local `.git/config` before anything asks for a signature, and Git's own precedence — local always
outranks global and system — makes it authoritative regardless of what either ambient level says, with no ambient
hook, alias or credential helper ever consulted, because none of the machinery that would consult them (a spawned
`git`) runs at all for this part. `commit.gpgsign` is set to `false` in that same local config, so a case's own
`[commands]` that later run a literal `git commit` cannot hit the exact hang this ADR's Context describes.

`LibGit2::init_isolated` also pins the initial branch to `main` (`RepositoryInitOptions::initial_head`), rather
than reading whatever `init.defaultBranch` the running machine's global or system config happens to declare — a
smaller leak than an ambient identity or a signing prompt, but the same class of thing, and one line to close.

### A case's own commands are isolated by environment, not by fighting config levels

Seeding needs no isolation trick because it never asks Git to search anything. A case's own `[commands]` are
different: the entire reason to ask for `git` is so they can run a literal `git`, and a spawned process does
search the running machine's config exactly as any `git` invocation would. So every command in a case that set
`git` — not only the ones that come after seeding — is spawned with `GIT_CONFIG_NOSYSTEM=1` and `GIT_CONFIG_GLOBAL`
pointed at a path inside the sandbox's own `.git/` that is never created, so Git treats "global" as empty rather
than reading the running machine's `~/.gitconfig`. This mirrors `tests/common/mod.rs::scrub_git_env`, written for
the identical reason one layer further out: isolating this project's own test suite from the developer's
`~/.gitconfig`.

The two variables are merged into each list's own resolved `env` (`commands.env`, a list's own override) at a
*lower* precedence than either — the exact override precedent `TEMPLATE_ROOT` and the colour variables already
have (ADR-027) — so a case that deliberately wants a different value for either still gets it. Riding the same
`env: &BTreeMap<String, String>` parameter those already pass through `execute_commands`/`run_one`/
`expand_command` means the isolation variables are visible to `{{ env.* }}` templating for free, and can never
disagree with what the spawned process actually receives, because both come from the one merged map.

`HOME` is deliberately not touched: `GIT_CONFIG_GLOBAL` alone overrides the location Git would otherwise compute
from `HOME`/`XDG_CONFIG_HOME`, so there is nothing left for `HOME` to leak through for Git's own purposes, and
changing it for the whole spawned command risks breaking a tool that needs it for an unrelated reason (locating
its own cache, say) that Git's own isolation has no business touching.

### `--skip-commands`/`tpl.testCommands` disable `git` too

Both existing switches (ADR-027) disable `[commands]` for a run; they now also skip a case's `git`, as a
corollary rather than a second toggle. There is nothing for an isolated repository to prove if nothing runs
inside it, and a person who has already said "skip the commands" has said everything relevant about whether the
sandbox needs to be anything more than a scratch directory.

### Reuses the existing `sandbox_failed` diagnostic

A seeding failure — `LibGit2::init_isolated` or a subsequent `GitBackend` call returning an error — is a fact
about the machine running the suite, not about the template, exactly the class of failure
`tpl::testing::sandbox_failed` already covers for the sandbox's own tempdir creation. It aborts the run rather
than being recorded as a case failure, and reuses that code rather than minting a new one.

### Forward compatibility

An old git-tpl reading a case with `git` already fails loudly today, at the existing deny-unknown-key check in
`Case::parse` — `` `git` is not a test case key`` — the same property `commands.env` (#130) and `lacks` (#87)
established for their own additive case-schema changes: never a silent no-op.

## Consequences

**What stays closed.** `git2` remains confined to `src/git/libgit2.rs` (ADR-011) — the one new constructor,
`LibGit2::init_isolated`, is inherent, alongside `init`/`discover`/`open`/`clone_bare`, per the trait's own
membership rule: it produces a backend rather than uses one. Invariant 5 is untouched: nothing here spawns a
process that was not already ADR-027's own, pre-existing exception, and seeding itself spawns nothing at all.

**What changes.** `src/ops/testing.rs` calls into `GitBackend` to *write*, for the first time — every existing use
there is a read (`workdir`, `subtree`, `list_tree`, `read_blob`) against the already-resolved template. The write
lands only in a case's own throwaway sandbox, never touching invariant 1's worktree.

**On-disk contract.** `git` is an addition to the case schema ADR-016 already governs, with the same permanence.

**Cost.** No new dependency. No new `GitBackend` trait method: `set_config_str`, `set_config_bool`, `build_tree`,
`create_commit` and `set_ref` already existed for other callers (`init`, tests). `execute_commands`/`run_one`/
`expand_command` need no signature change, since the isolation variables ride their existing `env` parameter.
