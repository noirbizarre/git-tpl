# Development setup

## Prerequisites

- [Rust](https://rustup.rs) — the channel is pinned by `rust-toolchain.toml`
- [mise](https://mise.jdx.dev) 2026.9.2+ — installs tools and enables native Cargo caching

```sh
git clone https://github.com/noirbizarre/git-tpl
cd git-tpl
mise install
prek install
```

`mise install` reads `mise.toml` and `mise.lock`, so you get the same tool versions CI does. `prek install` wires
up the Git hooks.

Rustup supplies toolchains; mise declares stable Rust to enable native [Mr Boxington](https://mr-boxington.jdx.dev/)
Cargo caching and compilation scheduling.
`rust-toolchain.toml` and the mise Rust entry use the same channel.
An explicit `RUSTUP_TOOLCHAIN` wins; in CI, the runtime follows the rustup default selected by
`dtolnay/rust-toolchain`, preserving per-job components and targets.
Intel Macs use plain Cargo because MBX 1.28 has no Intel macOS executable.

Use `mise exec -- cargo ...` or mise tasks to exercise the wrapper;
a direct rustup Cargo proxy bypasses it.
`mise exec -- mbx doctor` checks the setup, and MBX build summaries distinguish cache hits from Cargo freshness.
Automatic cache collection and managed target relocation are disabled to preserve existing local outputs.
CI uses MBX's target payload for shared build caches and local-only caches for publication.
See [CONTRIBUTING.md](https://github.com/noirbizarre/git-tpl/blob/main/CONTRIBUTING.md) for cache details.

## Tasks

```sh
mise run              # format, lint, lint:actions, lint:md, build, test — the default
mise run build
mise run test         # accepts nextest selectors: mise run test render
mise run format
mise run lint
mise run spell
mise run cover
mise run check        # everything that does not modify the tree
mise run ci           # check + docs:build
```

```sh
mise run cli -- status        # run git-tpl from source
mise run setup                # cargo install --path . --force
```

```sh
mise run docs                 # serve the documentation (Zensical's default port)
mise run docs:build
```

`mise tasks` lists them all.
`mise run fmt`, `mise run fmt:check` and `mise run tpl` remain supported as aliases.
Clippy runs explicitly in CI and `mise run ci`, rather than during every commit.
Hooks check both source spelling and commit-message spelling without rewriting the message.

## Maintaining the project's template

This repository is attached to [rust.tpl](https://github.com/noirbizarre/rust.tpl).
`.config/git.tpl.toml` records its answers and tracks `main`; the rendered ref is merged into project history.

```sh
mise run tpl:update    # restore a missing baseline, then update the rendered ref
mise run tpl:diff      # preview the merge
git tpl merge         # apply it with an ordinary Git merge
```

`mise run tpl:check` updates first, then exits nonzero if merging would change files.
Updating never changes HEAD, the index or rendered worktree files;
semantically changed answers and composed child locks are recorded in `.config/git.tpl.toml`, unstaged.
The tasks restore a missing local template ref from merged rendering provenance, so fresh clones need full history.
The weekly/manual template workflow does the same and reports drift without pushing or merging anything.

The initial adoption preserves this project's public library name, release inputs, archive layouts,
runtime man/completion generation, both AUR packages, and architecture guards.
Git's merge base preserves those customizations on later updates; marker comments are not protected regions.

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
├── migration.rs     discovering and applying template migrations (ADR-024)
├── suggest.rs       "did you mean?"
├── data/            data sources
├── git/             the Git abstraction
│   ├── mod.rs       GitBackend — our types, never git2's
│   ├── ignore.rs    .gitignore evaluation, ours not libgit2's (ADR-017)
│   └── libgit2.rs   the only implementation
├── ops/             orchestration, one function per command
│   ├── mod.rs       init, update, status, diff, merge, fetch, push, and the rest
│   ├── resolve.rs   fetching a template to a revision, and its `[extends]` chain (ADR-034)
│   ├── extends.rs   merging an `[extends]` chain's manifests into one (ADR-034)
│   ├── backport.rs  the patch that carries a fix upstream (ADR-020)
│   ├── hunks.rs     hunk selection, by prompt or by name (ADR-023, ADR-037)
│   ├── unsubstitute.rs  reversing a substitution in a change (ADR-022)
│   └── testing.rs   running a template's own tests (ADR-016)
├── cli.rs           argument types only
├── report.rs        the --json envelope, success and failure
├── theme.rs         formatting helpers that return String
├── prompt.rs        the demand-based prompter
└── commands/        one module per subcommand
```

`ops/` holds one *function* per command in `mod.rs` — `init`, `update`, `status`, `diff`, `merge`, `fetch`, `push`,
`lint`, `questions` and the rest — and a file of its own for each command too large to be one: `backport`,
`testing`, `hunks`, `unsubstitute`, plus `resolve` for fetching a template and `extends` for merging an `[extends]`
chain. `commands/` is the directory with one module per subcommand; that is where argument handling and output
formatting live, and nothing else.

Dependencies point inward. `ops` uses `render`, `graph`, `git`; nothing in `template/` or `render.rs` knows a
command exists.

## Invariants

These are enforced, not merely intended. Breaking one fails a hook or a test.

**`git2` appears only in `src/git/libgit2.rs`.** The `GitBackend` trait is the boundary; a `use git2::Oid` anywhere
else makes it decorative. The `git-backend-isolation` prek hook is what actually stops that. If you need a Git
capability the trait lacks, add it to the trait — not a `git2` import.

**`update` does not modify `HEAD`, the index, or render output in the worktree.** An integration test asserts all
three are unchanged, or change only in the one expected way: `.config/git.tpl.toml` itself, rewritten in place when
a template question gains a newly recorded answer. The renderer writes to a Git tree builder and never to the
filesystem, so a rendered file changing is structural — the test exists to keep it that way.

**Rendering is deterministic.** A test renders twice and compares trees. See
[Determinism](../concepts/determinism.md).

**Template refs are append-only.** The parent is always the previous tip — `push` never forces. Rewriting a
template ref would destroy the merge base a user's branch depends on.

**No code execution from templates.** No subprocess, no shell, no eval, no HTTP data-source access outside
`src/data/`; git-backed network access (cloning or fetching a template) is confined to `src/git/libgit2.rs` by the
`git2`-isolation invariant above.
The deliberate process-execution exceptions are the test harness's `[commands]` (ADR-027)
and native Git display of a completed diff preview in `src/commands/diff.rs` (ADR-039).
Neither is available to a template render.

## Tests

```sh
mise run test
mise run test init          # nextest selectors
cargo nextest run --no-capture
```

Unit tests live at the bottom of the module they test. Integration tests are in `tests/` and build **real** Git
repositories in temporary directories — nothing about Git is mocked, because the entire premise of the project is
that Git's behaviour is the behaviour.

Test names are sentences: `an_unchanged_template_produces_no_commit`, `a_cycle_is_reported_before_any_prompt`.

Snapshots use `insta`:

```sh
mise run snapshots     # cargo insta review
```

They live in `tests/snapshots.rs`, and they pin the transcripts quoted by `docs/usage/` and the envelopes described
in `docs/reference/json.md`. A failing snapshot there is not necessarily a bug: it means documented output changed,
and accepting it commits you to the matching documentation edit in the same pull request.

### Git identity

The integration tests commit, and libgit2 refuses to build a signature without one:

```sh
git config --global user.name  "Your Name"
git config --global user.email "you@example.com"
```

## Style

Follow what is there. The one convention worth stating explicitly: **every non-obvious line carries a comment
saying why**, ideally naming the failure it prevents. A comment that restates the code is worse than none; a
comment that records the bug that motivated the line saves the next person an afternoon.
