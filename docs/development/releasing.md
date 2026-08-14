# Releasing

Releases are driven by [gh-ship](https://github.com/noirbizarre/gh-ship). There
is no manual step and no custom script.

## The flow

```
push to main
     │
     ▼
🚢 Ship  →  gh ship prepare
     │
     ▼
🚀 Prepare Release
     │  git-cliff computes the version and the changelog
     │  Cargo.toml is bumped, CHANGELOG.md written
     ▼
Release PR  (chore(release): X.Y.Z)
     │
     │  ← review the changelog, then squash-merge
     ▼
🚢 Ship  →  gh ship release
     │  tags, creates a draft release
     ▼
📦 Publish Release
     │  cross-compiles six targets, uploads assets + SHA256SUMS
     ▼
the release is made public
```

## Division of labour

**gh-ship orchestrates.** The Release PR, the tag, the draft, dispatching the
publish workflow, making it public.

**git-cliff versions.** The next version comes from the unreleased Conventional
Commits, via `git cliff --bumped-version`. `feat` bumps the minor, a breaking
change bumps the major (`breaking_always_bump_major = true` — this project's
on-disk format and ref layout are contracts).

**The workflows do the work.** Bumping `Cargo.toml`, writing `CHANGELOG.md`,
compiling. gh-ship never edits a source file.

## Preview

```sh
mise run changelog      # git cliff --unreleased
mise run release        # git cliff --bumped-version
```

Neither writes anything.

!!! tip "Set `GITHUB_TOKEN` for these"

    `cliff.toml` declares `[remote.github]`, so git-cliff calls the GitHub API to
    work out who contributed for the first time. Unauthenticated, that is 60
    requests per hour **per IP**, and it does not fail gracefully — git-cliff
    panics trying to parse the rate-limit response as a commit list.

    ```sh
    export GITHUB_TOKEN=$(gh auth token)
    ```

    The token only ever reads public metadata. CI passes the GitHub App's
    installation token for the same reason — runners share IP addresses, so the
    anonymous budget there is permanently spent by other people — and uses the
    App rather than `secrets.GITHUB_TOKEN` because its limit is higher:
    5000/hour against 1000/hour per repository.

## Configuration

| File | Owns |
|---|---|
| `.github/ship.yml` | The Release PR, the release branch, draft mode |
| `cliff.toml` | Changelog format, commit grouping, version bumping |
| `.github/workflows/ship.yaml` | The driver |
| `.github/workflows/prepare-release.yaml` | Version, changelog, release artifact |
| `.github/workflows/publish-release.yaml` | Cross-compilation and assets |

CI validates the setup on every run:

```sh
gh ship validate
```

so a release cannot break at the moment you need it.

## Assets

Six targets, named `git-tpl_<tag>_<platform>`:

| Target | Asset |
|---|---|
| `x86_64-unknown-linux-gnu` | `linux-amd64` |
| `aarch64-unknown-linux-gnu` | `linux-arm64` |
| `x86_64-unknown-linux-musl` | `linux-amd64-musl` |
| `x86_64-apple-darwin` | `darwin-amd64` |
| `aarch64-apple-darwin` | `darwin-arm64` |
| `x86_64-pc-windows-msvc` | `windows-amd64.exe` |

Plus `SHA256SUMS`.

libgit2 is vendored and built from source on every target, so each binary is
self-contained and every platform gets identical Git semantics.

## Requirements

A GitHub App with `APP_CLIENT_ID` (a repository variable) and
`APP_PRIVATE_KEY` (a secret) in the `release` environment. The default
`GITHUB_TOKEN` cannot trigger workflows, so a Release PR it authored would show
no CI results.
