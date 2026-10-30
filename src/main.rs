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
mod report;
mod theme;

use std::process::ExitCode;

use clap::{Arg, ArgAction, CommandFactory, FromArgMatches};
use miette::Report;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = parse_cli();

    install_diagnostic_hook(&cli);

    let result = match cli.command {
        // Every command returns its own exit code. `status` is the only one
        // that returns anything but SUCCESS, but splitting the authority — the
        // code in `status` and in `main` for the rest — is how the two come to
        // disagree the day a second command grows a code of its own.
        Command::Init(args) => commands::init(args, &cli.global),
        Command::Update(args) => commands::update(args, &cli.global),
        Command::Render(args) => commands::render(args, &cli.global),
        Command::Lint(args) => commands::lint(args, &cli.global),
        Command::Test(args) => commands::test(args, &cli.global),
        Command::Questions(args) => commands::questions(args, &cli.global),
        Command::Context(args) => commands::context(args, &cli.global),
        Command::Status(args) => commands::status(args, &cli.global),
        Command::Log(args) => commands::log(args, &cli.global),
        Command::Diff(args) => commands::diff(args, &cli.global),
        Command::Show(args) => commands::show(args, &cli.global),
        Command::Merge(args) => commands::merge(args, &cli.global),
        Command::Backport(args) => commands::backport(args, &cli.global),
        Command::Fetch(args) => commands::fetch(args, &cli.global),
        Command::Push(args) => commands::push(args, &cli.global),
        Command::Completion(args) => commands::completion(args, &cli.global),
        Command::Man(args) => commands::man(args, &cli.global),
    };

    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if cli.global.json {
                // Stdout, and still a non-zero exit. A caller reads the code
                // to branch and the exit status to fail; neither substitutes
                // for the other.
                println!("{}", report::error(&error));
            } else {
                // `{:?}` is miette's rendered diagnostic — the code, the help,
                // the source snippet — not the derive output. `{}` would print
                // only the one-line message and throw away everything that
                // makes the error useful.
                eprintln!("{:?}", Report::new(error));
            }
            ExitCode::from(exit::FAILURE)
        }
    }
}

/// Resolve presentation before clap's help and argument errors can exit.
fn parse_cli() -> Cli {
    let argv: Vec<_> = std::env::args_os().collect();
    // Probe the actual grammar rather than scanning argv: option values and
    // tokens after `--` must never become accidental color instructions.
    // Help/version actions must be inert because ignore_errors only suppresses
    // failures, not the successful early exits used to display help and version.
    let probe = Cli::command()
        .ignore_errors(true)
        .disable_help_subcommand(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("probe_help")
                .short('h')
                .long("help")
                .global(true)
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("probe_version")
                .short('V')
                .long("version")
                .action(ArgAction::Count),
        )
        .get_matches_from(&argv);
    // The probe can stop at invalid input (including the synthetic `help`
    // path). Only the fresh strict parser below decides validity and exits.
    let choice = probe
        .get_one::<cli::ColorChoice>("color")
        .copied()
        .unwrap_or(cli::ColorChoice::Auto);
    let color = if theme::Theme::resolve(choice).is_colored() {
        clap::ColorChoice::Always
    } else {
        clap::ColorChoice::Never
    };
    let mut matches = Cli::command().color(color).get_matches_from(argv);
    Cli::from_arg_matches_mut(&mut matches).unwrap_or_else(|error| error.exit())
}

/// Configure how diagnostics are rendered.
fn install_diagnostic_hook(cli: &Cli) {
    // Under `--json` nothing reaches miette's renderer, so installing a hook
    // would only decide the appearance of output that is never produced.
    if cli.global.json {
        return;
    }

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
