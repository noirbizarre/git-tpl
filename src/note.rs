//! Rendering a template's own words to a terminal, safely.
//!
//! A template repository is untrusted input, and [ADR-019] is where it first
//! gets to put bytes on the user's screen. That is a larger surface than it
//! looks: terminal escape sequences can write the clipboard, erase output that
//! is already on the screen, or reproduce git-tpl's own styling so that a line
//! the template wrote appears to be a line git-tpl wrote.
//!
//! So this module is an **allowlist**. A denylist cannot anticipate the next
//! terminal extension, and the sequences worth having are two.
//!
//! git-tpl still runs nothing a note names. A note saying "run
//! `curl … | sh`" is exactly as dangerous as a `README.md` saying it, and no
//! more — invariant 5 is untouched.
//!
//! [ADR-019]: https://github.com/noirbizarre/git-tpl/blob/main/docs/adr/019-templates-address-never-act.md

/// What a note is allowed to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formatting {
    /// Colour and text attributes (SGR), and `https` hyperlinks.
    ///
    /// A note with no emphasis and no clickable link is a note people
    /// stop reading, and the `README.md` in a pager it replaces has both.
    Allowed,

    /// Nothing. Plain text.
    ///
    /// Used when the stream is not a terminal, under `NO_COLOR`, and under
    /// `--json` — a note is not more readable for carrying escapes into a
    /// log file, and a `--json` consumer is not a terminal at all.
    Stripped,
}

/// The maximum bytes of a note that will be shown.
///
/// A `note_file` is a path into the template repository, so its size is
/// whatever the template chose. Without a bound, a template could push every
/// preceding line of git-tpl's output out of the scrollback — the same attack
/// as a cursor escape, achieved with newlines, which are not escapes and
/// cannot be stripped.
pub const LIMIT_BYTES: usize = 8 * 1024;

/// The marker appended when [`LIMIT_BYTES`] truncates a note.
const TRUNCATION_NOTE: &str = "… (truncated)";

/// Strip everything a terminal could act on beyond colour and a safe link.
///
/// The one entry point. Every path that shows a template's own text goes
/// through here, so there is no second, forgotten one.
pub fn sanitise(raw: &str, formatting: Formatting) -> String {
    let (text, truncated) = truncate(raw);

    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| clean(line, formatting))
        .collect();

    // A trailing newline in a file would otherwise print an empty line inside
    // the block, which reads as the template having said nothing at the end.
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    if truncated {
        lines.push(TRUNCATION_NOTE.to_string());
    }

    lines.join("\n")
}

/// Cut at a character boundary, so truncation cannot produce invalid UTF-8.
fn truncate(raw: &str) -> (&str, bool) {
    if raw.len() <= LIMIT_BYTES {
        return (raw, false);
    }
    let mut end = LIMIT_BYTES;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    (&raw[..end], true)
}

/// The sequence that ends a hyperlink: OSC 8 with an empty URI.
const LINK_CLOSE: &str = "\x1b]8;;\x1b\\";

/// The prefix every hyperlink sequence shares.
const LINK_PREFIX: &str = "\x1b]8;;";

/// One line, with everything a terminal could act on removed.
fn clean(line: &str, formatting: Formatting) -> String {
    let mut out = String::with_capacity(line.len());
    let mut styled = false;
    // Tracked because the two halves of a hyperlink are separate sequences and
    // either can be refused independently. Emitting one without the other is
    // how a refused `file:` link left a stray `\x1b]8;;\x1b\` in the output.
    let mut link_open = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if let Some(kept) = escape(&mut chars, formatting) {
                    if kept == LINK_CLOSE {
                        // A close for a link that was never opened, because its
                        // target was refused. Dropping the label's `]8;;` noise
                        // is the whole of the fix.
                        if !link_open {
                            continue;
                        }
                        link_open = false;
                    } else if kept.starts_with(LINK_PREFIX) {
                        link_open = true;
                    }
                    out.push_str(&kept);
                    styled = true;
                }
            }

            // A tab is the one control character that formats rather than
            // acts. Everything else in C0 either moves the cursor or is a
            // terminal instruction wearing a single byte: `\r` rewrites the
            // line just printed, `\x08` deletes backwards through it, and NUL
            // is undefined behaviour dressed as whitespace.
            '\t' => out.push('\t'),
            c if (c as u32) < 0x20 => {}
            '\x7f' => {}

            // C1. `\u{9b}` *is* CSI, and `\u{9d}` is OSC — a single byte that
            // opens exactly the sequences the `\x1b` arm is filtering. Dropping
            // the whole block is the only way to be sure the filter above
            // cannot be bypassed by writing the short form.
            c if ('\u{80}'..='\u{9f}').contains(&c) => {}

            c => out.push(c),
        }
    }

    // A link left open would swallow everything printed after it — including
    // the frame's own bottom rule — into the template's URL.
    if link_open {
        out.push_str(LINK_CLOSE);
    }

    // Reset at the end of every line, so a template that opens a style and
    // never closes it cannot colour git-tpl's own subsequent output — or, on a
    // terminal that keeps reverse video on, the user's shell prompt.
    if styled {
        out.push_str("\x1b[0m");
    }

    out
}

/// Consume one escape sequence, returning it only if it is allowed.
///
/// The cursor is consumed either way: a sequence that is dropped must be
/// dropped *whole*, or its parameter bytes fall through to the caller and are
/// printed as text — which is how a filter turns `\x1b[2J` into a visible `2J`
/// and, worse, how a partially-consumed sequence can resynchronise into a real
/// one.
fn escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    formatting: Formatting,
) -> Option<String> {
    match chars.next()? {
        // CSI. Parameters, then intermediates, then one final byte.
        '[' => {
            let mut params = String::new();
            let mut final_byte = None;
            for c in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&c) {
                    final_byte = Some(c);
                    break;
                }
                params.push(c);
            }

            // `m` is SGR: colour, bold, underline. It changes how the following
            // text looks and nothing else. Every other final byte moves the
            // cursor, erases, scrolls or queries — `J` clears the screen, `A`
            // moves up over output already printed, `n` makes the terminal
            // *write to stdin*.
            if final_byte != Some('m') || formatting == Formatting::Stripped {
                return None;
            }

            // Private-mode and intermediate bytes are not SGR even with an `m`
            // final byte, and their meaning is terminal-specific.
            if params.chars().any(|c| !c.is_ascii_digit() && c != ';') {
                return None;
            }

            Some(format!("\x1b[{params}m"))
        }

        // OSC. Terminated by BEL or ST (`ESC \`).
        ']' => {
            let mut body = String::new();
            while let Some(c) = chars.next() {
                if c == '\x07' {
                    break;
                }
                if c == '\x1b' {
                    // ST. The `\` is part of the terminator, not of the body.
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
                body.push(c);
            }

            if formatting == Formatting::Stripped {
                return None;
            }
            hyperlink(&body)
        }

        // Everything else: two-byte escapes, and the string-opening families.
        // DCS, SOS, PM and APC (`P`, `X`, `^`, `_`) carry a payload terminated
        // by ST, so dropping only the introducer would print the payload.
        c @ ('P' | 'X' | '^' | '_') => {
            let _ = c;
            while let Some(c) = chars.next() {
                if c == '\x1b' {
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
                if c == '\x07' {
                    break;
                }
            }
            None
        }

        // A bare two-byte escape — `ESC c` resets the terminal, `ESC 7` saves
        // the cursor. None of them format anything.
        _ => None,
    }
}

/// An OSC 8 hyperlink, if it is one and its target is safe.
///
/// `OSC 8 ; params ; URI ST`. An empty URI closes the link, and is kept: a
/// note that opens a link and cannot close it would swallow everything
/// printed after it into the same link.
fn hyperlink(body: &str) -> Option<String> {
    // OSC 52 writes the system clipboard. A note that could reach it could
    // plant `curl … | sh` for the user's next paste — an attack that survives
    // the terminal being closed, and that the user performs themselves. It is
    // the single reason this function is an allowlist.
    let rest = body.strip_prefix("8;")?;

    let (_params, uri) = rest.split_once(';')?;

    if uri.is_empty() {
        return Some(LINK_CLOSE.to_string());
    }

    // `https` only. `file:` reveals a local path and can be made to look like
    // a remote one; `javascript:` and `data:` are executable in some terminal
    // embeddings; and a scheme-relative or relative URI is resolved against
    // something we cannot know.
    if !uri.starts_with("https://") {
        return None;
    }

    // A `;` would make the rest of the URI a further OSC parameter, which is
    // how `8;;https://x.invalid;52;c;…` smuggles a second command past a naive
    // reader. ESC and BEL need no check: they terminate the sequence in the
    // caller's loop, so they cannot reach this far.
    if uri.contains(';') {
        return None;
    }

    Some(format!("{LINK_PREFIX}{uri}\x1b\\"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn allowed(raw: &str) -> String {
        sanitise(raw, Formatting::Allowed)
    }

    #[test]
    fn ordinary_text_passes_through_unchanged() {
        assert_eq!(
            allowed("Run scripts/bootstrap.sh"),
            "Run scripts/bootstrap.sh"
        );
    }

    #[test]
    fn colour_survives_because_an_unreadable_note_goes_unread() {
        let line = allowed("\x1b[1mBold\x1b[0m");
        assert!(line.starts_with("\x1b[1m"), "{line:?}");
        assert!(line.contains("Bold"));
    }

    /// The attack that makes this an allowlist rather than a denylist: OSC 52
    /// plants text in the clipboard, and the user pastes it themselves.
    #[test]
    fn an_osc_52_sequence_cannot_write_the_clipboard() {
        let line = allowed("\x1b]52;c;Y3VybCB4IHwgc2g=\x07done");
        assert_eq!(line, "done");
    }

    /// A dropped sequence must be dropped whole. Leaving the parameters behind
    /// would print `2J` and, worse, let the remainder resynchronise.
    #[rstest]
    // Erase the screen — everything git-tpl printed above the note.
    #[case("\x1b[2Jgone", "gone")]
    // Move up and overwrite the line that says which template this is.
    #[case("\x1b[3Aup", "up")]
    // Device status report: the terminal answers on *stdin*.
    #[case("\x1b[6nquery", "query")]
    // Scroll region.
    #[case("\x1b[1;2rscroll", "scroll")]
    // Full terminal reset.
    #[case("\x1bcreset", "reset")]
    // A DCS payload, which is not printable and must not be printed.
    #[case("\x1bPpayload\x1b\\after", "after")]
    // APC, the same shape.
    #[case("\x1b_payload\x1b\\after", "after")]
    fn a_sequence_that_acts_on_the_terminal_is_dropped_whole(
        #[case] raw: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(allowed(raw), expected);
    }

    /// `\u{9b}` is CSI in one byte. Without this, every case above can be
    /// rewritten to bypass the `\x1b` filter.
    #[test]
    fn a_single_byte_c1_introducer_cannot_bypass_the_filter() {
        assert_eq!(allowed("\u{9b}2Jgone"), "2Jgone");
        assert_eq!(allowed("\u{9d}52;c;x\x07"), "52;c;x");
    }

    /// `\r` rewrites the line just printed and `\x08` deletes backwards through
    /// it — cursor motion without an escape sequence.
    #[test]
    fn carriage_returns_and_backspaces_cannot_rewrite_a_printed_line() {
        assert_eq!(allowed("real\rfake"), "realfake");
        assert_eq!(allowed("safe\x08\x08\x08\x08evil"), "safeevil");
        assert_eq!(allowed("nul\0here"), "nulhere");
    }

    #[test]
    fn an_https_hyperlink_survives() {
        let line = allowed("\x1b]8;;https://example.com\x1b\\docs\x1b]8;;\x1b\\");
        assert!(line.contains("https://example.com"), "{line:?}");
        assert!(line.contains("docs"));
    }

    /// A `file:` target reveals a local path and can be dressed to look remote;
    /// `javascript:` is executable in some terminal embeddings.
    #[rstest]
    #[case("file:///etc/passwd")]
    #[case("javascript:alert(1)")]
    #[case("data:text/html,x")]
    #[case("http://example.com")]
    #[case("//example.com")]
    fn a_hyperlink_that_is_not_https_loses_its_target(#[case] uri: &str) {
        let line = allowed(&format!("\x1b]8;;{uri}\x1b\\label\x1b]8;;\x1b\\"));
        assert!(!line.contains(uri), "{line:?}");
        // The label is text and is kept; only the link is refused.
        assert!(line.contains("label"), "{line:?}");
    }

    /// A refused target used to leave the label followed by a bare `]8;;`,
    /// which is inert but looks exactly like a bug to the person reading it.
    #[test]
    fn a_refused_hyperlink_leaves_no_stray_close_sequence() {
        let line = allowed("\x1b]8;;file:///etc/passwd\x1b\\local\x1b]8;;\x1b\\ after");

        assert_eq!(line, "local after");
        assert!(!line.contains('\x1b'), "{line:?}");
    }

    /// Otherwise the frame's own bottom rule, and everything after it, is
    /// swallowed into the template's URL.
    #[test]
    fn a_hyperlink_left_open_is_closed_at_the_end_of_the_line() {
        let line = allowed("\x1b]8;;https://example.com\x1b\\dangling");

        assert!(line.contains("dangling"));
        assert!(
            line.trim_end_matches("\x1b[0m").ends_with(LINK_CLOSE),
            "{line:?}"
        );
    }

    /// Otherwise a note that opens a style and never closes it colours
    /// git-tpl's own following output, or the user's shell prompt.
    #[test]
    fn styling_cannot_leak_past_the_note_block() {
        let line = allowed("\x1b[31munclosed");
        assert!(line.ends_with("\x1b[0m"), "{line:?}");
    }

    /// DEL. Not an escape sequence, and not printable — some terminals treat it
    /// as a backspace, which is cursor motion by another name.
    #[test]
    fn a_delete_character_is_dropped() {
        assert_eq!(allowed("safe\x7fevil"), "safeevil");
    }

    /// A private-mode or intermediate parameter byte makes a sequence
    /// terminal-specific, whatever its final byte. `\x1b[?25l` hides the
    /// cursor; an `m` final byte does not make such a sequence SGR.
    #[rstest]
    #[case("\x1b[?25mhidden", "hidden")]
    #[case("\x1b[>1mgreater", "greater")]
    #[case("\x1b[!mbang", "bang")]
    fn a_private_mode_sequence_is_not_mistaken_for_styling(
        #[case] raw: &str,
        #[case] expected: &str,
    ) {
        let line = allowed(raw);
        assert_eq!(line, expected, "{line:?}");
        assert!(!line.contains('\x1b'));
    }

    /// A string-opening sequence may be terminated by BEL as well as by ST, and
    /// dropping only the introducer would print the payload as text.
    #[test]
    fn a_dcs_payload_terminated_by_bel_is_dropped() {
        assert_eq!(allowed("\x1bPpayload\x07after"), "after");
    }

    /// A second command smuggled into the URI as a further OSC parameter. The
    /// link is refused, and — the property that actually matters — whatever the
    /// remainder resynchronises to is re-filtered rather than emitted, so no
    /// escape survives to reach the terminal.
    #[test]
    fn a_hyperlink_uri_carrying_a_second_command_is_refused() {
        let line = allowed("\x1b]8;;https://example.com;\x1b]52;c;ZXZpbA==\x1b\\label");

        assert!(!line.contains('\x1b'), "{line:?}");
        assert!(!line.contains("https://example.com"), "{line:?}");
    }

    /// The OSC arm has to refuse before it parses a hyperlink, not after —
    /// otherwise a stripped stream would still carry the link sequence.
    #[test]
    fn an_osc_hyperlink_is_dropped_entirely_when_formatting_is_stripped() {
        let line = sanitise(
            "\x1b]8;;https://example.com\x1b\\label\x1b]8;;\x1b\\",
            Formatting::Stripped,
        );
        assert_eq!(line, "label");
    }
    #[test]
    fn each_line_is_reset_independently() {
        let text = allowed("\x1b[31mfirst\nsecond");
        let mut lines = text.split('\n');
        assert!(lines.next().unwrap().ends_with("\x1b[0m"));
        assert_eq!(lines.next().unwrap(), "second");
    }

    /// Under `--json`, when piped, and under `NO_COLOR`. A log file is not more
    /// readable for containing escapes.
    #[test]
    fn stripped_formatting_leaves_no_escape_at_all() {
        let line = sanitise(
            "\x1b[1mBold\x1b[0m \x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\",
            Formatting::Stripped,
        );
        assert_eq!(line, "Bold link");
        assert!(!line.contains('\x1b'));
    }

    /// Newlines are not escapes and cannot be filtered, so a long enough
    /// note would scroll git-tpl's own output away without one.
    #[test]
    fn an_oversized_note_is_truncated_rather_than_scrolling_the_screen() {
        let raw = "x".repeat(LIMIT_BYTES * 2);
        let out = allowed(&raw);
        assert!(out.len() < raw.len());
        assert!(
            out.ends_with(TRUNCATION_NOTE),
            "{:?}",
            &out[out.len() - 40..]
        );
    }

    /// Truncation cuts bytes, and a multi-byte character must not be split.
    #[test]
    fn truncation_lands_on_a_character_boundary() {
        let raw = "é".repeat(LIMIT_BYTES);
        // The assertion is that this does not panic and produces valid UTF-8.
        let out = allowed(&raw);
        assert!(out.ends_with(TRUNCATION_NOTE));
    }

    /// A file almost always ends in a newline, and an empty line inside the
    /// block reads as the template having trailed off.
    #[test]
    fn a_trailing_newline_does_not_become_an_empty_line() {
        assert_eq!(allowed("done\n"), "done");
        assert_eq!(allowed("done\n\n\n"), "done");
    }

    /// Tabs format; they are the one control character that does not act.
    #[test]
    fn a_tab_survives_because_it_lays_out_rather_than_acts() {
        assert_eq!(allowed("a\tb"), "a\tb");
    }
}
