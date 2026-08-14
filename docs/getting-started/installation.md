# Installation

git-tpl is a single binary with no runtime dependencies. libgit2 is compiled
into it, so there is nothing to install alongside.

## From source

```sh
cargo install git-tpl
```

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
