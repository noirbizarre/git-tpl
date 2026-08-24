//! `git tpl show`

use std::io::{self, Write};

use tpl::ops::{self, OpError, Shown};

use super::{Session, answering, report_ignored_paths, supplied, trust};
use crate::cli::{GlobalArgs, ShowArgs};
use crate::prompt::{Confirmer, Interactive};
use crate::theme::warning;

pub fn run(args: ShowArgs, global: &GlobalArgs) -> Result<u8, OpError> {
    let ctx = Session::discover(global)?;

    // `--dirty` renders the template's working tree into a commit no ref
    // points at, so an author can read a file out of an uncommitted edit
    // without committing it first.
    let against = if args.dirty {
        let preferences = tpl::gitconfig::Preferences::load(&ctx.repo)?;
        let mut prompter = Interactive;
        let mut confirmer = Confirmer;
        let preview = ops::render_preview(
            &ctx.repo,
            &ctx.root,
            supplied(&args.answers)?,
            true,
            args.answers.strict_answers,
            &ctx.user,
            answering(&args.answers, preferences.interactive, &mut prompter),
            trust(
                &args.answers,
                false,
                preferences.interactive,
                &mut confirmer,
            ),
        )?;
        // stderr, so the file bytes on stdout stay exactly the file bytes.
        report_ignored_paths(&ctx.out, &preview.ignored);
        Some(preview.commit)
    } else {
        None
    };

    match ops::show(&ctx.repo, &ctx.root, &args.path, against)? {
        // Bytes, verbatim, to stdout: `git tpl show README.md > mine.md` and
        // piping into an editor must both work, and rendered content is not
        // necessarily UTF-8 — so `write_all`, never `print!`.
        Shown::File(bytes) => write_out(&ctx.out, &bytes),
        // One root-relative path per line, the same shape as
        // `git tpl diff --name-only`, and for the same reason: `| xargs`.
        // Line-terminated rather than line-separated, so the last path is not
        // a partial line to whatever reads this next.
        Shown::Directory(paths) => {
            let mut listing = String::new();
            for path in &paths {
                listing.push_str(path);
                listing.push('\n');
            }
            write_out(&ctx.out, listing.as_bytes());
        }
    }

    Ok(crate::exit::SUCCESS)
}

/// Write to stdout, tolerating a closed pipe.
///
/// `git tpl show big-file | head` closes the pipe under us, and a diagnostic
/// about it would be noise about something the user did on purpose.
///
/// Through the [`Reporter`](super::Reporter) rather than a bare `eprintln!`:
/// this was the one command that hand-rolled its own `error: ` prefix, which
/// matched neither miette's rendering at the binary edge nor `warn`'s, and it
/// is a warning — the run still succeeds — so `warn` is what carries it past
/// `--quiet`.
fn write_out(out: &super::Reporter, bytes: &[u8]) {
    let stdout = io::stdout();
    let mut stream = stdout.lock();
    if let Err(error) = stream.write_all(bytes)
        && error.kind() != io::ErrorKind::BrokenPipe
    {
        out.warn(warning(
            &out.theme,
            &format!("could not write to stdout: {error}"),
        ));
    }
    let _ = stream.flush();
}
