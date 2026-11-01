//! Markdown presentation only: note evaluation and terminal sanitisation live elsewhere.

use console::Style;
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};

use super::Theme;

struct Prefix {
    first: String,
    rest: String,
    used: bool,
}

struct Renderer {
    out: String,
    styles: Vec<Style>,
    prefixes: Vec<Prefix>,
}

impl Renderer {
    fn text(&mut self, text: &str) {
        for part in text.split_inclusive('\n') {
            if part == "\n" {
                self.out.push('\n');
                continue;
            }
            if self.out.is_empty() || self.out.ends_with('\n') {
                for prefix in &mut self.prefixes {
                    self.out.push_str(if prefix.used {
                        &prefix.rest
                    } else {
                        &prefix.first
                    });
                    prefix.used = true;
                }
            }
            let body = part.strip_suffix('\n').unwrap_or(part);
            self.out
                .push_str(&self.styles.last().unwrap().apply_to(body).to_string());
            if part.ends_with('\n') {
                self.out.push('\n');
            }
        }
    }

    fn boundary(&mut self, blank: bool) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        if blank && !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn prefix(&mut self, first: String, rest: String) {
        self.prefixes.push(Prefix {
            first,
            rest,
            used: false,
        });
    }
}

pub(super) fn render(theme: &Theme, source: &str) -> String {
    let mut renderer = Renderer {
        out: String::new(),
        styles: vec![Style::new().force_styling(true)],
        prefixes: Vec::new(),
    };
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut links = Vec::new();

    for event in Parser::new(source) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { .. } => {
                    renderer.boundary(true);
                    renderer.styles.push(theme.heading.clone());
                }
                Tag::Paragraph => renderer.boundary(false),
                Tag::Strong => {
                    renderer
                        .styles
                        .push(renderer.styles.last().unwrap().clone().bold());
                }
                Tag::Emphasis => {
                    renderer
                        .styles
                        .push(renderer.styles.last().unwrap().clone().italic());
                }
                Tag::CodeBlock(_) => {
                    renderer.boundary(true);
                    renderer.prefix("    ".into(), "    ".into());
                    renderer.styles.push(theme.command.clone());
                }
                Tag::BlockQuote(_) => {
                    renderer.boundary(false);
                    renderer.prefix("│ ".into(), "│ ".into());
                }
                Tag::List(start) => {
                    renderer.boundary(false);
                    lists.push(start);
                }
                Tag::Item => {
                    renderer.boundary(false);
                    let marker = match lists.last_mut().unwrap() {
                        Some(number) => {
                            let marker = format!("{number}. ");
                            *number = number.saturating_add(1);
                            marker
                        }
                        None => "• ".into(),
                    };
                    let rest = " ".repeat(console::measure_text_width(&marker));
                    renderer.prefix(marker, rest);
                }
                Tag::Link {
                    dest_url,
                    link_type,
                    ..
                }
                | Tag::Image {
                    dest_url,
                    link_type,
                    ..
                } => {
                    // Visible destinations work for relative links too, without
                    // granting Markdown a second producer of OSC hyperlinks.
                    links.push((
                        dest_url,
                        matches!(link_type, LinkType::Autolink | LinkType::Email),
                    ));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) | TagEnd::CodeBlock => {
                    renderer.styles.pop();
                    if tag == TagEnd::CodeBlock {
                        renderer.prefixes.pop();
                    }
                    renderer.boundary(true);
                }
                TagEnd::Strong | TagEnd::Emphasis => {
                    // Restore the enclosing style, not a universal reset: a
                    // bold span inside a heading must still look like a heading.
                    renderer.styles.pop();
                }
                TagEnd::Paragraph => renderer.boundary(true),
                TagEnd::Item | TagEnd::BlockQuote(_) => {
                    renderer.boundary(false);
                    renderer.prefixes.pop();
                }
                TagEnd::List(_) => {
                    lists.pop();
                    renderer.boundary(lists.is_empty());
                }
                TagEnd::Link | TagEnd::Image => {
                    if let Some((destination, automatic)) = links.pop()
                        && !automatic
                    {
                        renderer.text(&format!(" ({destination})"));
                    }
                }
                _ => {}
            },
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                // HTML is visible source, never another interpreted language.
                renderer.text(&text);
            }
            Event::Code(text) => {
                renderer.styles.push(theme.command.clone());
                renderer.text(&text);
                renderer.styles.pop();
            }
            Event::SoftBreak => renderer.text(" "),
            Event::HardBreak => renderer.boundary(false),
            Event::Rule => {
                renderer.boundary(true);
                renderer.text("────────");
                renderer.boundary(true);
            }
            _ => {}
        }
    }
    renderer.out.trim_end_matches('\n').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpl::note::{Formatting, sanitise};

    fn block(source: &str) -> String {
        super::super::note_block(&Theme::colored(), &sanitise(source, Formatting::Allowed))
    }

    #[test]
    fn headings_and_nested_emphasis_restore_the_enclosing_style() {
        let theme = Theme::colored();
        let text = render(&theme, "# Heading **bold *nested* after** end");
        assert_eq!(
            console::strip_ansi_codes(&text),
            "Heading bold nested after end"
        );
        assert!(text.contains(&theme.heading.apply_to(" end").to_string()));
        assert!(text.contains(&theme.heading.clone().bold().apply_to(" after").to_string()));
        assert!(
            text.contains(
                &theme
                    .heading
                    .clone()
                    .bold()
                    .italic()
                    .apply_to("nested")
                    .to_string()
            )
        );
    }

    #[test]
    fn lists_quotes_and_code_keep_their_structure() {
        let text = render(
            &Theme::colored(),
            "3. first\n   - nested\n4. second\n\n> quoted\n\n```sh\n  **literal**\n\nnext\n```",
        );
        let plain = console::strip_ansi_codes(&text);
        assert!(
            plain.contains("3. first\n   • nested\n4. second"),
            "{plain}"
        );
        assert!(plain.contains("│ quoted"), "{plain}");
        assert!(plain.contains("      **literal**\n\n    next"), "{plain}");
    }

    #[test]
    fn links_images_and_html_remain_visible_without_terminal_actions() {
        let text = block(
            "[docs](https://example.com) <https://example.org>\n\n![alt](image.png)\n\n<b>literal</b>",
        );
        let plain = console::strip_ansi_codes(&text);
        assert!(
            plain.contains("docs (https://example.com) https://example.org"),
            "{plain}"
        );
        assert_eq!(plain.matches("https://example.org").count(), 1);
        assert!(plain.contains("alt (image.png)"), "{plain}");
        assert!(plain.contains("<b>literal</b>"), "{plain}");
        assert!(!text.contains("\x1b]"));
    }

    #[test]
    fn plain_presentation_preserves_markdown_source() {
        let source = "# Heading\n\n**bold** and `code`\n- [docs](../README.md)";
        let text = super::super::note_block(&Theme::plain(), source);
        assert!(
            text.contains("  # Heading\n  \n  **bold** and `code`\n  - [docs](../README.md)\n")
        );
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn decoded_entities_cannot_introduce_terminal_controls() {
        let text =
            block("&#27;[2Jgone &#13;rewrite &#8;erase &#x9b;clear\n\n[x](javascript:alert(1))");
        assert!(!text.contains("\x1b[2J"));
        for control in ['\r', '\x08', '\u{9b}'] {
            assert!(!text.contains(control));
        }
        assert!(!text.contains("\x1b]"));
        assert!(text.contains("gone"));
    }

    #[test]
    fn author_styles_and_links_close_before_the_frame() {
        let text = block("\x1b[31mred\n\x1b]8;;https://example.com\x1b\\linked");
        assert!(text.contains("\x1b[31m"));
        assert!(text.contains("\x1b]8;;\x1b\\"));
        let lines: Vec<_> = text.lines().collect();
        assert!(lines[1].ends_with("\x1b[0m"));
        assert!(lines[2].ends_with("\x1b[0m"));
        assert!(!lines.last().unwrap().contains("https://"));
    }

    #[test]
    fn long_unicode_notes_remain_bounded_and_report_truncation() {
        let text = block(&format!(
            "# Heading\n\n{}",
            "界".repeat(tpl::note::LIMIT_BYTES)
        ));
        assert!(text.contains("… (truncated)"));
        // The frame is outside the note's byte budget.
        assert!(text.len() < tpl::note::LIMIT_BYTES + 1024);
    }

    #[test]
    fn markdown_without_visible_content_has_no_frame() {
        assert!(block("[unused]: https://example.com").is_empty());
        assert!(block("&#27;").is_empty());
    }
}
