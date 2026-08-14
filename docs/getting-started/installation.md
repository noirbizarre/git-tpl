# Installation

git-tpl is a single binary with no runtime dependencies. libgit2 is compiled
into it, so there is nothing to install alongside.

## From source

```sh
cargo install git-tpl
```

## With mise

[mise](https://mise.jdx.dev) installs git-tpl globally or per project, and pins
the version in `mise.toml` so everyone working on the project has the same one.

=== "From crates.io"

    ```sh
    mise use -g cargo:git-tpl
    ```

    Compiled from source, so it works on every platform mise supports —
    including the ones the release binaries do not cover.

=== "From a release binary"

    ```sh
    mise use -g github:noirbizarre/git-tpl
    ```

    No compiler needed, and mise verifies the artifact attestation and the SLSA
    provenance of the asset it downloads. Limited to the six published targets.

Either form puts an executable named `git-tpl` on your `PATH` through mise's
shims, which is all Git needs to resolve `git tpl`.

!!! note "The backend prefix is not optional"

    `mise use git-tpl` on its own fails: there is no entry for git-tpl in mise's
    registry yet, so the tool has to be named by its backend — `cargo:` or
    `github:`. Getting the short name is tracked in
    [PLAN.md](https://github.com/noirbizarre/git-tpl/blob/main/PLAN.md).

## From a release

Download the binary for your platform from the
[releases page](https://github.com/noirbizarre/git-tpl/releases) and put it
somewhere on your `PATH`.

```sh
curl -fsSL -o git-tpl \
  https://github.com/noirbizarre/git-tpl/releases/latest/download/git-tpl_0.1.0_linux-amd64
chmod +x git-tpl
mv git-tpl ~/.local/bin/
```

Each release also carries a `SHA256SUMS` file.

## Verify

The binary **must** be named `git-tpl` and be on your `PATH`. That is how Git
resolves subcommands: `git tpl` looks for an executable called `git-tpl`.

```sh
git tpl --version
```

If that works, you are done. If `git: 'tpl' is not a git command` appears, the
binary is either not on your `PATH` or not named `git-tpl`.

!!! tip "Both invocations work"

    `git tpl update` and `git-tpl update` are the same program. The Git form is
    the intended one; the direct form is handy in scripts where you would rather
    not depend on Git's subcommand resolution.

## Development builds

```sh
git clone https://github.com/noirbizarre/git-tpl
cd git-tpl
mise run setup    # cargo install --path . --force
```

See [Development setup](../development/setup.md).
