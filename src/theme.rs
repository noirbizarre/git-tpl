//! Terminal presentation.
//!
//! Every function here returns a `String` rather than printing. That keeps the
//! formatting testable without capturing stdout, and keeps the decision about
//! *where* output goes with the caller — human output goes to stderr, so
//! `--format json` leaves stdout machine-readable.

use console::Style;

use crate::cli::ColorChoice;

/// The styles used across the CLI.
#[derive(Debug, Clone)]
pub struct Theme {
    colored: bool,
    pub heading: Style,
    pub muted: Style,
    pub added: Style,
    pub modified: Style,
    pub deleted: Style,
    pub warning: Style,
    pub command: Style,
}

impl Theme {
    /// No colour at all.
    pub fn plain() -> Self {
        Self {
            colored: false,
            heading: Style::new(),
            muted: Style::new(),
            added: Style::new(),
            modified: Style::new(),
            deleted: Style::new(),
            warning: Style::new(),
            command: Style::new(),
        }
    }

    /// Colour, using Git's own vocabulary — green added, red deleted — so the
    /// output reads like the Git output it sits beside.
    pub fn colored() -> Self {
        Self {
            colored: true,
            heading: Style::new().cyan().bold(),
            muted: Style::new().dim(),
            added: Style::new().green(),
            modified: Style::new().yellow(),
            deleted: Style::new().red(),
            warning: Style::new().yellow(),
            command: Style::new().cyan(),
        }
    }

    /// Decide from the environment.
    pub fn resolve(choice: ColorChoice) -> Self {
        match choice {
            ColorChoice::Always => Self::colored(),
            ColorChoice::Never => Self::plain(),
            ColorChoice::Auto => {
                if decide(
                    std::env::var("NO_COLOR").ok().as_deref(),
                    std::env::var("CLICOLOR_FORCE").ok().as_deref(),
                    std::env::var("TERM").ok().as_deref(),
                    console::user_attended_stderr(),
                ) {
                    Self::colored()
                } else {
                    Self::plain()
                }
            }
        }
    }

    /// Whether this theme colourises, for callers that must decide separately —
    /// miette's handler takes its own colour flag.
    pub fn is_colored(&self) -> bool {
        self.colored
    }
}

/// Whether to colourise, given the environment.
///
/// Pure, so the precedence can be tested without setting process-wide
/// environment variables — which is both racy under a parallel test runner and
/// impossible to assert cleanly.
pub fn decide(
    no_color: Option<&str>,
    clicolor_force: Option<&str>,
    term: Option<&str>,
    is_terminal: bool,
) -> bool {
    // https://force-color.org — an explicit request wins over everything,
    // including not being a terminal, because that is what CI systems set.
    if clicolor_force.is_some_and(|v| v != "0") {
        return true;
    }
    // https://no-color.org — presence is the signal, whatever the value.
    if no_color.is_some() {
        return false;
    }
    if term == Some("dumb") {
        return false;
    }
    is_terminal
}

// --- line helpers -----------------------------------------------------------

/// A `key: value` line, aligned to a common column.
pub fn field(theme: &Theme, key: &str, value: &str) -> String {
    format!("{:<10} {value}", theme.muted.apply_to(format!("{key}:")))
}

/// One entry in a change list: `  added     path`.
pub fn change(theme: &Theme, kind: tpl::git::ChangeKind, path: &str) -> String {
    let style = match kind {
        tpl::git::ChangeKind::Added => &theme.added,
        tpl::git::ChangeKind::Modified => &theme.modified,
        tpl::git::ChangeKind::Deleted => &theme.deleted,
    };
    format!("  {}  {path}", style.apply_to(kind.label()))
}

/// A suggested command, indented.
pub fn command(theme: &Theme, text: &str) -> String {
    format!("  {}", theme.command.apply_to(text))
}

/// A section heading.
pub fn heading(theme: &Theme, text: &str) -> String {
    theme.heading.apply_to(text).to_string()
}

/// A headline: an emphasised verb and the thing it happened to.
///
/// `Created refs/tpl/x`, `Updated refs/tpl/x`, `Merged refs/tpl/x into ...`.
/// Five call sites were composing the same `"{} {}"` by hand around
/// [`heading`], which is one shape too many to leave to each of them.
pub fn headline(theme: &Theme, verb: &str, subject: &str) -> String {
    format!("{} {subject}", heading(theme, verb))
}

/// A dimmed note.
pub fn muted(theme: &Theme, text: &str) -> String {
    theme.muted.apply_to(text).to_string()
}

/// A warning line.
pub fn warning(theme: &Theme, text: &str) -> String {
    format!("{} {text}", theme.warning.apply_to("warning:"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // An explicit request wins over not being a terminal — that is exactly the
    // case CI systems set it for.
    #[case(None, Some("1"), None, false, true)]
    // NO_COLOR wins over being a terminal, whatever its value.
    #[case(Some(""), None, None, true, false)]
    #[case(Some("1"), None, None, true, false)]
    // ...but not over an explicit force.
    #[case(Some("1"), Some("1"), None, false, true)]
    #[case(None, None, Some("dumb"), true, false)]
    #[case(None, None, Some("xterm"), true, true)]
    #[case(None, None, Some("xterm"), false, false)]
    fn colour_precedence_is_explicit_request_then_refusal_then_terminal(
        #[case] no_color: Option<&str>,
        #[case] force: Option<&str>,
        #[case] term: Option<&str>,
        #[case] is_terminal: bool,
        #[case] expected: bool,
    ) {
        assert_eq!(decide(no_color, force, term, is_terminal), expected);
    }

    #[test]
    fn a_plain_theme_adds_no_escape_sequences() {
        let theme = Theme::plain();
        let line = change(&theme, tpl::git::ChangeKind::Added, "Cargo.toml");
        assert_eq!(line, "  added     Cargo.toml");
        assert!(!line.contains('\x1b'));
    }

    /// The labels are padded so that paths line up; a change to one label's
    /// width would silently ragged the whole list.
    #[test]
    fn change_lines_align_their_paths() {
        let theme = Theme::plain();
        let lines = [
            change(&theme, tpl::git::ChangeKind::Added, "a"),
            change(&theme, tpl::git::ChangeKind::Modified, "b"),
            change(&theme, tpl::git::ChangeKind::Deleted, "c"),
        ];
        let columns: Vec<_> = lines.iter().map(|l| l.rfind(' ').unwrap()).collect();
        assert!(columns.windows(2).all(|w| w[0] == w[1]), "{lines:?}");
    }

    #[test]
    fn fields_align_their_values() {
        let theme = Theme::plain();
        let lines = [
            field(&theme, "Template", "x"),
            field(&theme, "Ref", "y"),
            field(&theme, "Worktree", "z"),
        ];
        let columns: Vec<_> = lines.iter().map(|l| l.rfind(' ').unwrap()).collect();
        assert!(columns.windows(2).all(|w| w[0] == w[1]), "{lines:?}");
    }
}
