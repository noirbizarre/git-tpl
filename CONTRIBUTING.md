# Contributing to git-tpl

## Setup

The toolchain is managed by [mise](https://mise.jdx.dev/); every tool is pinned
to an exact build in `mise.lock`, so a local run and a CI run use the same
versions.

```bash
mise install          # the tools
prek install          # the Git hooks
```

### Mr Boxington build caching

[Mr Boxington](https://mr-boxington.jdx.dev/setup) is enabled through mise's
native Rust option (requires mise 2026.9.2+). Rustup provides the stable channel
declared in `rust-toolchain.toml`; keep that channel and the mise Rust version
in sync when changing a pin, preserving existing components and other options.
Explicit `RUSTUP_TOOLCHAIN` selections win. In GitHub Actions, the runtime
preserves the rustup default selected by `dtolnay/rust-toolchain`, including
MSRV jobs. Mise still ensures its stable install and declared components exist;
the install request stays stable so locked installs remain valid.

```bash
mise exec -- cargo build --locked
mise exec -- cargo nextest run
mise exec -- mbx doctor
```

**Mise tasks have mbx built in:** `mise build`, `mise test`, and their explicit
forms `mise run build` and `mise run test` already route Cargo through mbx; no
extra `mbx` prefix is needed. Plain terminal `cargo` requires effective
mise activation or shims; a direct rustup Cargo proxy bypasses it. Coding agents
should use `mise exec` rather than assume the terminal's PATH. Native integration
does not require `mbx setup` or configure rust-analyzer. Doctor can warn about a
missing standalone shim even when wrapping works; verify real build summaries.
If personal environment paths shadow the wrapper in task shells, enable
`[settings] activate_aggressive = true` in ignored `mise.local.toml` or use the
explicit `mise exec -- cargo ...` route.

An existing compiler wrapper makes mbx delegate rather than cache. Inspect
Cargo configuration and `RUSTC_WRAPPER` before changing one; an authorized
project-only `[build] rustc-wrapper = ""` override can clear an inherited wrapper.
Environment-selected wrappers still take precedence and prevent mbx caching.

CI uses `jdx/mr-boxington-action` instead of sccache and rust-cache. Its default
GitHub `target` payload includes Cargo outputs and registry/git downloads, so
an additional rust-cache action is unnecessary. Successful main pushes populate
the cache; same-repository pull requests also save in their own PR scope.
Fork pull requests only restore. Cross-build jobs retain their container-produced
target outputs through that payload; native builds also use compiler caching.
Publication jobs use `backend: local` without shared compiler-output restores
or saves. No separate remote cache service or local-to-CI connection is configured.

Local compiler actions, eligible links, build scripts and C/C++ compilations
are cached by default. Compilation scheduling, in-flight deduplication and
learned private incremental reuse are also enabled. For concurrent agents or
test suites, optionally add `[scheduler] tests = true` in workspace `.mbx.toml`;
it schedules tests, not their outcomes. Review nextest timeouts if tests queue.

This feature disables automatic collection and managed target placement through
`MBX_GC_AUTO=0` and `MBX_TARGET_VIEWS=0`, preserving existing outputs and caches.
To demonstrate reuse, run equivalent builds in two fresh temporary target
directories using `--target-dir` and `MBX_SUMMARY=full`. Require positive mbx hits
on the second build; Cargo freshness is not a hit. Diagnose with
`mise exec -- mbx explain --last` and remove only your temporary targets.

To undo, remove or disable `mr_boxington` first and run `mise reshim`, then remove
the mbx tool/lock entry and introduced environment settings. Restore rustup-only
ownership by removing the added mise Rust entry, preserving unrelated pins and
options. Remove any wrapper/scheduler and local activation overrides you added;
keep the shared cache. Set the template answer `mbx = false` and review the
rendered update to restore the original CI cache actions too.

Before committing or opening a PR, choose checks relevant to your changes;
see [Before you push](#before-you-push) for guidance. Clippy runs through
`mise lint` and in CI, rather than in the commit hooks. `mise ci` is an
optional comprehensive check.

## Tasks

| Command | What it does |
|---|---|
| `mise build` | Build the project |
| `mise cli` | Run `git-tpl` from source (passes flags through) |
| `mise test` | Run the tests (accepts nextest selectors) |
| `mise cover` | Run the tests with coverage |
| `mise format` | Format |
| `mise lint` | Clippy, warnings denied |
| `mise lint:actions` | actionlint over the workflows |
| `mise lint:md` | markdownlint over AGENTS.md, CONTRIBUTING.md, README.md and docs/ |
| `mise spell` | typos |
| `mise snapshots` | Review pending insta snapshots |
| `mise check` | Everything that does not modify the working tree |
| `mise ci` | `check` plus the documentation build |
| `mise docs` | Serve the documentation locally |
| `mise changelog` | Preview the changelog for unreleased commits |
| `mise release` | Show the version the next release would take |
| `mise tpl:update` | Bring the rendered template ref up to date |
| `mise tpl:diff` | Show what merging the template would change |

`mise <task>` is a shorthand for `mise run <task>`, but a builtin subcommand of
the same name wins it silently — which is why the format task is `format` and
not `fmt` (`mise fmt` formats `mise.toml`), and why running the binary is
`mise cli` and not `mise run` (`mise run` runs a task). Prefer the explicit
`mise run <task>` in scripts: mise can claim a new name in any release.

## Before you push

Choose checks based on the files changed and the risk. Run relevant tasks
individually rather than launching every check in parallel by default.

- **Rust code:** run `mise lint` (Clippy) and the relevant tests through
  `mise test`, which accepts nextest selectors. Target affected packages
  and tests instead of running the whole suite by default. For example,
  replacing the placeholders with the affected package and test name:

  ```sh
  mise test -p <package> -E 'test(<test_name>)'
  ```

- **Documentation, workflows or tooling:** run the relevant checks, such as
  `mise lint:md`, `mise spell` or `mise lint:actions`, and
  `mise docs:build` when documentation changes warrant a build.
  Documentation-only changes do not require Rust lint or tests.

Broaden validation when the change warrants it. `mise ci` is available as
an optional comprehensive check, not a prerequisite for committing, pushing or
opening a PR.

If a local check is unavailable or prohibitively expensive, proceed with the
commit or PR and report which checks ran, which were skipped and why in the
handoff or PR description. CI provides full validation.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/), enforced by
commitlint on `commit-msg`. The type selects the changelog section, and a `!`
or a `BREAKING CHANGE:` footer drives the version bump — so the message is part
of the release, not paperwork around it.

The official typos hooks check source files on `pre-commit` and commit messages
on `commit-msg`. Message spelling is checked without rewriting the message, so
fix any reported errors before retrying the commit.

## Releases

Releases are run by [gh-ship](https://github.com/noirbizarre/gh-ship).
**Never bump a version or push a tag by hand.**

1. A push to `main` triggers 🚢 Ship, which runs `gh ship run` and selects preparation.
2. `prepare` dispatches 🚀 Prepare Release, which asks git-cliff for the next
   version, writes `CHANGELOG.md`, bumps `Cargo.toml`, commits, and uploads a
   `ship.release.json` artifact describing what would ship.
3. gh-ship opens (or updates) the Release PR from `release/next`. Review it.
4. Merging it triggers 🚢 Ship again, where `gh ship run` selects publication: it tags the
   merge commit, creates a draft release, dispatches 📦 Publish Release to
   attach the binaries and publish to crates.io, then makes the
   release public.

Nothing to release is the normal case for step 1, and costs one workflow run
reporting `changed: false`.

`gh ship validate` runs on every pull request, so a broken release contract
fails on the PR rather than mid-release.

### Repository requirements

The release jobs authenticate as a GitHub App, not with `GITHUB_TOKEN` — the
default token cannot trigger workflows, so a Release PR it authored would show
no CI results. That means the repository needs:

- a `release` environment holding the variable `APP_CLIENT_ID` and the secret
  `APP_PRIVATE_KEY`.
- squash-merge settings of `squash_merge_commit_title: PR_TITLE` and
  `squash_merge_commit_message: BLANK`, so the squash commit subject is the
  Conventional Commit title from `.github/ship.yml`.
- Trusted Publishing configured on crates.io for the `release` environment.
- a `homebrew` environment holding the secret `TAP_TOKEN`, scoped to push to
  `noirbizarre/homebrew-tap` only.
- an `aur` environment holding the secret `AUR_SSH_PRIVATE_KEY`, for the AUR
  account that owns the `git-tpl-bin` package.

## This repository is generated from a template

The toolchain, hooks, CI and release workflows come from
[rust.tpl](https://github.com/noirbizarre/rust.tpl):

```bash
git tpl status         # is there a template update pending?
mise tpl:update    # advance refs/tpl/<id> — HEAD, index and worktree untouched
mise tpl:diff      # read what merging it would change
git tpl merge          # take it
```

`tpl:update` is safe to run at any time: it only advances the rendered ref.
Nothing reaches your branch until the merge.

Requires git-tpl on your PATH (`cargo install git-tpl`). It is not declared in
`mise.toml`'s `[tools]` on purpose — it vendors libgit2, so a global entry
would make every CI job compile a tool no CI job runs.

Files carrying template-owned content — `mise.toml`, `prek.toml`, `Cargo.toml` —
end with a `# --- project-specific ---` marker. Add below it; Git's 3-way merge
then preserves your additions across updates. A fix that belongs to every
project belongs in the template, not here.
