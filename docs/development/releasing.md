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
     │  then publishes to crates.io
     ▼
the release is made public
     │
     ▼
🍺 Homebrew
     │  renders the formula and pushes it to noirbizarre/homebrew-tap
```

## Division of labour

**gh-ship orchestrates.** The Release PR, the tag, the draft, dispatching the
publish workflow, making it public.

**git-cliff versions.** The next version comes from the unreleased Conventional
Commits, via `git cliff --bumped-version`. `feat` bumps the minor
(`features_always_bump_minor = true`).

Before 1.0, a breaking change bumps the *minor* too
(`breaking_always_bump_major = false`), so `0.1.x` → `0.2.0` is how a break is
signalled. This project's on-disk format and ref layout are contracts, and once
1.0 is out that setting flips and a break bumps the major.

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
| `.github/workflows/homebrew.yaml` | The Homebrew tap formula |

CI validates the setup on every run:

```sh
gh ship validate
```

so a release cannot break at the moment you need it.

## Assets

Six targets, named `git-tpl_<tag>_<platform>.<ext>`:

| Target | Asset |
|---|---|
| `x86_64-unknown-linux-gnu` | `linux-amd64.tar.gz` |
| `aarch64-unknown-linux-gnu` | `linux-arm64.tar.gz` |
| `x86_64-unknown-linux-musl` | `linux-amd64-musl.tar.gz` |
| `x86_64-apple-darwin` | `darwin-amd64.tar.gz` |
| `aarch64-apple-darwin` | `darwin-arm64.tar.gz` |
| `x86_64-pc-windows-msvc` | `windows-amd64.zip` |

Plus `SHA256SUMS`, which covers the archives.

Each archive holds one entry at its root: `git-tpl`, or `git-tpl.exe` on
Windows. That plain name is the point of archiving at all — a bare versioned
asset forces every packaging format that consumes a release to rename the file
it downloaded, and Git only resolves `git tpl` through an executable called
exactly `git-tpl`.

libgit2 is vendored and built from source on every target, so each binary is
self-contained and every platform gets identical Git semantics.

## crates.io

The `crates` job publishes with [Trusted
Publishing](https://crates.io/docs/trusted-publishing): the workflow's OIDC
identity is exchanged for a token that lives 30 minutes and is revoked when the
job ends. There is no API key stored in this repository.

It runs **last**, after the binaries have built on all six targets *and* been
attached to the release. That ordering is deliberate: publishing to crates.io
cannot be undone. `cargo yank` hides a version from resolution but never removes
it, and the version number is spent permanently. If the job fails, re-running it
costs nothing; the reverse ordering could leave a permanent release on crates.io
with no binaries behind it.

Re-running is safe. The job checks whether the version is already on crates.io
and skips if it is, so a re-run after a partial failure is a no-op rather than
an `already exists` error — which would otherwise leave the GitHub release stuck
as a draft, because gh-ship only undrafts when this workflow succeeds.

### Configuration

Set once, under crates.io → git-tpl → Settings → Trusted Publishing:

| Field | Value |
|---|---|
| Repository owner | `noirbizarre` |
| Repository name | `git-tpl` |
| Workflow filename | `publish-release.yaml` |
| Environment | `release` |

crates.io validates the OIDC claim against every one of these. Renaming the
workflow file, or changing the job's `environment:`, breaks publishing with a
`403` at the worst possible moment — so if either changes here, change it there
too.

!!! note "The first publish was manual"

    crates.io has no pending-publisher concept: a trusted publisher can only be
    configured on a crate that already exists, and the publish endpoint refuses
    to create new crates. 0.1.0 was therefore published by hand with an API
    token. Every version since goes through this job.

## Homebrew

The `git-tpl` formula in
[noirbizarre/homebrew-tap](https://github.com/noirbizarre/homebrew-tap) is
generated, never hand-edited. `packaging/homebrew/git-tpl.rb` is the template;
`@VERSION@` and the three `@SHA256_*@` placeholders are substituted by
`.github/workflows/homebrew.yaml`, which then commits the result to the tap.

It triggers on `release: published`, not on the publish workflow finishing.
gh-ship only undrafts the release once 📦 Publish Release has succeeded, so
`published` is the first moment at which the download URLs baked into the
formula actually resolve. Publishing earlier would put a formula in the tap
that 404s.

The checksums are computed from the downloaded assets rather than read out of
`SHA256SUMS`, so a disagreement between the two cannot reach users — and an
unsubstituted placeholder fails the job, because a formula that kept one would
install nothing.

Re-running is safe. The push compares the staged formula against the tap's and
exits without committing when they match, so a re-run for a tag already in the
tap is a no-op rather than an empty commit.

A tap only covers macOS arm64 and Intel, and Linux x86-64 — the platforms with
an asset the formula can trust. homebrew-core is not a target yet: it has an
acceptance bar around notability and release history that this project does not
clear, and submitting early spends a reviewer's afternoon for nothing.

## Requirements

A GitHub App with `APP_CLIENT_ID` (a repository variable) and
`APP_PRIVATE_KEY` (a secret) in the `release` environment. The default
`GITHUB_TOKEN` cannot trigger workflows, so a Release PR it authored would show
no CI results.

A `TAP_TOKEN` secret in the `homebrew` environment: a fine-grained personal
access token whose repository access is limited to `noirbizarre/homebrew-tap`
with `Contents: Read and write`. It is scoped to its own environment rather
than sharing `release`, so nothing that publishes a release can also rewrite
the tap.
