//! Human diagnostic composition. Miette owns snippets; we own block hierarchy.

use std::{error::Error, fmt};

use miette::{Diagnostic, LabeledSpan, ReportHandler, Severity, SourceCode};

use crate::theme::child;

pub struct Handler {
    colored: bool,
    links: bool,
    width: usize,
}

impl Handler {
    pub fn new(colored: bool, links: bool) -> Self {
        Self {
            colored,
            links,
            width: console::Term::stderr()
                .size_checked()
                .map_or(80, |(_, width)| usize::from(width)),
        }
    }

    fn render(
        &self,
        diagnostic: &dyn Diagnostic,
        source: Option<&dyn SourceCode>,
        depth: usize,
    ) -> String {
        let source = diagnostic.source_code().or(source);
        let width = self.width.saturating_sub(depth * 4).max(12);
        // Keep miette's environment-sensitive graphical/narrated selection and
        // snippet support, but suppress recursion so every child uses our layout.
        let handler = miette::MietteHandlerOpts::new()
            .color(self.colored)
            .unicode(self.colored)
            .width(width)
            .context_lines(2)
            .without_cause_chain()
            .terminal_links(self.links)
            .build();
        let body = Body { diagnostic, source };
        let mut output = format!("{:?}", Rendered(&handler, &body));
        if let Some(help) = diagnostic.help() {
            let help = help.to_string();
            let label = console::Style::new()
                .cyan()
                .force_styling(self.colored)
                .apply_to("help:")
                .to_string();
            // Wrap explicit lines separately: authored lists and source/reason
            // fields must not collapse into a single paragraph.
            for (index, line) in help.split('\n').enumerate() {
                if line.is_empty() {
                    output.push('\n');
                    continue;
                }
                let indentation = line.len() - line.trim_start_matches(' ').len();
                let first = if index == 0 {
                    format!("    {label} {}", " ".repeat(indentation))
                } else {
                    " ".repeat(10 + indentation)
                };
                let rest = " ".repeat(10 + indentation);
                let options = textwrap::Options::new(width)
                    .initial_indent(&first)
                    .subsequent_indent(&rest);
                output.push_str(&textwrap::fill(&line[indentation..], options));
                output.push('\n');
            }
        }
        // Prefer typed diagnostic causes, which carry help and source labels,
        // over the parallel standard Error chain to avoid duplicate reports.
        if let Some(cause) = diagnostic.diagnostic_source() {
            output.push_str(&child(&child("Caused by:\n")));
            output.push_str(&child(&child(&self.render(cause, source, depth + 1))));
        } else {
            let mut cause = diagnostic.source();
            while let Some(error) = cause {
                output.push_str(&child(&child(&format!("Caused by: {error}\n"))));
                cause = error.source();
            }
        }
        if let Some(related) = diagnostic.related() {
            for related in related {
                output.push('\n');
                output.push_str(&child(&child(&self.render(related, source, depth + 1))));
            }
        }
        output
    }
}

impl ReportHandler for Handler {
    fn debug(&self, diagnostic: &dyn Diagnostic, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            return fmt::Debug::fmt(diagnostic, f);
        }
        f.write_str(&self.render(diagnostic, None, 0))
    }
}

struct Rendered<'a>(&'a dyn ReportHandler, &'a dyn Diagnostic);

impl fmt::Debug for Rendered<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.debug(self.1, f)
    }
}

struct Body<'a> {
    diagnostic: &'a dyn Diagnostic,
    source: Option<&'a dyn SourceCode>,
}

impl fmt::Debug for Body<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.diagnostic, f)
    }
}

impl fmt::Display for Body<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.diagnostic, f)
    }
}

// Deliberately no Error::source(): causes are composed once, above.
impl Error for Body<'_> {}

impl Diagnostic for Body<'_> {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.diagnostic.code()
    }

    fn severity(&self) -> Option<Severity> {
        self.diagnostic.severity()
    }

    fn url<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.diagnostic.url()
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.source
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.diagnostic.labels()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("Something failed; literal help: is part of the statement")]
    struct Example {
        help: String,
        related: Vec<Example>,
        cause: Option<Box<Example>>,
        input: Option<String>,
        label: Option<miette::SourceSpan>,
    }

    impl Diagnostic for Example {
        fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
            Some(Box::new(&self.help))
        }

        fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
            Some(Box::new(
                self.related.iter().map(|item| item as &dyn Diagnostic),
            ))
        }

        fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
            self.cause.as_deref().map(|item| item as &dyn Diagnostic)
        }

        fn source_code(&self) -> Option<&dyn SourceCode> {
            self.input.as_ref().map(|input| input as &dyn SourceCode)
        }

        fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
            self.label.map(|span| {
                Box::new(std::iter::once(LabeledSpan::new_with_span(
                    Some("the offending text".into()),
                    span,
                ))) as Box<dyn Iterator<Item = LabeledSpan>>
            })
        }
    }

    fn example(help: &str) -> Example {
        Example {
            help: help.into(),
            related: Vec::new(),
            cause: None,
            input: None,
            label: None,
        }
    }

    fn handler(colored: bool, width: usize) -> Handler {
        Handler {
            colored,
            links: false,
            width,
        }
    }

    #[test]
    fn help_aligns_with_the_statement_in_plain_and_colored_reports() {
        for colored in [false, true] {
            let output = handler(colored, 80).render(&example("Try again"), None, 0);
            let plain = console::strip_ansi_codes(&output);
            assert!(plain.contains("    help: Try again\n"), "{plain}");
            assert!(plain.contains("literal help: is part of the statement"));
            let statement = plain
                .lines()
                .find(|line| line.contains("Something failed"))
                .unwrap();
            assert_eq!(
                console::measure_text_width(statement.split_once("Something failed").unwrap().0),
                4
            );
            assert_eq!(colored, output.contains('\x1b'));
        }
    }

    #[test]
    fn wrapped_help_preserves_explicit_nested_lines() {
        let output = handler(false, 42).render(&example(
            "Review the answers before trying this operation again\n\n  - keep this item nested",
        ), None, 0);
        assert!(
            output.contains(
                "\n          this operation again\n\n            - keep this item nested\n"
            ),
            "{output}"
        );
        assert!(
            output
                .lines()
                .all(|line| console::measure_text_width(line) <= 42)
        );
    }

    #[test]
    fn causes_and_related_help_remain_inside_their_own_blocks() {
        let mut root = example("Root help");
        root.cause = Some(Box::new(example("Cause help")));
        root.related.push(example("Related help"));
        let output = handler(false, 80).render(&root, None, 0);
        assert!(output.contains("    Caused by:\n"));
        assert!(output.contains("        help: Cause help\n"));
        assert!(output.contains("        help: Related help\n"));
        assert_eq!(output.matches("Cause help").count(), 1);
    }

    #[test]
    fn child_labels_keep_the_parent_source_code() {
        let mut root = example("Root help");
        root.input = Some("offending text\n".into());
        let mut related = example("Related help");
        related.label = Some((0, 9).into());
        root.related.push(related);
        let output = handler(false, 80).render(&root, None, 0);
        assert!(output.contains("offending text"), "{output}");
        assert!(output.contains("the offending text"));
    }
}
