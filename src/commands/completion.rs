//! `git tpl completion`
//!
//! Prints a shell completion script, generated from the same `clap::Command`
//! the parser is built from. A checked-in script would be a second definition
//! of the flags, and the second definition is the one that goes stale.

use std::io;

use clap::CommandFactory;
use tpl::ops::OpError;

use crate::cli::{Cli, CompletionArgs, GlobalArgs};

pub fn run(args: CompletionArgs, _global: &GlobalArgs) -> Result<u8, OpError> {
    // `git-tpl`, not the `bin_name = "git tpl"` the help advertises, and set
    // here rather than left to `generate` so the subcommands inherit it too: a
    // completion script keys off the executable the shell sees on PATH, and one
    // registered for a name with a space in it is one no shell can trigger.
    //
    // `git tpl <TAB>` belongs to Git's own completion, not to this script; see
    // docs/usage/completion.md for the one line that hands it here.
    let mut cmd = Cli::command().bin_name("git-tpl");
    clap_complete::generate(args.shell, &mut cmd, "git-tpl", &mut io::stdout());

    // No `--json` branch: the output *is* the machine format, and wrapping a
    // shell script in a JSON envelope would only mean nobody could source it.
    Ok(crate::exit::SUCCESS)
}
