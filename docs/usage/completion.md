# `git tpl completion`

Print a shell completion script.

```sh
git tpl completion <bash|zsh|fish|elvish|powershell>
```

The script is generated from the same command definition the parser is built
from, so it can never offer a flag the binary does not accept.

## Installing it

=== "bash"

    ```sh
    git tpl completion bash > ~/.local/share/bash-completion/completions/git-tpl
    ```

=== "zsh"

    ```sh
    git tpl completion zsh > ~/.local/share/zsh/site-functions/_git-tpl
    ```

    The leading underscore is not decoration — it is how zsh finds the file. The
    directory must be on your `fpath`.

=== "fish"

    ```sh
    git tpl completion fish > ~/.config/fish/completions/git-tpl.fish
    ```

=== "elvish"

    ```sh
    git tpl completion elvish > ~/.config/elvish/lib/git-tpl.elv
    ```

    Then `use git-tpl` from `rc.elv`.

=== "powershell"

    ```powershell
    git tpl completion powershell | Out-String | Invoke-Expression
    ```

    Add that line to your `$PROFILE` to make it permanent.

If you installed git-tpl with Homebrew or from the AUR, the completions are
already in place — the packages install them for you.

## `git tpl <TAB>` versus `git-tpl <TAB>`

The generated script completes `git-tpl`, the executable your shell sees on
`PATH`. It does not complete `git tpl`, because that first word is `git`, and
what happens after it belongs to Git's own completion.

`git-tpl <TAB>` therefore works as soon as the script is installed.
`git tpl <TAB>` needs one line telling Git's completion to defer to it:

=== "bash"

    ```bash
    _git_tpl() { _git__tpl; }
    ```

    Git's bash completion looks for a function named after the subcommand. This
    hands it the one the generated script defines.

=== "zsh"

    Nothing to do when the script is on your `fpath`: zsh's Git completion falls
    back to `_git-tpl` for an unknown subcommand.

Both spellings run the same program either way; only the completion differs.

## The man page

The companion to this is the man page, which is what makes `git tpl --help`
work at all — Git intercepts `--help` for a subcommand and runs `man git-tpl`.
The packages install it. If you installed with `cargo install`, generate it
yourself:

```sh
git tpl man --out-dir ~/.local/share/man/man1
```

Without it, `git tpl --help` fails with *No manual entry for git-tpl*. Use
`git-tpl --help` in the meantime, which Git never intercepts.
