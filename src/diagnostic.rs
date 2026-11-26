//! Presentation-only diagnostic decoration, before miette performs layout.

use std::{error::Error, fmt};

use miette::{Diagnostic, LabeledSpan, ReportHandler, Severity, SourceCode};

use crate::theme::{Theme, prose};

pub struct Handler {
    pub inner: miette::MietteHandler,
    pub theme: Theme,
}

impl ReportHandler for Handler {
    fn debug(&self, error: &dyn Diagnostic, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Alternate Debug is an inspection of the original error, not UI prose.
        if f.alternate() || !self.theme.is_colored() {
            self.inner.debug(error, f)
        } else {
            self.inner.debug(&Presented::new(error, &self.theme), f)
        }
    }

    fn track_caller(&mut self, location: &'static std::panic::Location<'static>) {
        self.inner.track_caller(location);
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
}
