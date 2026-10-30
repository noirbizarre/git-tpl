//! Terminal presentation.
//!
//! Every function here returns a `String` rather than printing. That keeps the
//! formatting testable without capturing stdout, and keeps the decision about
//! *where* output goes with the caller — human output goes to stderr, so
//! `--json` leaves stdout machine-readable.

use console::Style;

use tpl::ops::{Proposal, Selection};

use crate::cli::ColorChoice;

/// Clap uses its own style types; keep its palette aligned with human reports.
pub fn clap_styles() -> clap::builder::Styles {
    use clap::builder::styling::AnsiColor;

    clap::builder::Styles::styled()
        .header(AnsiColor::Cyan.on_default().bold())
        .usage(AnsiColor::Cyan.on_default().bold())
        .literal(AnsiColor::Cyan.on_default())
        .placeholder(clap::builder::styling::Style::new().dimmed())
        .error(AnsiColor::Red.on_default().bold())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Yellow.on_default())
}

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
    /// Emphasis carrying no colour of its own — bold white — for text that
    /// needs to stand out (a case name in `git tpl test`'s progress display)
    /// without borrowing a hue that already means something else (`added`'s
    /// green, `deleted`'s red, `heading`'s cyan).
    pub bold: Style,
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
            bold: Style::new(),
        }
    }

    /// Colour, using Git's own vocabulary — green added, red deleted — so the
    /// output reads like the Git output it sits beside.
    pub fn colored() -> Self {
        // Forced regardless of `console`'s own terminal auto-detection: an
        // explicit "colourised" theme must not silently produce no escape
        // codes just because stderr is a pipe. `console::Style` otherwise
        // defers to a global, isatty-gated flag — which is exactly backwards
        // for `--color always`, a CI log, or any of this suite's
        // `tpl_colored` tests, none of which are ever a real terminal.
        fn forced(style: Style) -> Style {
            style.force_styling(true)
        }
        Self {
            colored: true,
            heading: forced(Style::new().cyan().bold()),
            muted: forced(Style::new().dim()),
            added: forced(Style::new().green()),
            modified: forced(Style::new().yellow()),
            deleted: forced(Style::new().red()),
            warning: forced(Style::new().yellow()),
            command: forced(Style::new().cyan()),
            bold: forced(Style::new().white().bold()),
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
                    std::env::var("FORCE_COLOR").ok().as_deref(),
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
    force_color: Option<&str>,
    clicolor_force: Option<&str>,
    term: Option<&str>,
    is_terminal: bool,
) -> bool {
    // https://force-color.org — an explicit request wins over everything,
    // including not being a terminal, because that is what CI systems set.
    // Checked ahead of `CLICOLOR_FORCE`: the clicolors standard itself
    // (https://bixense.com/clicolors/, now deprecated in `FORCE_COLOR`'s
    // favour) says software supporting it "should treat FORCE_COLOR as an
    // alias for CLICOLOR_FORCE, enabling colour whenever either is set" — so
    // both get the same "present and not `0`" rule, not two different ones.
    if force_color.is_some_and(|v| v != "0") {
        return true;
    }
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

/// A compact source-history row. Only our styles may reach the terminal.
pub fn log_entry(
    theme: &Theme,
    entry: &tpl::git::HistoryEntry,
    now: i64,
    relative: bool,
) -> String {
    let subject = tpl::note::plain_line(&entry.subject);
    let subject = match tpl::ops::log::conventional(&subject) {
        Some(prefix) => format!(
            "{}{}",
            theme.heading.apply_to(&subject[..prefix.prefix_len]),
            &subject[prefix.prefix_len..]
        ),
        None => subject,
    };
    format!(
        "  {}  {}  {}",
        theme.modified.apply_to(entry.oid.short()),
        subject,
        theme.muted.apply_to(format!(
            "({})",
            commit_date(entry.committed_at, now, relative)
        ))
    )
}

/// Dates are presentation only; a fixed clock makes relative output testable.
pub fn commit_date(timestamp: i64, now: i64, relative: bool) -> String {
    if !relative {
        return match time::OffsetDateTime::from_unix_timestamp(timestamp) {
            Ok(date) => format!(
                "{:04}-{:02}-{:02} {:02}:{:02} UTC",
                date.year(),
                date.month() as u8,
                date.day(),
                date.hour(),
                date.minute()
            ),
            // Git accepts timestamps beyond the calendar library's range; do not panic on source input.
            Err(_) => format!("{timestamp} Unix seconds"),
        };
    }
    let seconds = now.abs_diff(timestamp);
    let (count, unit) = [
        (31_536_000, "year"),
        (2_592_000, "month"),
        (604_800, "week"),
        (86_400, "day"),
        (3_600, "hour"),
        (60, "minute"),
        (1, "second"),
    ]
    .into_iter()
    .find_map(|(size, unit)| (seconds >= size).then_some((seconds / size, unit)))
    .unwrap_or((0, "second"));
    let duration = format!("{count} {unit}{}", if count == 1 { "" } else { "s" });
    if timestamp > now {
        format!("in {duration}")
    } else {
        format!("{duration} ago")
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;

    #[test]
    fn relative_dates_handle_unit_boundaries_and_future_clocks() {
        for (elapsed, expected) in [
            (0, "0 seconds ago"),
            (1, "1 second ago"),
            (59, "59 seconds ago"),
            (60, "1 minute ago"),
            (3_599, "59 minutes ago"),
            (3_600, "1 hour ago"),
            (86_400, "1 day ago"),
            (604_800, "1 week ago"),
            (2_592_000, "1 month ago"),
            (31_536_000, "1 year ago"),
        ] {
            assert_eq!(commit_date(0, elapsed, true), expected);
        }
        assert_eq!(commit_date(120, 0, true), "in 2 minutes");
        // Malicious or corrupt Git timestamps cannot overflow relative arithmetic.
        assert!(!commit_date(i64::MIN, i64::MAX, true).is_empty());
    }

    #[test]
    fn absolute_dates_use_utc_and_truncate_seconds_to_the_minute() {
        assert_eq!(commit_date(0, 0, false), "1970-01-01 00:00 UTC");
        assert_eq!(commit_date(-1, 0, false), "1969-12-31 23:59 UTC");
        assert_eq!(commit_date(951_868_799, 0, false), "2000-02-29 23:59 UTC");
        assert_eq!(
            commit_date(i64::MAX, 0, false),
            format!("{} Unix seconds", i64::MAX)
        );
    }

    #[test]
    fn history_rows_keep_plain_subjects_and_style_only_conventional_prefixes() {
        let entry = tpl::git::HistoryEntry {
            oid: tpl::git::Oid::from_bytes([0; 20]),
            subject: "feat(cli)!: show history".into(),
            committed_at: 0,
        };
        assert_eq!(
            log_entry(&Theme::plain(), &entry, 120, true),
            "  0000000  feat(cli)!: show history  (2 minutes ago)"
        );
        let colored = log_entry(&Theme::colored(), &entry, 120, true);
        assert!(colored.contains("feat(cli)!: \x1b[0mshow history"));
        let plain = tpl::git::HistoryEntry {
            subject: "Merge feature branch".into(),
            ..entry
        };
        assert!(log_entry(&Theme::plain(), &plain, 0, true).contains("Merge feature branch"));
    }
}

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

/// One line of a unified diff — from `git tpl test`'s recorded snapshots — in
/// Git's own vocabulary: green added, red deleted, the same vocabulary
/// [`hunks`] uses for `backport`'s diff. The file header (`---`/`+++`) and the
/// `@@ ... @@` hunk marker stay muted, like everything else in this list —
/// only the change itself should stand out.
///
/// Takes a flat line rather than a [`tpl::ops::Hunk`] like [`hunks`] does:
/// `ops::testing::patch` produces a plain `similar` unified-diff string, not
/// the structured hunks a `backport` selection already has split out.
pub fn patch_line(theme: &Theme, line: &str) -> String {
    if line.starts_with('+') && !line.starts_with("+++") {
        theme.added.apply_to(line).to_string()
    } else if line.starts_with('-') && !line.starts_with("---") {
        theme.deleted.apply_to(line).to_string()
    } else {
        muted(theme, line)
    }
}

/// A dimmed note.
pub fn muted(theme: &Theme, text: &str) -> String {
    theme.muted.apply_to(text).to_string()
}

/// Bold white — emphasis with no other meaning attached.
pub fn bold(theme: &Theme, text: &str) -> String {
    theme.bold.apply_to(text).to_string()
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
    // case CI systems set it for. Either variable, since they are aliases.
    #[case(None, None, Some("1"), None, false, true)]
    #[case(None, Some("1"), None, None, false, true)]
    // `0` is a no-op for either variable, not a request to disable colour —
    // there is nothing left to force it on here, so it falls through to
    // "no terminal" and stays plain.
    #[case(None, Some("0"), None, None, false, false)]
    // NO_COLOR wins over being a terminal, whatever its value.
    #[case(Some(""), None, None, None, true, false)]
    #[case(Some("1"), None, None, None, true, false)]
    // ...but not over an explicit force, from either variable.
    #[case(Some("1"), None, Some("1"), None, false, true)]
    #[case(Some("1"), Some("1"), None, None, false, true)]
    #[case(None, None, None, Some("dumb"), true, false)]
    #[case(None, None, None, Some("xterm"), true, true)]
    #[case(None, None, None, Some("xterm"), false, false)]
    fn colour_precedence_is_explicit_request_then_refusal_then_terminal(
        #[case] no_color: Option<&str>,
        #[case] force_color: Option<&str>,
        #[case] clicolor_force: Option<&str>,
        #[case] term: Option<&str>,
        #[case] is_terminal: bool,
        #[case] expected: bool,
    ) {
        assert_eq!(
            decide(no_color, force_color, clicolor_force, term, is_terminal),
            expected
        );
    }

    #[test]
    fn a_plain_theme_adds_no_escape_sequences() {
        let theme = Theme::plain();
        let line = change(&theme, tpl::git::ChangeKind::Added, "Cargo.toml");
        assert_eq!(line, "  added     Cargo.toml");
        assert!(!line.contains('\x1b'));
    }

    /// Guards the fix for `Theme::colored()` never forcing styling: this test
    /// runs in-process, with no control over whether the test binary's own
    /// stdout/stderr happen to be a terminal, so before the fix this assertion
    /// was true or false depending on how `cargo test` was invoked rather than
    /// on the theme actually being "colourised".
    #[test]
    fn a_colored_theme_adds_escape_sequences_without_a_terminal() {
        assert!(muted(&Theme::colored(), "x").contains('\x1b'));
    }

    /// `+`/`-` stand out; the file header they sit beside does not, because it
    /// is not the change itself.
    #[test]
    fn patch_line_colours_additions_and_deletions_but_not_the_header() {
        let theme = Theme::colored();
        assert_eq!(
            patch_line(&theme, "+added"),
            theme.added.apply_to("+added").to_string()
        );
        assert_eq!(
            patch_line(&theme, "-removed"),
            theme.deleted.apply_to("-removed").to_string()
        );
        assert_eq!(
            patch_line(&theme, "+++ rendered"),
            muted(&theme, "+++ rendered")
        );
        assert_eq!(
            patch_line(&theme, "--- snapshot"),
            muted(&theme, "--- snapshot")
        );
        assert_eq!(
            patch_line(&theme, "@@ -1,2 +1,3 @@"),
            muted(&theme, "@@ -1,2 +1,3 @@")
        );
    }

    #[test]
    fn a_plain_theme_patch_line_adds_no_escape_sequences() {
        let theme = Theme::plain();
        assert_eq!(patch_line(&theme, "+added"), "+added");
        assert_eq!(patch_line(&theme, "-removed"), "-removed");
        assert!(!patch_line(&theme, "+added").contains('\x1b'));
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
