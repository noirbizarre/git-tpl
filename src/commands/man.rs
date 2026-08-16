//! `git tpl man`
//!
//! Writes the man pages, in troff, from the live `clap::Command`.
//!
//! This is not a convenience. Git intercepts `--help` for any subcommand and
//! execs `man git-tpl`; with no page installed the user's first `git tpl
//! --help` fails with "No manual entry for git-tpl" and exit 16. The pages this
//! writes are what make the project's flagship invocation work.

use std::fs;
use std::io;
use std::path::Path;

use clap::CommandFactory;
use clap_mangen::Man;
use tpl::ops::OpError;

use crate::cli::{Cli, GlobalArgs, ManArgs};

/// The section every command-line tool belongs in.
const SECTION: &str = "1";

pub fn run(args: ManArgs, _global: &GlobalArgs) -> Result<u8, OpError> {
    // `display_name` overrides `bin_name = "git tpl"` for the page title. Left
    // alone, the page would be titled `git tpl` and `man git-tpl` — the exact
    // command Git runs — would never match it, which is the entire failure this
    // module exists to fix.
    let mut cmd = Cli::command().display_name("git-tpl");
    // Propagates globals and defaults into the subcommands before they are
    // read, so a per-command page documents the flags that command truly takes.
    cmd.build();

    match args.out_dir.as_deref() {
        None => render(&cmd, &mut io::stdout()).map_err(|e| failed("<stdout>", &e))?,
        Some(dir) => write_pages(&cmd, dir)?,
    }

    Ok(crate::exit::SUCCESS)
}

/// Write `git-tpl.1` plus one page per visible subcommand into `dir`.
fn write_pages(cmd: &clap::Command, dir: &Path) -> Result<(), OpError> {
    fs::create_dir_all(dir).map_err(|e| failed(&dir.display().to_string(), &e))?;

    write_page(cmd, dir, "git-tpl")?;

    for sub in cmd.get_subcommands() {
        // A hidden command is hidden everywhere. `git tpl man` documenting
        // itself in a page shipped to every user would undo the decision to
        // hide it from `--help`.
        if sub.is_hide_set() {
            continue;
        }

        let name = format!("git-tpl-{}", sub.get_name());
        // Renamed rather than titled: the name is what clap_mangen puts in both
        // the `.TH` line and the NAME section, and `man git-tpl-init` has to
        // find them agreeing or the page is unreachable by its own title.
        //
        // The version is restated because a subcommand carries none of its own,
        // and a page footer reading `git-tpl-init` with no release beside it
        // tells a reader nothing about which one they are looking at.
        let page = sub
            .clone()
            .name(name.clone())
            .display_name(name.clone())
            .version(env!("CARGO_PKG_VERSION"));
        write_page(&page, dir, &name)?;
    }

    Ok(())
}

fn write_page(cmd: &clap::Command, dir: &Path, name: &str) -> Result<(), OpError> {
    let path = dir.join(format!("{name}.{SECTION}"));
    let mut file = fs::File::create(&path).map_err(|e| failed(&path.display().to_string(), &e))?;
    render(cmd, &mut file).map_err(|e| failed(&path.display().to_string(), &e))
}

fn render(cmd: &clap::Command, to: &mut dyn io::Write) -> io::Result<()> {
    Man::new(cmd.clone()).section(SECTION).render(to)
}

fn failed(path: &str, error: &io::Error) -> OpError {
    OpError::WriteFailed {
        path: path.to_string(),
        reason: error.to_string(),
    }
}
