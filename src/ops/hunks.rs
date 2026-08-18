//! Choosing which parts of a change to send.
//!
//! `git tpl backport -p`, and the whole of what makes it more than a flag.
//!
//! The selection is taken on the **rendered → project** diff — the user's own
//! edits, which is what `git add -p` shows and what they recognise — and the
//! result is a *partial project text*: the rendering, with only the chosen
//! hunks applied. That text is then handed to `backport`'s existing pipeline
//! exactly as the real file would have been.
//!
//! Selecting on the emitted patch instead was the obvious alternative and is
//! wrong: it happens after the source has been patched, so a file that refuses
//! is refused before the user ever sees a hunk, and there is nothing
//! well-defined left for the round trip to compare against. Here the ADR-020
//! proof keeps its exact meaning with one clause added — the patched source
//! renders to your file *with only the chosen hunks* — and `verify` needs no
//! change at all. See ADR-023.
//!
//! Nothing here decides anything. It cuts a change into hunks, and it puts a
//! chosen subset back together; who chooses is [`Picker`], and lives in the
//! frontend.

use std::ops::Range;

use similar::{DiffOp, TextDiff};

use super::backport::split_lines;
use super::unsubstitute::split_terminator;

/// The context radius of a hunk, matching the emitted patch.
///
/// The same three lines `file_diff` uses. A picker that grouped changes
/// differently from the patch it produces would be showing the user a
/// different document from the one they are approving.
const CONTEXT: usize = 3;

/// One contiguous run of changes, with its surrounding context.
///
/// Line-oriented, like everything else in `backport`: ADR-022 records why a
/// finer granularity does not survive `\r\n` or a combining sequence, and a
/// hunk the user cannot map back to a line of their file is not reviewable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// Position in the file's hunk list, 0-based. What [`Picker`] returns.
    pub index: usize,
    /// The `@@ -a,b +c,d @@` line, in `git diff` form.
    pub header: String,
    /// The body, each line prefixed ` `, `-` or `+`, terminators stripped.
    pub lines: Vec<String>,
    /// The lines of the *rendered* file this hunk spans, 0-based, context
    /// included. What lets a refusal that names a line name a hunk instead.
    pub rendered_lines: Range<usize>,
    /// Lines the project added.
    pub insertions: usize,
    /// Lines the project removed.
    pub deletions: usize,
}

/// One file's worth of change, offered for selection.
pub struct Selection<'a> {
    /// The rendered path, as the user knows it.
    pub path: &'a str,
    /// The template source that would be patched.
    pub template_path: &'a str,
    /// Every hunk of the change, in file order.
    pub hunks: &'a [Hunk],
}

/// Chooses which hunks of a change to carry.
///
/// Returns the indices to keep. [`None`] is a cancellation and aborts the
/// command — deliberately unlike [`crate::ops::Unsubstituter`], where Ctrl-C is
/// a decline. There, declining is the safe answer and the one the user was
/// heading for anyway. Here a stray Escape would either ship a change they
/// were in the middle of rejecting or drop one they had not reached yet, and
/// neither is something to guess at.
pub trait Picker {
    /// Show one file's hunks and take the answer.
    fn pick(&mut self, selection: &Selection<'_>) -> Option<Vec<usize>>;
}

/// Whether `backport` carries the whole change, and who says otherwise.
pub enum Picking<'a> {
    /// Carry every hunk. What `backport` does without `-p`.
    All,
    /// Ask, per file. `-p`.
    Ask(&'a mut dyn Picker),
}

/// Cut a change into hunks.
///
/// `rendered` is what the template produced; `project` is what the user has.
/// An added file is `rendered == ""`, which yields exactly one hunk — so
/// deselecting it is how a named added file is dropped, and the caller needs no
/// second code path for it.
pub(super) fn hunks(rendered: &str, project: &str) -> Vec<Hunk> {
    let old = split_lines(rendered);
    let new = split_lines(project);
    let diff = TextDiff::from_slices(&old, &new);

    diff.grouped_ops(CONTEXT)
        .into_iter()
        .enumerate()
        .map(|(index, group)| {
            let mut lines = Vec::new();
            let mut insertions = 0;
            let mut deletions = 0;

            for op in &group {
                // `Replace` is emitted as its deletions then its insertions,
                // which is the order `git diff` uses and the order the header's
                // line counts describe.
                for line in &old[op.old_range()] {
                    match op {
                        DiffOp::Equal { .. } => lines.push(format!(" {}", body(line))),
                        _ => {
                            deletions += 1;
                            lines.push(format!("-{}", body(line)));
                        }
                    }
                }
                if !matches!(op, DiffOp::Equal { .. }) {
                    for line in &new[op.new_range()] {
                        insertions += 1;
                        lines.push(format!("+{}", body(line)));
                    }
                }
            }

            Hunk {
                index,
                header: header(&group),
                lines,
                rendered_lines: spanned(&group),
                insertions,
                deletions,
            }
        })
        .collect()
}

/// Reassemble the project text with only `chosen` applied.
///
/// The inverse property is what makes this safe to put in front of the proof:
/// choosing every hunk reproduces `project` byte for byte, and choosing none
/// reproduces `rendered`. Both are pinned below, and both are why `backport`
/// needs no special case for "all" or "nothing".
pub(super) fn apply(rendered: &str, project: &str, chosen: &[usize]) -> String {
    let old = split_lines(rendered);
    let new = split_lines(project);
    let diff = TextDiff::from_slices(&old, &new);

    // Which group each changed op belongs to. Keyed on the op's position, which
    // is stable: `grouped_ops` trims the *context* at a group's edges, never a
    // changed op, so every key here appears in `ops()` unaltered.
    let mut group_of = std::collections::HashMap::new();
    for (index, group) in diff.grouped_ops(CONTEXT).into_iter().enumerate() {
        for op in group {
            if !matches!(op, DiffOp::Equal { .. }) {
                group_of.insert((op.old_range().start, op.new_range().start), index);
            }
        }
    }

    // One emission point. There is exactly one rule — take the project's lines
    // where the hunk was chosen and the rendering's everywhere else — and a
    // second loop that forgot half of it is a patch that silently carries what
    // the user just declined.
    let mut out = String::new();
    for op in diff.ops() {
        let taken = !matches!(op, DiffOp::Equal { .. })
            && group_of
                .get(&(op.old_range().start, op.new_range().start))
                .is_some_and(|index| chosen.contains(index));

        if taken {
            out.extend(new[op.new_range()].iter().copied());
        } else {
            out.extend(old[op.old_range()].iter().copied());
        }
    }
    out
}

/// The hunk a refusal's line falls in, if it is one the user chose.
///
/// A refusal names a line of the rendered file; after `-p` the actionable form
/// of that is "this hunk, the one you selected". Only chosen hunks are
/// considered: a line inside a hunk that was left behind cannot be the cause of
/// a refusal, because none of its changes are in the patch.
pub(super) fn containing<'h>(hunks: &'h [Hunk], chosen: &[usize], line: usize) -> Option<&'h Hunk> {
    hunks
        .iter()
        .find(|hunk| chosen.contains(&hunk.index) && hunk.rendered_lines.contains(&line))
}

/// The rendered lines a group of ops spans, context included.
fn spanned(group: &[DiffOp]) -> Range<usize> {
    match (group.first(), group.last()) {
        (Some(first), Some(last)) => first.old_range().start..last.old_range().end,
        _ => 0..0,
    }
}

/// The `@@ -a,b +c,d @@` line for a group of ops.
fn header(group: &[DiffOp]) -> String {
    let (first, last) = match (group.first(), group.last()) {
        (Some(first), Some(last)) => (first, last),
        // `grouped_ops` never yields an empty group, but a header is not worth
        // a panic if it ever does.
        _ => return "@@ @@".to_string(),
    };
    let old = first.old_range().start..last.old_range().end;
    let new = first.new_range().start..last.new_range().end;

    // A zero-length range is anchored on the line *before* it, which is what
    // `git diff` emits for a pure insertion at the top of a file.
    let old_start = if old.is_empty() {
        old.start
    } else {
        old.start + 1
    };
    let new_start = if new.is_empty() {
        new.start
    } else {
        new.start + 1
    };

    format!(
        "@@ -{old_start},{} +{new_start},{} @@",
        old.len(),
        new.len()
    )
}

/// A line without its terminator, for display.
fn body(line: &str) -> &str {
    split_terminator(line).0
}

#[cfg(test)]
mod tests {
    use super::*;

    const RENDERED: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";

    /// The same file with two changes far enough apart to be separate hunks.
    const PROJECT: &str = "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n";

    fn all(hunks: &[Hunk]) -> Vec<usize> {
        hunks.iter().map(|hunk| hunk.index).collect()
    }

    #[test]
    fn a_change_far_apart_is_two_hunks() {
        let hunks = hunks(RENDERED, PROJECT);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].index, 0);
        assert_eq!(hunks[1].index, 1);
        assert_eq!(hunks[0].insertions, 1);
        assert_eq!(hunks[0].deletions, 1);
        assert!(hunks[0].lines.contains(&"-one".to_string()));
        assert!(hunks[0].lines.contains(&"+ONE".to_string()));
        assert!(hunks[0].header.starts_with("@@ -1,"));
    }

    #[test]
    fn a_refusal_is_attributed_to_the_hunk_that_carried_it() {
        let hunks = hunks(RENDERED, PROJECT);
        // Line 0 of the rendering is the first change; line 9 the second.
        assert_eq!(
            containing(&hunks, &all(&hunks), 0).map(|h| h.index),
            Some(0)
        );
        assert_eq!(
            containing(&hunks, &all(&hunks), 9).map(|h| h.index),
            Some(1)
        );
        // A hunk left behind is never the cause: none of it is in the patch.
        assert!(containing(&hunks, &[0], 9).is_none());
    }

    #[test]
    fn every_hunk_selected_reproduces_the_project_file() {
        let hunks = hunks(RENDERED, PROJECT);
        assert_eq!(apply(RENDERED, PROJECT, &all(&hunks)), PROJECT);
    }

    #[test]
    fn no_hunk_selected_reproduces_the_rendering() {
        assert_eq!(apply(RENDERED, PROJECT, &[]), RENDERED);
    }

    #[test]
    fn an_unselected_hunk_keeps_the_rendered_lines() {
        // The first change is taken, the second left behind.
        let partial = apply(RENDERED, PROJECT, &[0]);
        assert!(partial.starts_with("ONE\n"));
        assert!(partial.ends_with("ten\n"));
    }

    #[test]
    fn a_crlf_file_keeps_its_terminators_through_a_partial_selection() {
        // Windows. A selection that normalised line endings would rewrite every
        // line of the template and `git am` would refuse the result outright.
        let rendered = "one\r\ntwo\r\nthree\r\n";
        let project = "ONE\r\ntwo\r\nthree\r\n";
        assert_eq!(apply(rendered, project, &[]), rendered);
        assert_eq!(apply(rendered, project, &[0]), project);
    }

    #[test]
    fn a_missing_final_newline_survives_a_partial_selection() {
        let rendered = "one\ntwo";
        let project = "one\nTWO";
        assert_eq!(apply(rendered, project, &[]), rendered);
        assert_eq!(apply(rendered, project, &[0]), project);
    }

    #[test]
    fn an_added_file_is_one_hunk() {
        let hunks = hunks("", "alpha\nbeta\n");
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].insertions, 2);
        assert_eq!(hunks[0].deletions, 0);
        // Anchored on line 0: there is no line 1 to anchor on.
        assert!(hunks[0].header.starts_with("@@ -0,0 +1,2"));
        assert_eq!(apply("", "alpha\nbeta\n", &[]), "");
        assert_eq!(apply("", "alpha\nbeta\n", &[0]), "alpha\nbeta\n");
    }

    #[test]
    fn a_hunk_shows_its_context() {
        let hunks = hunks(RENDERED, PROJECT);
        // Three lines either side, so the first hunk's context is `two`,
        // `three` and `four` — and nothing from the second change.
        assert_eq!(hunks[0].lines.last().unwrap(), " four");
        assert!(!hunks[0].lines.iter().any(|line| line.contains("ten")));
    }

    #[test]
    fn an_unchanged_file_has_no_hunks() {
        assert!(hunks(RENDERED, RENDERED).is_empty());
        assert_eq!(apply(RENDERED, RENDERED, &[]), RENDERED);
    }
}
