//! Human diagnostic composition with inline highlighting before layout.

use std::{error::Error, fmt};

use miette::{Diagnostic, LabeledSpan, ReportHandler, Severity, SourceCode};

use crate::theme::{Theme, child, prose};

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
    fn debug(&self, error: &dyn Diagnostic, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Alternate Debug is an inspection of the original error, not UI prose.
        if f.alternate() {
            fmt::Debug::fmt(error, f)
        } else if self.colored {
            // Decorate before composing blocks so wrapping measures visible
            // text, and literal source snippets never pass through the formatter.
            let theme = Theme::colored();
            f.write_str(&self.render(&Presented::new(error, &theme), None, 0))
        } else {
            f.write_str(&self.render(error, None, 0))
        }
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

#[derive(Debug)]
struct Presented<'a> {
    original: &'a dyn Diagnostic,
    theme: &'a Theme,
    cause: Option<Box<Presented<'a>>>,
    error_source: Option<ErrorText>,
    related: Option<Vec<Presented<'a>>>,
}

impl<'a> Presented<'a> {
    fn new(original: &'a dyn Diagnostic, theme: &'a Theme) -> Self {
        Self {
            original,
            theme,
            cause: original
                .diagnostic_source()
                .map(|cause| Box::new(Self::new(cause, theme))),
            error_source: original
                .source()
                .map(|source| ErrorText::new(source, theme)),
            related: original
                .related()
                .map(|related| related.map(|item| Self::new(item, theme)).collect()),
        }
    }
}

impl fmt::Display for Presented<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&prose(self.theme, &self.original.to_string()))
    }
}

impl Error for Presented<'_> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.error_source
            .as_ref()
            .map(|source| source as &dyn Error)
    }
}

impl Diagnostic for Presented<'_> {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.original.code()
    }

    fn severity(&self) -> Option<Severity> {
        self.original.severity()
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.original
            .help()
            .map(|help| Box::new(prose(self.theme, &help.to_string())) as Box<dyn fmt::Display>)
    }

    fn url<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.original.url()
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.original.source_code()
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.original.labels().map(|labels| {
            Box::new(labels.map(|mut label| {
                label.set_label(label.label().map(|text| prose(self.theme, text)));
                label
            })) as Box<dyn Iterator<Item = LabeledSpan>>
        })
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        self.related.as_ref().map(|related| {
            Box::new(related.iter().map(|item| item as &dyn Diagnostic))
                as Box<dyn Iterator<Item = &dyn Diagnostic>>
        })
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        self.cause.as_deref().map(|cause| cause as &dyn Diagnostic)
    }
}

// Error::source requires a 'static object. Own just the displayed ordinary
// cause chain rather than leaking ANSI into the underlying library errors.
#[derive(Debug)]
struct ErrorText {
    message: String,
    source: Option<Box<ErrorText>>,
}

impl ErrorText {
    fn new(error: &dyn Error, theme: &Theme) -> Self {
        Self {
            message: prose(theme, &error.to_string()),
            source: error
                .source()
                .map(|source| Box::new(Self::new(source, theme))),
        }
    }
}

impl fmt::Display for ErrorText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ErrorText {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|source| source as &dyn Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miette::{GraphicalReportHandler, GraphicalTheme, NamedSource, SourceSpan};
    use thiserror::Error;

    // A fixture code outside the public catalogue, which scans source files too.
    #[derive(Debug, Error, Diagnostic)]
    #[error("could not load `template`")]
    #[diagnostic(
        code(example::failed),
        severity(Warning),
        url("https://example.com"),
        help("try `git tpl lint`")
    )]
    struct Example {
        #[source_code]
        source: NamedSource<String>,
        #[label(primary, "invalid `value`")]
        span: SourceSpan,
        #[source]
        #[diagnostic_source]
        cause: Cause,
        #[related]
        related: Vec<Cause>,
    }

    #[derive(Debug, Error, Diagnostic)]
    #[error("missing `input`")]
    #[diagnostic(help("provide `input`"))]
    struct Cause(#[source] std::io::Error);

    fn example() -> Example {
        Example {
            source: NamedSource::new("example.toml", "key = `literal`\n".to_owned()),
            span: (6, 9).into(),
            cause: Cause(std::io::Error::other("cannot read `source`")),
            related: vec![Cause(std::io::Error::other("cannot read `related`"))],
        }
    }

    #[test]
    fn presentation_preserves_diagnostic_metadata_and_source_bytes() {
        let original = example();
        let theme = Theme::colored();
        let shown = Presented::new(&original, &theme);
        assert_eq!(
            shown.code().unwrap().to_string(),
            original.code().unwrap().to_string()
        );
        assert_eq!(
            shown.url().unwrap().to_string(),
            original.url().unwrap().to_string()
        );
        assert_eq!(shown.severity(), Some(Severity::Warning));
        let label = shown.labels().unwrap().next().unwrap();
        assert_eq!(label.offset(), 6);
        assert_eq!(label.len(), 9);
        assert!(label.primary());
        assert_eq!(label.label(), Some("invalid \x1b[36mvalue\x1b[0m"));
        let source = shown
            .source_code()
            .unwrap()
            .read_span(label.inner(), 0, 0)
            .unwrap();
        assert_eq!(source.data(), b"`literal`");
        assert!(shown.to_string().contains("\x1b[36mtemplate\x1b[0m"));
        assert!(
            shown
                .help()
                .unwrap()
                .to_string()
                .contains("\x1b[36mgit tpl lint\x1b[0m")
        );
        let cause = shown.diagnostic_source().unwrap();
        assert!(cause.to_string().contains("\x1b[36minput\x1b[0m"));
        assert!(
            cause
                .source()
                .unwrap()
                .to_string()
                .contains("\x1b[36msource\x1b[0m")
        );
        let related = shown.related().unwrap().next().unwrap();
        assert!(
            related
                .source()
                .unwrap()
                .to_string()
                .contains("\x1b[36mrelated\x1b[0m")
        );
        assert!(!original.to_string().contains('\x1b'));
    }

    #[test]
    fn miette_wraps_highlighted_prose_without_interpreting_source_snippets() {
        let original = example();
        let theme = Theme::colored();
        let shown = Presented::new(&original, &theme);
        let mut output = String::new();
        GraphicalReportHandler::new_themed(GraphicalTheme::unicode())
            .with_width(32)
            .render_report(&mut output, &shown)
            .unwrap();
        let text = console::strip_ansi_codes(&output);
        assert!(text.contains("key = `literal`"), "{text}");
        assert!(!text.contains("`template`"), "{text}");
        assert!(!text.contains("`input`"), "{text}");
        assert!(!text.contains("`source`"), "{text}");
        assert!(!text.contains("`related`"), "{text}");
        assert!(text.contains("git tpl lint"), "{text}");
        assert!(output.contains("\x1b[36mtemplate\x1b[0m"), "{output}");
    }

    #[test]
    fn ambiguous_backticks_in_diagnostic_paths_remain_literal() {
        let theme = Theme::colored();
        for path in ["a`b`c", "café`文件`名", "a`b"] {
            let message = format!("failed to render `{path}`");
            let original = miette::MietteDiagnostic::new(&message);
            let shown = Presented::new(&original, &theme);
            assert_eq!(shown.to_string(), message);
        }
    }

    #[test]
    fn inline_highlighting_preserves_help_alignment_in_nested_diagnostic_blocks() {
        for colored in [false, true] {
            let handler = Handler {
                colored,
                links: false,
                width: 80,
            };
            let original = example();
            let output = format!("{:?}", Rendered(&handler, &original));
            let plain = console::strip_ansi_codes(&output);
            let command = if colored {
                "git tpl lint"
            } else {
                "`git tpl lint`"
            };
            let input = if colored { "input" } else { "`input`" };
            assert!(
                plain.contains(&format!("    help: try {command}\n")),
                "{plain}"
            );
            assert!(
                plain.contains(&format!("        help: provide {input}\n")),
                "{plain}"
            );
            assert!(plain.contains("key = `literal`"), "{plain}");
            assert_eq!(
                output.contains("\x1b[36mtemplate\x1b[0m"),
                colored,
                "{output}"
            );
            assert_eq!(
                output.contains("\x1b[36msource\x1b[0m"),
                colored,
                "{output}"
            );
        }
    }
}

#[cfg(test)]
mod layout_tests {
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
