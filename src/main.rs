//! The `git-tpl` binary.
//!
//! A thin frontend over the `tpl` library. Everything of substance lives
//! there, so that `gh-tpl` — or any other frontend — reuses it rather than
//! reimplementing it.

// The same waiver as the library: several error enums carry miette diagnostic
// payloads, which makes them large. See src/lib.rs.
#![allow(clippy::result_large_err)]

mod cli;
mod commands;
mod exit;
mod prompt;
mod theme;

use std::process::ExitCode;

use clap::Parser;
use miette::Report;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();

    install_diagnostic_hook(&cli);

    let result = match cli.command {
        // Every command returns its own exit code. `status` is the only one
        // that returns anything but SUCCESS, but splitting the authority — the
        // code in `status` and in `main` for the rest — is how the two come to
        // disagree the day a second command grows a code of its own.
        Command::Init(args) => commands::init(args, &cli.global),
        Command::Update(args) => commands::update(args, &cli.global),
        Command::Status(args) => commands::status(args, &cli.global),
        Command::Diff(args) => commands::diff(args, &cli.global),
        Command::Merge(args) => commands::merge(args, &cli.global),
        Command::Fetch(args) => commands::fetch(args, &cli.global),
        Command::Push(args) => commands::push(args, &cli.global),
    };

    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            // `{:?}` is miette's rendered diagnostic — the code, the help, the
            // source snippet — not the derive output. `{}` would print only
            // the one-line message and throw away everything that makes the
            // error useful.
            eprintln!("{:?}", Report::new(error));
            ExitCode::from(exit::FAILURE)
        }
    }
}

/// Configure how diagnostics are rendered.
fn install_diagnostic_hook(cli: &Cli) {
    let colored = theme::Theme::resolve(cli.global.color).is_colored();
    let verbose = cli.global.verbose > 0;

    // Ignored deliberately: the hook can only fail if one is already
    // installed, and miette's default is perfectly serviceable. Refusing to
    // start over terminal formatting would be absurd.
    let _ = miette::set_hook(Box::new(move |_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .color(colored)
                .unicode(colored)
                .context_lines(2)
                // The cause chain is noise in the common case — the diagnostic
                // already carries the actionable part in `help` — but it is
                // exactly what is wanted when something unexpected goes wrong.
                .with_cause_chain()
                .terminal_links(verbose)
                .build(),
        )
    }));
}
