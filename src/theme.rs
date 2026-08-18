//! Terminal presentation.
//!
//! Every function here returns a `String` rather than printing. That keeps the
//! formatting testable without capturing stdout, and keeps the decision about
//! *where* output goes with the caller — human output goes to stderr, so
//! `--format json` leaves stdout machine-readable.

use console::Style;

use tpl::ops::{Proposal, Selection};

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

/// One entry in a diffstat: `  modified  README.md   +9  -3`.
///
/// `path_width` is the longest path in the list being printed. The helper
/// cannot know it — it sees one line — and without it the count columns are
/// ragged, which is the whole reason a diffstat is read as a column.
pub fn change_stat(theme: &Theme, stat: &tpl::git::FileStat, path_width: usize) -> String {
    let head = change(theme, stat.kind, &format!("{:<path_width$}", stat.path));
    if stat.binary {
        // No counts rather than two zeroes: `+0 -0` reads as "nothing changed",
        // which for a replaced image is exactly wrong.
        return format!("{head}  {}", muted(theme, "Bin"));
    }
    format!(
        "{head}  {:>5} {:>5}",
        theme.added.apply_to(format!("+{}", stat.insertions)),
        theme.deleted.apply_to(format!("-{}", stat.deletions)),
    )
}

/// `3 files changed, 57 insertions(+), 15 deletions(-)`.
///
/// Git's own wording, singular where Git is singular, and a zero term omitted
/// rather than printed — this line is read beside `git diff --stat`'s, and a
/// difference in it would read as a difference in the numbers.
pub fn diff_summary(files: usize, insertions: usize, deletions: usize) -> String {
    let mut parts = vec![format!(
        "{files} {} changed",
        if files == 1 { "file" } else { "files" }
    )];
    if insertions > 0 {
        parts.push(format!(
            "{insertions} {}(+)",
            if insertions == 1 {
                "insertion"
            } else {
                "insertions"
            }
        ));
    }
    if deletions > 0 {
        parts.push(format!(
            "{deletions} {}(-)",
            if deletions == 1 {
                "deletion"
            } else {
                "deletions"
            }
        ));
    }
    parts.join(", ")
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

/// A revision transition: `from → to`, with an optional muted note.
///
/// One producer, because there were two and they had already diverged —
/// `status` appended a muted "template has moved" and `update` did not. The
/// arrow, its spacing and the note's styling are one decision, and the two
/// ends of the line are produced by `ops::describe_revision` for the same
/// reason.
pub fn transition(theme: &Theme, from: &str, to: &str, note: Option<&str>) -> String {
    match note {
        Some(note) => format!("{from} → {to}   {}", muted(theme, note)),
        None => format!("{from} → {to}"),
    }
}

/// A proposed un-substitution, laid out for the person who has to judge it.
///
/// Here rather than in `prompt.rs` for the reason at the top of this file: the
/// layout is the part worth testing, and a `demand` prompt cannot be driven
/// without a terminal. What is left at the prompt is the question itself.
///
/// Three texts, because two of them do not determine the third — the same edit
/// has several placements that all render back correctly, and the one chosen is
/// what is being consented to. See ADR-022.
pub fn reversal(theme: &Theme, proposal: &Proposal<'_>) -> String {
    let Proposal {
        path,
        template_path,
        line,
        rendered,
        project,
        patched,
        expressions,
    } = proposal;
    // Named, because the placeholders are the thing being kept — and the reason
    // the user is asked rather than told. If they meant to change what is
    // *inside* one, they meant to change their answer, not the template.
    let kept = expressions
        .iter()
        .map(|expression| format!("`{expression}`"))
        .collect::<Vec<_>>()
        .join(" and ");

    format!(
        "\n{}\n\n  rendered  {rendered}\n  yours     {project}\n  upstream  {patched}\n\n{}\n",
        format_args!("`{path}` line {line} was changed around a value the template substitutes."),
        muted(
            theme,
            &format!("It keeps {kept} and sends the rest of the line to `{template_path}`."),
        ),
    )
}

/// One file's hunks, laid out for the person choosing between them.
///
/// Here rather than in `prompt.rs` for the same reason as [`reversal`]: the
/// layout is the part worth testing, and a `demand` prompt cannot be driven
/// without a terminal.
///
/// Coloured in Git's vocabulary — green added, red deleted — because this is
/// the one place git-tpl shows a diff of the user's own working tree, and it
/// should read like the `git diff` they have just run. The `@@` header is
/// numbered, so what the picker lists below it (`1`, `2`, …) names something
/// visible here rather than something the user has to count.
pub fn hunks(theme: &Theme, selection: &Selection<'_>) -> String {
    let mut out = format!(
        "\n{}\n",
        heading(
            theme,
            &format!("{} → {}", selection.path, selection.template_path)
        )
    );

    for hunk in selection.hunks {
        out.push_str(&format!(
            "\n{}\n",
            theme
                .heading
                .apply_to(format!("  {}  {}", hunk.index + 1, hunk.header))
        ));
        for line in &hunk.lines {
            let styled = match line.as_bytes().first() {
                Some(b'+') => theme.added.apply_to(line).to_string(),
                Some(b'-') => theme.deleted.apply_to(line).to_string(),
                _ => muted(theme, line),
            };
            out.push_str(&format!("    {styled}\n"));
        }
    }
    out
}

/// A dimmed note.
pub fn muted(theme: &Theme, text: &str) -> String {
    theme.muted.apply_to(text).to_string()
}

/// A warning line.
pub fn warning(theme: &Theme, text: &str) -> String {
    format!("{} {text}", theme.warning.apply_to("warning:"))
}

/// The template's own words, framed and attributed to it.
///
/// The frame is load-bearing, not decoration. A note is untrusted text from a
/// template repository, and the mechanical half of the risk — escape sequences
/// — is handled by [`tpl::note::sanitise`], which every caller must have
/// applied before reaching here. What is left is the social half: an unframed
/// line reading `Run: curl … | sh` in git-tpl's own [`command`] styling would
/// appear to be git-tpl's own advice. The rule stays true whatever the note
/// says. See ADR-019.
///
/// Returns the whole block, lines included, because the frame's width depends
/// on the content and a caller printing line by line could not know it.
pub fn note_block(theme: &Theme, sanitised: &str) -> String {
    const LABEL: &str = " from the template ";
    // Wide enough for the label and for what is inside it, and capped so that a
    // long line does not draw a rule off the edge of a narrow terminal.
    let width = sanitised
        .lines()
        .map(|line| console::measure_text_width(line) + 2)
        .chain(std::iter::once(LABEL.len() + 4))
        .max()
        .unwrap_or(LABEL.len() + 4)
        .min(76);

    let mut out = String::new();
    out.push_str(&muted(
        theme,
        &format!(
            "──{LABEL}{}",
            "─".repeat(width.saturating_sub(LABEL.len() + 2))
        ),
    ));
    out.push('\n');
    for line in sanitised.lines() {
        // Indented, so that even a line the template styled to look like a
        // git-tpl heading sits visibly inside the frame rather than beside it.
        out.push_str(&format!("  {line}\n"));
    }
    out.push_str(&muted(theme, &"─".repeat(width)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// The three texts, in a fixed order, each labelled.
    ///
    /// The layout is the thing being judged — a user deciding whether to send a
    /// reversed substitution upstream is comparing these three lines and
    /// nothing else — so it is pinned here rather than left to a prompt no test
    /// can drive.
    /// The hunks are numbered from 1, because the picker's list is, and a user
    /// matching "2" in the list to "2" above it should not have to count.
    #[test]
    fn hunks_are_numbered_from_one_and_show_their_body() {
        let listed = [tpl::ops::Hunk {
            index: 1,
            header: "@@ -4,6 +4,7 @@".to_string(),
            lines: vec![
                " context".to_string(),
                "-was".to_string(),
                "+is".to_string(),
            ],
            rendered_lines: 3..9,
            insertions: 1,
            deletions: 1,
        }];
        let shown = hunks(
            &Theme::plain(),
            &Selection {
                path: "README.md",
                template_path: "README.md.jinja",
                hunks: &listed,
            },
        );

        assert!(shown.contains("README.md → README.md.jinja"), "{shown}");
        assert!(shown.contains("  2  @@ -4,6 +4,7 @@"), "{shown}");
        assert!(shown.contains("    -was\n"), "{shown}");
        assert!(shown.contains("    +is\n"), "{shown}");
    }

    #[test]
    fn a_reversal_shows_what_was_rendered_what_you_have_and_what_would_be_sent() {
        let kept = ["{{ project_name }}".to_string()];
        let shown = reversal(
            &Theme::plain(),
            &Proposal {
                path: "README.md",
                template_path: "README.md.jinja",
                line: 1,
                rendered: "# acme — a service",
                project: "# acme — a web service",
                patched: "# {{ project_name }} — a web service",
                expressions: &kept,
            },
        );

        assert!(
            shown.contains(
                "`README.md` line 1 was changed around a value the template substitutes."
            ),
            "{shown}"
        );
        assert!(
            shown.contains("  rendered  # acme — a service\n"),
            "{shown}"
        );
        assert!(
            shown.contains("  yours     # acme — a web service\n"),
            "{shown}"
        );
        assert!(
            shown.contains("  upstream  # {{ project_name }} — a web service\n"),
            "{shown}"
        );
        assert!(
            shown.contains(
                "It keeps `{{ project_name }}` and sends the rest of the line to `README.md.jinja`."
            ),
            "{shown}"
        );
    }

    /// Every placeholder is named, not just the first: the user is consenting
    /// to keeping all of them.
    #[test]
    fn a_reversal_names_every_placeholder_it_keeps() {
        let kept = ["{{ tool }}".to_string(), "{{ task }}".to_string()];
        let shown = reversal(
            &Theme::plain(),
            &Proposal {
                path: "run.sh",
                template_path: "run.sh.jinja",
                line: 3,
                rendered: "cargo run test",
                project: "cargo run --release test",
                patched: "{{ tool }} run --release {{ task }}",
                expressions: &kept,
            },
        );
        assert!(
            shown.contains("It keeps `{{ tool }}` and `{{ task }}`"),
            "{shown}"
        );
    }

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

    fn stat(kind: tpl::git::ChangeKind, path: &str, ins: usize, del: usize) -> tpl::git::FileStat {
        tpl::git::FileStat {
            kind,
            path: path.to_string(),
            insertions: ins,
            deletions: del,
            binary: false,
        }
    }

    /// A diffstat is read as a column. Paths of different lengths must not
    /// stagger the counts beside them.
    #[test]
    fn diffstat_lines_align_their_counts() {
        let theme = Theme::plain();
        let stats = [
            stat(tpl::git::ChangeKind::Added, "a", 1, 0),
            stat(
                tpl::git::ChangeKind::Modified,
                "a/much/longer/path",
                20,
                300,
            ),
            stat(tpl::git::ChangeKind::Deleted, "mid.rs", 0, 4),
        ];
        let width = stats.iter().map(|s| s.path.len()).max().unwrap();
        let lines: Vec<_> = stats
            .iter()
            .map(|s| change_stat(&theme, s, width))
            .collect();
        // Right-aligned counts, so every line ends in the same column whatever
        // the path's length or the count's magnitude.
        let widths: Vec<_> = lines.iter().map(|l| l.chars().count()).collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{lines:?}");
    }

    #[test]
    fn a_binary_file_shows_no_counts() {
        let theme = Theme::plain();
        let mut s = stat(tpl::git::ChangeKind::Modified, "logo.png", 0, 0);
        s.binary = true;
        let line = change_stat(&theme, &s, 8);
        assert_eq!(line, "  modified  logo.png  Bin");
    }

    #[rstest]
    #[case(1, 1, 1, "1 file changed, 1 insertion(+), 1 deletion(-)")]
    #[case(3, 57, 15, "3 files changed, 57 insertions(+), 15 deletions(-)")]
    // A zero term is omitted, as Git omits it: `0 deletions(-)` is noise that
    // reads as a measurement.
    #[case(2, 4, 0, "2 files changed, 4 insertions(+)")]
    #[case(1, 0, 9, "1 file changed, 9 deletions(-)")]
    fn a_summary_counts_the_way_git_counts(
        #[case] files: usize,
        #[case] insertions: usize,
        #[case] deletions: usize,
        #[case] expected: &str,
    ) {
        assert_eq!(diff_summary(files, insertions, deletions), expected);
    }
}
