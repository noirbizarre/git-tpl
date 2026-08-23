# Installation

git-tpl is a single binary with no runtime dependencies.
libgit2 is compiled into it — with its HTTPS and SSH transports, and, on Linux, its own OpenSSL — so there is
nothing to install alongside.

## With Homebrew

```sh
brew install noirbizarre/tap/git-tpl
```

The formula lives in [noirbizarre/homebrew-tap][tap] rather than homebrew-core, and installs the prebuilt release
binary — no Rust toolchain involved.

macOS on Apple silicon and on Intel, and Linux on x86-64.
Homebrew on Linux arm64 is not covered: there is no statically linked aarch64 build yet, and a glibc-linked one
would fail to load on the distributions Homebrew is most used on.

[tap]: https://github.com/noirbizarre/homebrew-tap

## On Arch Linux

Two packages on the [AUR](https://aur.archlinux.org), differing only in whether your machine or GitHub's does the
compiling:

=== "Prebuilt"

    ```sh
    paru -S git-tpl-bin
    ```

    Repackages the release archive for your architecture — no Rust toolchain involved.

=== "Compiled"

    ```sh
    paru -S git-tpl
    ```

    Builds from the tagged source against the `Cargo.lock` this project commits, and runs the test suite before
    packaging.
    For people who would rather not run someone else's binary.

Both cover x86-64 and aarch64, and both install the man page and the bash, zsh and fish completions.
They deliberately conflict — both install `/usr/bin/git-tpl` — so pick one, and your AUR helper will offer to
replace the other if it is already there.

## From source

=== "Compiled"

    ```sh
    cargo install git-tpl
    ```

=== "Prebuilt"

    ```sh
    cargo binstall git-tpl
    ```

    [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) downloads the release archive for your
    platform instead of compiling.
    Falls back to `cargo install` on a target with no published binary.

Neither installs the man page or the shell completions — Cargo only places the binary — so `git tpl --help` will
report *No manual entry for git-tpl* until you generate them:

```sh
git tpl man --out-dir ~/.local/share/man/man1
```

See [Shell completion](../usage/completion.md) for the rest.

## With mise

[mise](https://mise.jdx.dev) installs git-tpl globally or per project, and pins the version in `mise.toml` so
everyone working on the project has the same one.

=== "From crates.io"

    ```sh
    mise use -g cargo:git-tpl
    ```

    Compiled from source, so it works on every platform mise supports — including the ones the release binaries
    do not cover.

=== "From a release binary"

    ```sh
    mise use -g github:noirbizarre/git-tpl
    ```

    No compiler needed, and mise verifies the artifact attestation and the SLSA provenance of the asset it
    downloads.
    Limited to the six published targets.

Either form puts an executable named `git-tpl` on your `PATH` through mise's shims, which is all Git needs to
resolve `git tpl`.

!!! note "The backend prefix is not optional"

    `mise use git-tpl` on its own fails: there is no entry for git-tpl in mise's registry yet, so the tool has to
    be named by its backend — `cargo:` or `github:`.
    Getting the short name is tracked in [issue #25](https://github.com/noirbizarre/git-tpl/issues/25).

## From a release

Download the archive for your platform from the [releases page](https://github.com/noirbizarre/git-tpl/releases),
extract it and put the binary somewhere on your `PATH`.

```sh
VERSION=0.7.0
curl -fsSLO \
  https://github.com/noirbizarre/git-tpl/releases/download/$VERSION/git-tpl_${VERSION}_linux-amd64.tar.gz
tar xzf git-tpl_${VERSION}_linux-amd64.tar.gz
mv git-tpl ~/.local/bin/
```

The archive contains the executable `git-tpl` at its root, already marked executable — nothing to rename and no
`chmod` to remember.
Beside it are `man/man1/` and `completions/`, which you can install or ignore; the man page is what makes
`git tpl --help` work, since Git runs `man git-tpl` for it.

Assets are named `git-tpl_<version>_<platform>.tar.gz`, and `git-tpl_<version>_windows-amd64.zip` on Windows, so
`latest/download/` cannot be used without knowing the version — set `VERSION` to the release you want.

Each release also carries a `SHA256SUMS` file covering the archives:

```sh
curl -fsSLO https://github.com/noirbizarre/git-tpl/releases/download/$VERSION/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
```

!!! warning "Releases before 0.4.0"

    Up to 0.3.0 the assets were bare binaries named `git-tpl_<version>_<platform>`, with no archive and no
    extension.
    Anything pinned to those names needs updating.

## Verify

The binary **must** be named `git-tpl` and be on your `PATH`.
That is how Git resolves subcommands: `git tpl` looks for an executable called `git-tpl`.

```sh
git tpl --version
```

If that works, you are done.
If `git: 'tpl' is not a git command` appears, the binary is either not on your `PATH` or not named `git-tpl`.

!!! tip "Both invocations work"

    `git tpl update` and `git-tpl update` are the same program.
    The Git form is the intended one; the direct form is handy in scripts where you would rather not depend on
    Git's subcommand resolution.

## Development builds

```sh
git clone https://github.com/noirbizarre/git-tpl
cd git-tpl
mise run setup    # cargo install --path . --force
```

See [Development setup](../development/setup.md).
