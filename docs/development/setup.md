# Development setup

## Prerequisites

- [Rust](https://rustup.rs) — the channel is pinned by `rust-toolchain.toml`
- [mise](https://mise.jdx.dev) — everything else installs itself

```sh
git clone https://github.com/noirbizarre/git-tpl
cd git-tpl
mise install
prek install
```

`mise install` reads `mise.toml` and `mise.lock`, so you get the same tool
versions CI does. `prek install` wires up the Git hooks.

Rust itself is deliberately not managed by mise: CI needs per-job components
(`rustfmt`, `clippy`, `llvm-tools-preview`) and cross-compilation targets, which
`dtolnay/rust-toolchain` handles better. `rust-toolchain.toml` pins the channel
for both.

## Tasks

```sh
mise run              # fmt, lint, lint:actions, build, test — the default
mise run build
mise run test         # accepts nextest selectors: mise run test render
mise run fmt
mise run lint
mise run spell
mise run cover
mise run check        # everything that does not modify the tree
mise run ci           # check + docs:build
```

```sh
mise run tpl -- status        # run git-tpl from source
mise run setup                # cargo install --path . --force
```

```sh
mise run docs                 # serve the documentation at localhost:8000
mise run docs:build
```

`mise tasks` lists them all.

## Layout

```
src/
├── lib.rs           the library surface
├── main.rs          the git-tpl binary
├── exit.rs          exit codes, defined once
├── config.rs        .config/git.tpl.toml
├── gitconfig.rs     tpl.* keys and their precedence
├── refs.rs          template id → refs/tpl/<id>
├── provenance.rs    commit trailers
├── template/        manifest, questions, the Value type
├── context.rs       the shared evaluation context
├── graph.rs         the dependency DAG
├── eval.rs          expression evaluation and prompting
├── render.rs        the tree walk
├── answers.rs       --answers-from files
├── data/            data source abstraction and loaders
├── git/             the Git abstraction
│   ├── mod.rs       the GitBackend trait — our types, never git2's
│   └── libgit2.rs   the only implementation
├── ops/             orchestration, one function per command
├── cli.rs           argument types only
├── theme.rs         formatting helpers that return String
├── prompt.rs        the demand-based prompter
└── commands/        one module per subcommand
```

`ops/` is a module with one *function* per command — `init`, `update`,
`status`, `diff`, `merge`, `fetch`, `push` — plus `resolve` for fetching a
template. `commands/` is the directory with one module per subcommand; that is
where argument handling and output formatting live, and nothing else.

Dependencies point inward. `ops` uses `render`, `graph`, `git`; nothing in
`template/` or `render.rs` knows a command exists.

## Invariants

These are enforced, not merely intended. Breaking one fails a hook or a test.

**`git2` appears only in `src/git/libgit2.rs`.** The `GitBackend` trait is the
boundary; a `use git2::Oid` anywhere else makes it decorative. The
`git-backend-isolation` prek hook is what actually stops that. If you need a
Git capability the trait lacks, add it to the trait — not a `git2` import.

**`update` does not modify the worktree.** An integration test asserts `HEAD`,
the index and the worktree are byte-identical across an update. The renderer
writes to a Git tree builder and never to the filesystem, so this is structural —
the test exists to keep it that way.

**Rendering is deterministic.** A test renders twice and compares trees. See
[Determinism](../concepts/determinism.md).

**No code execution from templates.** No subprocess, no shell, no eval, no HTTP
outside `src/data/`.

## Tests

```sh
mise run test
mise run test init          # nextest selectors
cargo nextest run --no-capture
```

Unit tests live at the bottom of the module they test. Integration tests are in
`tests/` and build **real** Git repositories in temporary directories — nothing
about Git is mocked, because the entire premise of the project is that Git's
behaviour is the behaviour.

Test names are sentences: `an_unchanged_template_produces_no_commit`,
`a_cycle_is_reported_before_any_prompt`.

Snapshots use `insta`:

```sh
mise run snapshots     # cargo insta review
```

### Git identity

The integration tests commit, and libgit2 refuses to build a signature without
one:

```sh
git config --global user.name  "Your Name"
git config --global user.email "you@example.com"
```

## Style

Follow what is there. The one convention worth stating explicitly: **every
non-obvious line carries a comment saying why**, ideally naming the failure it
prevents. A comment that restates the code is worse than none; a comment that
records the bug that motivated the line saves the next person an afternoon.
