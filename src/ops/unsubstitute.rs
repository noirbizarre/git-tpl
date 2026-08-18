//! Reversing a substitution, when the reversal can be proved line by line.
//!
//! ADR-020 shipped a backport that never invents a `{{ }}`: a change is carried
//! only where the template copied its source verbatim. ADR-022 lifts that, and
//! the way it lifts it is the whole content of this module.
//!
//! Not a substitution table. Searching the rendered text for an answer's value
//! and replacing it with `{{ name }}` cannot distinguish a substitution from a
//! coincidence — at the level of bytes there is no difference between them, and
//! no refusal list catches `author = "June"` against a template that hard-codes
//! the month. So nothing here searches for a value.
//!
//! Instead: split the *source* line into literal and `{{ … }}` spans, render
//! each expression span on its own, and require the pieces to reassemble into
//! the rendered line byte for byte. That reassembly is a proof, by the same
//! determinism (invariant 2) that ADR-020 leans on, and what it proves is an
//! exact byte-range provenance — which bytes of the output were copied and
//! which an expression produced. An edit inside a copied range is a change to
//! the template; an edit touching a produced range is a change to an *answer*,
//! and is refused.
//!
//! What this does not prove is that the result is right for anyone else's
//! answers. See [`Unsubstituter`] and ADR-022 for why that is a human's
//! decision rather than a check.

use std::ops::Range;
use std::sync::Arc;

use similar::utils::diff_graphemes;
use similar::{Algorithm, ChangeTag};

use crate::context::Context;
use crate::eval::{Partials, Undefined, render_string_with};

/// Everything a line needs to be re-rendered, gathered once per file.
///
/// Carried rather than rebuilt because `transpose` may establish provenance for
/// a dozen candidate lines, and each one renders every expression it holds.
pub(super) struct LineContext<'a> {
    /// The resolved context the file was rendered with. The same one, never a
    /// fresh evaluation: a different context would prove a different template.
    pub context: &'a Context,
    /// The template's partials, so an expression calling a macro still works.
    pub partials: &'a Arc<Partials>,
    /// The manifest's undefined behaviour, so `strict = true` is strict here.
    pub undefined: Undefined,
}

/// Whether `backport` may reverse a substitution, and who says so.
///
/// Deliberately not modelled on [`crate::ops::Trust`], which has a `Refuse`
/// variant because a remote fetch is something the *template* asked for and
/// git-tpl must answer one way or the other. Un-substitution is something
/// git-tpl offers; nobody asked for it, so declining is just not doing it.
pub enum Unsubstitute<'a> {
    /// Do not attempt it. What `backport` did before ADR-022, and what happens
    /// when there is nobody to ask — a `substituted_region` refusal, unchanged.
    Never,
    /// Attempt it, and confirm every line with the user.
    Ask(&'a mut dyn Unsubstituter),
    /// Attempt it, confirming nothing. `--unsubstitute`.
    Always,
}

/// Confirms one reversed substitution, or declines it.
///
/// The round-trip check in `backport::verify` proves the patched source
/// produces *this user's* file. Un-substitution is the first thing in the
/// command for which that is not the same as being right for everyone:
///
/// ```text
/// source    version = "{{ version }}"      with version = "1.0"
/// rendered  version = "1.0"
/// project   version = "1.0.0"
/// ```
///
/// The inserted `.0` sits against the value, and attributing it to the literal
/// gives `version = "{{ version }}.0"` — which round-trips perfectly and
/// appends `.0` to every downstream project. The slider in [`Provenance::rewrite`]
/// refuses that particular one, but the class is not decidable from the bytes:
/// only the person who made the edit knows whether they meant to change the
/// template or their own answer. So they are asked, per line.
pub trait Unsubstituter {
    /// Show one proposed reversal and take the answer.
    fn confirm(&mut self, proposal: &Proposal<'_>) -> Verdict;
}

/// What the user is shown before a substitution is reversed.
///
/// Three texts, because two of them do not determine the third: the same edit
/// has several placements that all render back correctly, and the one chosen is
/// the thing being consented to.
pub struct Proposal<'p> {
    /// The rendered path, as the user knows it.
    pub path: &'p str,
    /// The template source that would be patched.
    pub template_path: &'p str,
    /// The line, 1-based, in the rendered file.
    pub line: usize,
    /// What the template produced.
    pub rendered: &'p str,
    /// What the project has now.
    pub project: &'p str,
    /// What would be written to the template source.
    pub patched: &'p str,
    /// The `{{ … }}` the reversal keeps, in the order they appear.
    pub expressions: &'p [String],
}

/// The answer to a [`Proposal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Carry the line.
    Accept,
    /// Refuse the file, as though the reversal had never been attempted.
    Decline,
}

/// One reversal a backport made, kept for the confirmation the caller owes.
#[derive(Debug, Clone)]
pub struct Unsubstitution {
    /// The rendered path. Empty until `backport` fills it in — `transpose`
    /// works on one file and has no need to name it.
    pub path: String,
    /// The template source that was patched. Empty until `backport` fills it.
    pub template_path: String,
    /// The line, 1-based, in the rendered file.
    pub line: usize,
    /// What the template produced, terminator stripped.
    pub rendered: String,
    /// What the project has now, terminator stripped.
    pub project: String,
    /// The template source line as it would be written, terminator stripped.
    pub patched: String,
    /// The `{{ … }}` kept, in the order they appear.
    pub expressions: Vec<String>,
}

/// A span of a source line, before anything is rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceSpan {
    /// Bytes the render copies to the output unchanged.
    Literal(Range<usize>),
    /// A `{{ … }}`, delimiters included.
    Expression(Range<usize>),
}

/// One span of a rendered line, with where it came from.
#[derive(Debug, Clone)]
struct Segment {
    /// The byte range in the source line. For an expression this is the
    /// `{{ … }}` as written — what the rebuild emits, so the placeholder comes
    /// back with its original spelling and spacing rather than a normalised one.
    source: Range<usize>,
    /// The byte range in the rendered line it accounts for.
    rendered: Range<usize>,
    /// Whether the render copied it or produced it.
    literal: bool,
}

/// One rendered line's byte-range provenance, established by re-rendering.
pub(super) struct Provenance {
    /// The source line this describes, indexed into the file's source lines.
    pub source: usize,
    /// Alternating literal and expression spans, covering the rendered line
    /// exactly. Never empty, and always contains at least one of each.
    segments: Vec<Segment>,
    /// The `{{ … }}` texts, for the confirmation.
    pub expressions: Vec<String>,
}

/// The source line that produced `rendered_body`, when exactly one did.
///
/// Every candidate in `candidates` is tried, rather than pairing by position.
/// A `Replace` op pairs a run of source lines with a run of rendered ones and
/// the pairing carries no meaning of its own — two unrelated lines changing at
/// once produce the same shape — so the answer is only taken when it is unique.
/// Two source lines that both reproduce this rendered line is a genuine
/// ambiguity, and there is nothing to choose between them.
pub(super) fn pair(
    source_lines: &[&str],
    candidates: Range<usize>,
    rendered_body: &str,
    lines: &LineContext<'_>,
) -> Option<Provenance> {
    // A `Replace` this wide is a rewritten block, not a line edit, and the
    // quadratic render cost below is not worth paying to find that out.
    const WIDEST: usize = 32;
    if candidates.len() > WIDEST {
        return None;
    }

    let mut found: Option<Provenance> = None;
    for index in candidates {
        let Some(provenance) = establish(source_lines[index], rendered_body, index, lines) else {
            continue;
        };
        if found.is_some() {
            return None;
        }
        found = Some(provenance);
    }
    found
}

/// Split `source_line` into spans and prove they reassemble into `rendered_body`.
///
/// The guard the whole feature rests on. A line inside a `{% for %}` body, a
/// line trimmed by whitespace control, a line inside `{% raw %}`, and a value
/// containing a newline all fail it — which is the point: each of those has a
/// provenance the byte ranges here would describe wrongly.
fn establish(
    source_line: &str,
    rendered_body: &str,
    source_index: usize,
    lines: &LineContext<'_>,
) -> Option<Provenance> {
    let (source_body, _) = split_terminator(source_line);
    let spans = scan(source_body)?;

    // A line that is nothing but an expression has no editable text: every
    // change to it is a change to the value, which is a change to an answer.
    if !spans
        .iter()
        .any(|span| matches!(span, SourceSpan::Literal(range) if !range.is_empty()))
    {
        return None;
    }

    let mut segments = Vec::with_capacity(spans.len());
    let mut expressions = Vec::new();
    let mut built = String::new();

    for span in &spans {
        match span {
            SourceSpan::Literal(range) => {
                let text = &source_body[range.clone()];
                let at = built.len();
                built.push_str(text);
                segments.push(Segment {
                    source: range.clone(),
                    rendered: at..built.len(),
                    literal: true,
                });
            }
            SourceSpan::Expression(range) => {
                let text = &source_body[range.clone()];
                let value = render_string_with(
                    text,
                    lines.context,
                    // Never surfaced: a failure here is a refusal, and the
                    // caller's `substituted_region` names the file already.
                    "",
                    lines.partials,
                    lines.undefined,
                )
                .ok()?;

                // An expression rendering to nothing occupies a zero-width
                // range, which is invisible in the output: the reassembly below
                // cannot confirm the expression was ever evaluated here, and an
                // insertion "at" it belongs to neither neighbour. This is also
                // what closes the `{% for %}`-body hole under `Lenient`, where
                // an unbound loop variable renders to exactly the empty string.
                //
                // A newline in a value means one source line produced several
                // rendered ones, and the line model here does not hold.
                if value.is_empty() || value.contains(['\n', '\r']) {
                    return None;
                }

                let at = built.len();
                built.push_str(&value);
                segments.push(Segment {
                    source: range.clone(),
                    rendered: at..built.len(),
                    literal: false,
                });
                expressions.push(text.to_string());
            }
        }
    }

    // The proof, at the scale of one line.
    if built != rendered_body {
        return None;
    }

    Some(Provenance {
        source: source_index,
        segments,
        expressions,
    })
}

impl Provenance {
    /// The source line rewritten to carry the project's edit, or `None`.
    ///
    /// `None` is a refusal in every case. The caller falls back to
    /// `substituted_region`, which is what it would have raised anyway.
    pub(super) fn rewrite(
        &self,
        source_line: &str,
        rendered_body: &str,
        project_body: &str,
    ) -> Option<String> {
        // One accumulator per segment. Literals collect the project's text;
        // expression accumulators are filled and then discarded, because the
        // rebuild emits the `{{ … }}` from the source instead.
        let mut parts: Vec<String> = vec![String::new(); self.segments.len()];
        let mut at = 0usize;

        // Graphemes rather than bytes or chars, so an edit cannot split a
        // `\r\n` or a combining sequence and leave a range that is not a
        // character boundary.
        for (tag, text) in diff_graphemes(Algorithm::Myers, rendered_body, project_body) {
            match tag {
                ChangeTag::Equal => {
                    self.spread(&mut parts, at, text);
                    at += text.len();
                }
                ChangeTag::Delete => {
                    // Deleted text contributes nothing, but it still has to be
                    // shown to have come from a literal.
                    self.literal_holding(slide_delete(rendered_body, at..at + text.len()))?;
                    at += text.len();
                }
                ChangeTag::Insert => {
                    let index = self.literal_holding(slide_insert(rendered_body, at, text))?;
                    parts[index].push_str(text);
                }
            }
        }

        let (source_body, terminator) = split_terminator(source_line);
        let mut out = String::new();
        for (index, segment) in self.segments.iter().enumerate() {
            if segment.literal {
                out.push_str(&parts[index]);
            } else {
                out.push_str(&source_body[segment.source.clone()]);
            }
        }
        out.push_str(terminator);
        Some(out)
    }

    /// Distribute an unchanged run across the segments it covers.
    fn spread(&self, parts: &mut [String], at: usize, text: &str) {
        let span = at..at + text.len();
        for (index, segment) in self.segments.iter().enumerate() {
            let start = segment.rendered.start.max(span.start);
            let end = segment.rendered.end.min(span.end);
            if start < end {
                parts[index].push_str(&text[start - at..end - at]);
            }
        }
    }

    /// The literal segment wholly holding `span`, or `None` to refuse.
    ///
    /// Closed at both ends, so a zero-width insertion point exactly on the
    /// boundary between a literal and a value counts as inside the literal —
    /// which is how appending to a line ending in `{{ … }}` works at all.
    fn literal_holding(&self, span: Range<usize>) -> Option<usize> {
        self.segments.iter().position(|segment| {
            segment.literal
                && segment.rendered.start <= span.start
                && span.end <= segment.rendered.end
        })
    }
}

/// Split a source line into alternating literal and `{{ … }}` spans.
///
/// `None` is a refusal, not a parse failure. A block tag spans lines and a
/// comment produces nothing, so neither can be given a byte range on this line;
/// whitespace control reaches into the *neighbouring* line's bytes, so a range
/// computed here would describe the wrong text. Guessing one is exactly how a
/// plausible wrong patch is built.
///
/// The result always starts and ends with a `Literal`, possibly zero-width, so
/// that every position in the line — including 0 and the end — falls inside
/// some literal's closed range.
fn scan(line: &str) -> Option<Vec<SourceSpan>> {
    if line.contains("{%") || line.contains("{#") {
        return None;
    }

    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut literal_start = 0usize;
    let mut at = 0usize;

    while at + 1 < bytes.len() {
        if bytes[at] != b'{' || bytes[at + 1] != b'{' {
            at += 1;
            continue;
        }
        // `{{-` trims the whitespace *before* the expression, which is as often
        // as not on the previous line.
        if bytes.get(at + 2) == Some(&b'-') {
            return None;
        }
        let close = closing(bytes, at + 2)?;
        // `-}}` trims what follows, with the same reach.
        if bytes[close - 1] == b'-' {
            return None;
        }
        spans.push(SourceSpan::Literal(literal_start..at));
        spans.push(SourceSpan::Expression(at..close + 2));
        at = close + 2;
        literal_start = at;
    }

    if spans.is_empty() {
        // Nothing was substituted into this line, so there is nothing to
        // reverse. The caller only reaches here for a line the alignment says
        // rendering changed, so this is a disagreement rather than a no-op.
        return None;
    }

    spans.push(SourceSpan::Literal(literal_start..bytes.len()));
    Some(spans)
}

/// The index of the `}}` closing an expression that opened before `from`.
///
/// Quote-aware, because `{{ "}}" }}` is one expression and not a truncated one.
/// MiniJinja's lexer knows where a string runs; a scan that did not would cut
/// the expression in half and render a fragment.
fn closing(bytes: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    let mut quote: Option<u8> = None;
    while at < bytes.len() {
        let byte = bytes[at];
        match quote {
            Some(delimiter) => {
                if byte == b'\\' {
                    at += 2;
                    continue;
                }
                if byte == delimiter {
                    quote = None;
                }
            }
            None => {
                if byte == b'\'' || byte == b'"' {
                    quote = Some(byte);
                } else if byte == b'}' && bytes.get(at + 1) == Some(&b'}') {
                    return Some(at);
                } else if byte == b'{' && bytes.get(at + 1) == Some(&b'{') {
                    // A nested `{{` is not something MiniJinja accepts, and
                    // guessing which of the two `}}` closes which is not a
                    // guess worth making.
                    return None;
                }
            }
        }
        at += 1;
    }
    None
}

/// Every position a deletion of `span` could equally have been placed at.
///
/// `similar` returns one alignment out of several equally short ones, and which
/// one it returns is an artefact of its tie-breaking rather than a statement
/// about what the user meant. So a hunk that *could* have been placed inside a
/// value is treated as though it was, and refused.
fn slide_delete(text: &str, span: Range<usize>) -> Range<usize> {
    let bytes = text.as_bytes();

    let (mut lo, mut hi) = (span.start, span.end);
    while lo > 0 && hi > 0 && bytes[lo - 1] == bytes[hi - 1] {
        lo -= 1;
        hi -= 1;
    }

    let (mut low, mut high) = (span.start, span.end);
    while high < bytes.len() && bytes[low] == bytes[high] {
        low += 1;
        high += 1;
    }

    lo..high
}

/// Every position an insertion of `insert` at `at` could equally have been at.
///
/// Sliding an insertion rotates it: inserting `xy` before `xy` and inserting
/// `yx` between the two are the same string, so the byte compared against
/// walks the insertion cyclically rather than staying put.
fn slide_insert(text: &str, at: usize, insert: &str) -> Range<usize> {
    let bytes = text.as_bytes();
    let inserted = insert.as_bytes();
    if inserted.is_empty() {
        return at..at;
    }

    let mut lo = at;
    while lo > 0 {
        let slid = at - lo;
        if bytes[lo - 1] == inserted[inserted.len() - 1 - (slid % inserted.len())] {
            lo -= 1;
        } else {
            break;
        }
    }

    let mut hi = at;
    while hi < bytes.len() {
        if bytes[hi] == inserted[(hi - at) % inserted.len()] {
            hi += 1;
        } else {
            break;
        }
    }

    lo..hi
}

/// A line split into its content and its terminator.
///
/// Everything above works on the content: a `\r\n` in the middle of a grapheme
/// diff is one more thing to get wrong, and the terminator the patch writes
/// comes from the *source* line regardless.
pub(super) fn split_terminator(line: &str) -> (&str, &str) {
    if let Some(body) = line.strip_suffix("\r\n") {
        (body, "\r\n")
    } else if let Some(body) = line.strip_suffix('\n') {
        (body, "\n")
    } else {
        (line, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn spans(line: &str) -> Option<Vec<SourceSpan>> {
        scan(line)
    }

    /// Spans as `(is_expression, text)`, which is all the tests care about.
    fn texts(line: &str) -> Option<Vec<(bool, &str)>> {
        Some(
            spans(line)?
                .iter()
                .map(|span| match span {
                    SourceSpan::Literal(range) => (false, &line[range.clone()]),
                    SourceSpan::Expression(range) => (true, &line[range.clone()]),
                })
                .collect(),
        )
    }

    #[test]
    fn a_line_splits_into_literals_and_expressions() {
        assert_eq!(
            texts("# {{ name }} v1").unwrap(),
            vec![(false, "# "), (true, "{{ name }}"), (false, " v1")]
        );
    }

    /// Every position in the line must fall inside some literal's closed range,
    /// or appending to a line that ends in an expression could not be placed.
    #[test]
    fn a_line_starting_and_ending_with_an_expression_gets_empty_literals() {
        assert_eq!(
            texts("{{ a }}-{{ b }}").unwrap(),
            vec![
                (false, ""),
                (true, "{{ a }}"),
                (false, "-"),
                (true, "{{ b }}"),
                (false, ""),
            ]
        );
    }

    /// A `}}` inside a string literal closes nothing. Cutting there would leave
    /// `{{ "}}` as the expression, which renders as a fragment or not at all.
    #[test]
    fn a_close_delimiter_inside_a_string_does_not_close_the_expression() {
        assert_eq!(
            texts(r#"x {{ "}}" }} y"#).unwrap(),
            vec![(false, "x "), (true, r#"{{ "}}" }}"#), (false, " y")]
        );
    }

    #[rstest]
    // A block tag has no line-local output to attribute an edit to.
    #[case("{% if x %}yes{% endif %}")]
    #[case("{# a comment #}")]
    // Whitespace control reaches outside the line.
    #[case("a {{- x }} b")]
    #[case("a {{ x -}} b")]
    // Unterminated.
    #[case("a {{ x b")]
    // Nested opens.
    #[case("a {{ {{ x }} }} b")]
    // Nothing was substituted, so there is nothing to reverse.
    #[case("plain text")]
    fn a_line_the_scanner_cannot_place_is_refused(#[case] line: &str) {
        assert!(spans(line).is_none(), "{line}");
    }

    #[test]
    fn a_line_keeps_its_terminator_separate() {
        assert_eq!(split_terminator("a\r\n"), ("a", "\r\n"));
        assert_eq!(split_terminator("a\n"), ("a", "\n"));
        assert_eq!(split_terminator("a"), ("a", ""));
    }

    /// The dangerous insertion: `.0` appended to a version sits against the
    /// value, and `similar` is free to place it either side. Attributing it to
    /// the literal gives `{{ version }}.0`, which round-trips perfectly and
    /// appends `.0` to every downstream project.
    #[test]
    fn an_insertion_that_could_have_slid_into_a_value_reaches_the_value() {
        let text = r#"version = "1.0""#;
        // `.0` inserted just after the value, at the closing quote.
        let slid = slide_insert(text, 14, ".0");
        assert_eq!(slid, 12..14);
    }

    #[test]
    fn an_insertion_with_nowhere_to_slide_stays_put() {
        assert_eq!(slide_insert("# acme", 6, "!"), 6..6);
    }

    #[test]
    fn a_deletion_slides_both_ways() {
        // Deleting one `a` from `aaa` could be any of the three.
        assert_eq!(slide_delete("aaa", 1..2), 0..3);
    }
}
