# AGENTS.md

Notes for anyone — human or otherwise — changing this repository.

## What this project is

git-tpl renders a template into a Git ref. Updating the template advances that ref. The user merges it with plain
`git merge`.

That sentence is the whole design. Most proposals that feel like improvements are proposals to do something Git
already does, and they should be declined.

Read `docs/concepts/git-model.md` before changing anything structural.

## Non-negotiable invariants

Each of these is enforced by a hook or a test. If you find yourself working around one, you are about to break the
project.

1. **`update` does not modify `HEAD`, the index or the worktree.** Not "should not" — does not. The renderer writes
   into a `TreeBuilder`, so there is no code path that opens a project file for writing. Keep it that way.
   `tests/update.rs` fingerprints all three.

2. **Rendering is deterministic.** Same template revision, same answers, same data → byte-identical tree. This is
   what makes an unchanged template produce no commit. Never introduce a timestamp, an environment read, a hash-map
   iteration over user data, or a filesystem traversal order.

3. **Template refs are append-only.** The parent is the previous tip, always. Never amend, never force. Rewriting
   destroys the merge base a user's branch depends on.

4. **`git2` appears only in `src/git/libgit2.rs`.** The `git-backend-isolation` prek hook fails the commit otherwise.
   If you need a Git capability, add it to the `GitBackend` trait — do not import `git2` "just this once".

5. **Templates cannot execute code.** No subprocess, no shell, no eval, no hooks. Network access exists only in
   `src/data/` — the `http-isolation` prek hook fails the commit otherwise. Template repositories and remote data
   are untrusted input.

## Layout

```
src/
├── lib.rs           the library surface
├── main.rs          the git-tpl binary
├── exit.rs          exit codes, defined once
├── config.rs        .config/git.tpl.toml
├── gitconfig.rs     tpl.* preferences and their precedence
├── refs.rs          template id → refs/tpl/<id>
├── provenance.rs    commit trailers
├── template/        manifest, questions, the Value type
├── context.rs       the shared evaluation context
├── graph.rs         the dependency DAG
├── eval.rs          expression evaluation and prompting
├── render.rs        the tree walk
├── answers.rs       --answers-from files
├── userconfig.rs    ~/.config/git-tpl/config.toml
├── seed.rs          the machine-seeded prompt defaults (ADR-018)
├── remote.rs        remote URL parts, for seeding
├── lint.rs          static template analysis
├── note.rs          terminal-safe rendering of a template's note (ADR-019)
├── suggest.rs       "did you mean?"
├── data/            data sources
├── git/             the Git abstraction
│   ├── mod.rs       GitBackend — our types, never git2's
│   ├── ignore.rs    .gitignore evaluation, ours not libgit2's (ADR-017)
│   └── libgit2.rs   the only implementation
├── ops/             orchestration, one function per command
│   ├── mod.rs       init, update, status, diff, merge, fetch, push, and the rest
│   ├── resolve.rs   fetching a template to a revision
│   ├── backport.rs  the patch that carries a fix upstream (ADR-020)
│   ├── hunks.rs     interactive hunk selection (ADR-023)
│   ├── unsubstitute.rs  reversing a substitution in a change (ADR-022)
│   └── testing.rs   running a template's own tests (ADR-016)
├── cli.rs           argument types only
├── report.rs        the --json envelope, success and failure
├── theme.rs         formatting helpers that return String
├── prompt.rs        the demand-based prompter
└── commands/        one module per subcommand
```

Dependencies point inward. Nothing below `ops` knows a command exists; nothing in `template/` or `render.rs` knows
about the CLI.

## When you touch...

**`src/template/` or `src/config.rs`** — the on-disk format is a contract. A breaking change needs a `!` on the
commit and an ADR.

**`src/render.rs`** — re-read invariant 2. Anything that could vary between two runs is a bug, not a feature.

**`src/git/mod.rs`** — adding to the trait is right; adding a `git2` type to a signature is not.

**`src/ops/`** — this is where the commands' semantics live: `mod.rs` for the commands that need one function, a
file of its own for the ones that do not — `backport`, `testing`, `hunks`, `unsubstitute`, `resolve`. A change here
almost certainly needs a documentation change in `docs/usage/`.

**Anything user-visible** — update the corresponding page under `docs/`. In the same PR. A feature is not finished
when it works; it is finished when someone else can find out that it works.

## Style

**Every non-obvious line carries a comment saying why.** Not what — the code says what. Ideally naming the failure
it prevents:

```rust
// Rejected rather than resolved. A template repository is untrusted input,
// and `..` here is a request to write outside the tree.
```

A comment that restates the code is worse than none. A comment recording the bug that motivated the line saves the
next person an afternoon.

**One name per concept.** Two concepts here are easy to conflate, so they have fixed names:

| Name | Type | Meaning |
|---|---|---|
| `reference` | `String` | the name asked for — a branch, tag, SHA, or `<worktree>` |
| `revision` | `Oid` | the commit it resolved to |

`revision` never names a `String`. A field holding the printable form of the pair is `*_description`, and is
produced by `ops::describe_revision` — never by a `format!` at the call site, or the two ends of a `A → B` line come
to disagree. The config key and CLI flag stay `ref`, because that is what a user writes.

**Errors are typed and actionable.** `thiserror` for the library, `miette` at the binary edge. A diagnostic must
carry the two things the user does not already know: what specifically failed, and what to do. Compare:

```
x could not load template data source `things`          ← useless
help: source: data/absent.toml                          ← useful
      reason: no such file in the template repository at revision ffa9b4a
```

Diagnostic codes are `tpl::<area>::<kind>`, where `<area>` is the declaring module's own name and a `mod.rs` takes
its directory's. So `src/ops/mod.rs` is `tpl::ops`, `src/ops/resolve.rs` is `tpl::resolve`, and
`src/template/value.rs` is `tpl::value` — the parent never appears. A code is a public identifier users grep for,
so renaming one is a breaking change.

**Test names are sentences.** `an_unchanged_template_produces_no_commit`, not `test_update_2`. The name should say
what would be broken if it failed.

**Integration tests use real Git.** No mocks, ever. The premise of the project is that Git's behaviour is the
behaviour; a test against a stub would test the stub. `tests/common/mod.rs` builds real repositories in temporary
directories.

**Documentation wraps on meaning, not on a column.** Break a line after a sentence, before a coordinating
conjunction joining two independent clauses, or before the colon that introduces a list — never by filling to a
fixed width. The one hard limit is 120 characters: past that, break at the nearest earlier clause boundary, not
mid-word. `docs/` and this file both follow it; `markdownlint-cli2` (`mise run lint:md`, and the prek hook of the
same name) enforces the 120-character ceiling, not the "semantic" half — that's a review concern.

## Driving git-tpl without a person

Every command takes `--json` and every failure carries a stable `error.code` — `docs/reference/diagnostics.md` is
the catalogue. Branch on the code, never on the message; messages are expected to improve and are pinned nowhere.

Almost every command also emits a success payload. `show`, `completion` and `man` do not: their stdout is already
the payload — a file's bytes, a shell script, troff — and wrapping it would leave nothing able to read, source or
render it.

The loop for working on a template, none of which needs a repository:

```sh
git tpl --json questions ./tpl        # the answer schema, in resolution order
git tpl --json lint ./tpl --dirty     # what a render would not tell you
git tpl render ./tpl --dirty -o /tmp/out --answers-from answers.toml --defaults
git tpl --json context ./tpl --eval '{{ some | filter }}'
```

Then check the *output* with the tools that understand it — `cargo build`, `actionlint`, whatever the template
emits. git-tpl runs nothing over a rendering and never will; that is invariant 5, not an omission.

## Commits

Conventional Commits, enforced by commitlint on `commit-msg`. The type becomes a changelog heading, so choose it as
if someone will read it in release notes — because they will.

## Before you push

```sh
mise run ci
```

Formatting, Clippy, spelling, workflow and documentation linting, tests and the documentation build. Same as CI.

## Things that will be declined

With reasons, in `docs/development/contributing.md`. Briefly: custom merge logic, a second template engine, code
execution from templates, runtime values in the render context, automatic ref push/fetch, Copier compatibility.

If you think one of these is wrong, the way to change it is an ADR that supersedes the existing one — not a PR that
quietly works around it.
