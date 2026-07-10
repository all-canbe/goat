//! Markdown renderer for terminal TUI display.
//!
//! Converts Markdown text into ratatui `Line`/`Span` sequences with
//! basic typography support: headings, bold, italic, code blocks
//! (syntax-highlighted via syntect), inline code, lists, tables, and rules.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

// ── Public API ─────────────────────────────────────────────────────

/// Render a single Markdown paragraph/block into ratatui Lines.
pub struct MarkdownRenderer;

impl MarkdownRenderer {
    /// Render Markdown text into a `Vec<Line>` suitable for a chat area.
    ///
    /// `width` is the available character width; code blocks and tables
    /// will be wrapped to fit.
    pub fn render(text: &str, width: u16) -> Vec<Line<'static>> {
        let mut renderer = RenderState::new(width as usize);
        renderer.render_document(text);
        renderer.lines
    }
}

// ── Internal render state ──────────────────────────────────────────

struct RenderState {
    lines: Vec<Line<'static>>,
    width: usize,
    /// Buffered spans for the current line.
    current: Vec<Span<'static>>,
    /// Tracking for list items (depth, bullet).
    list_depth: Vec<u8>,
    /// Tracking for table rendering.
    table_cells: Vec<Vec<String>>,
    table_header: Vec<String>,
    in_table_head: bool,
    /// Whether we are inside a code block.
    in_code_block: bool,
    code_lang: String,
    code_lines: Vec<String>,
    /// Whether we are inside a paragraph.
    in_paragraph: bool,
    /// Whether we are inside a heading.
    in_heading: Option<HeadingLevel>,
    /// Whether we are inside a table cell.
    in_table_cell: bool,
    /// Current table cell text buffer.
    cell_buf: String,
    /// Current table row cells.
    row_cells: Vec<String>,
    /// Current inline style overrides.
    bold: bool,
    italic: bool,
    code: bool,
}

impl RenderState {
    fn new(width: usize) -> Self {
        Self {
            lines: Vec::new(),
            width,
            current: Vec::new(),
            list_depth: Vec::new(),
            table_cells: Vec::new(),
            table_header: Vec::new(),
            in_table_head: false,
            in_code_block: false,
            code_lang: String::new(),
            code_lines: Vec::new(),
            in_paragraph: false,
            in_heading: None,
            in_table_cell: false,
            cell_buf: String::new(),
            row_cells: Vec::new(),
            bold: false,
            italic: false,
            code: false,
        }
    }

    fn render_document(&mut self, text: &str) {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

        let parser = Parser::new_ext(text, options);

        for event in parser {
            match event {
                Event::Start(tag) => self.handle_start(tag),
                Event::End(tag) => self.handle_end(tag),
                Event::Text(t) => self.handle_text(t.into()),
                Event::Code(t) => self.handle_code(t.into()),
                Event::Html(_) | Event::InlineHtml(_) => {
                    // ignore raw HTML
                }
                Event::FootnoteReference(_) => {}
                Event::InlineMath(_) | Event::DisplayMath(_) => {
                    // ignore math for now
                }
                Event::SoftBreak => {
                    // soft break → newline
                    self.flush_line();
                }
                Event::HardBreak => {
                    self.flush_line();
                    self.push_empty_line();
                }
                Event::Rule => {
                    self.flush_paragraph();
                    let rule: String = std::iter::repeat('─').take(self.width.min(40)).collect();
                    self.lines.push(Line::from(Span::styled(
                        rule,
                        Style::default().fg(Color::Rgb(80, 80, 90)),
                    )));
                    self.push_empty_line();
                }
                Event::TaskListMarker(_) => {
                    // ignore
                }
            }
        }

        // Flush any remaining state
        self.flush_code_block();
        self.flush_paragraph();
        self.flush_table();
    }

    // ── Tag handlers ────────────────────────────────────────────

    fn handle_start(&mut self, tag: Tag) {
        match tag {
            // Block-level
            Tag::Paragraph => {
                self.in_paragraph = true;
            }
            Tag::Heading { level, .. } => {
                self.flush_paragraph();
                self.in_heading = Some(level);
            }
            Tag::CodeBlock(kind) => {
                self.flush_paragraph();
                self.in_code_block = true;
                self.code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                self.code_lines.clear();
            }
            Tag::List(..) => {
                self.flush_paragraph();
            }
            Tag::Item => {
                // render bullet
                self.flush_paragraph();
                let depth = self.list_depth.len();
                let indent = "  ".repeat(depth);
                let bullet = if depth == 0 { "•" } else { "◦" };
                self.current.push(Span::styled(
                    format!("{}{} ", indent, bullet),
                    Style::default().fg(Color::Rgb(200, 200, 210)),
                ));
            }
            Tag::Table(_) => {
                self.flush_paragraph();
                self.table_cells.clear();
                self.table_header.clear();
                self.in_table_head = true;
            }
            Tag::TableHead => {
                self.in_table_head = true;
            }
            Tag::TableRow => {
                self.row_cells.clear();
            }
            Tag::TableCell => {
                self.in_table_cell = true;
                self.cell_buf.clear();
            }
            // Inline
            Tag::Emphasis => {
                self.italic = true;
            }
            Tag::Strong => {
                self.bold = true;
            }
            Tag::Strikethrough => {
                // not supported, ignore
            }
            Tag::Link { .. } | Tag::Image { .. } => {
                // links/images: render as plain text, ignore URL
            }
            _ => {}
        }
    }

    fn handle_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_paragraph();
            }
            TagEnd::Heading(_) => {
                self.flush_heading();
                self.in_heading = None;
            }
            TagEnd::CodeBlock => {
                self.flush_code_block();
                self.in_code_block = false;
                self.code_lang.clear();
            }
            TagEnd::List(_) => {
                self.push_empty_line();
            }
            TagEnd::Item => {
                self.flush_line();
            }
            TagEnd::Table => {
                self.flush_table();
                self.push_empty_line();
            }
            TagEnd::TableHead => {
                self.in_table_head = false;
            }
            TagEnd::TableRow => {
                if self.in_table_head {
                    self.table_header = self.row_cells.clone();
                } else {
                    self.table_cells.push(self.row_cells.clone());
                }
            }
            TagEnd::TableCell => {
                self.in_table_cell = false;
                self.row_cells.push(self.cell_buf.clone());
            }
            TagEnd::Emphasis => {
                self.italic = false;
            }
            TagEnd::Strong => {
                self.bold = false;
            }
            TagEnd::Strikethrough => {}
            TagEnd::Link | TagEnd::Image => {}
            _ => {}
        }
    }

    fn handle_text(&mut self, text: std::borrow::Cow<'_, str>) {
        if self.in_code_block {
            self.code_lines.push(text.into_owned());
            return;
        }
        if self.in_table_cell {
            self.cell_buf.push_str(&text);
            return;
        }
        let mut style = Style::default();
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.code {
            style = style.bg(Color::Rgb(50, 50, 55));
        }
        if let Some(level) = self.in_heading {
            style = match level {
                HeadingLevel::H1 => style.add_modifier(Modifier::BOLD).fg(Color::Rgb(6, 182, 212)),
                HeadingLevel::H2 => style.add_modifier(Modifier::BOLD).fg(Color::Rgb(255, 255, 255)),
                _ => style.add_modifier(Modifier::BOLD).fg(Color::Rgb(200, 200, 210)),
            };
        }
        self.current.push(Span::styled(text.into_owned(), style));
    }

    fn handle_code(&mut self, text: std::borrow::Cow<'_, str>) {
        if self.in_code_block {
            self.code_lines.push(text.into_owned());
            return;
        }
        // Inline code
        self.current.push(Span::styled(
            text.into_owned(),
            Style::default()
                .bg(Color::Rgb(50, 50, 55))
                .fg(Color::Rgb(230, 230, 230)),
        ));
    }

    // ── Flush helpers ───────────────────────────────────────────

    fn flush_paragraph(&mut self) {
        if !self.current.is_empty() {
            self.flush_line();
        }
        if self.in_paragraph {
            self.push_empty_line();
        }
        self.in_paragraph = false;
    }

    fn flush_heading(&mut self) {
        if self.current.is_empty() {
            return;
        }
        // H1 gets an underline
        let is_h1 = matches!(self.in_heading, Some(HeadingLevel::H1));
        self.flush_line();
        if is_h1 {
            let ruler: String = std::iter::repeat('─').take(self.width.min(30)).collect();
            self.lines.push(Line::from(Span::styled(
                ruler,
                Style::default().fg(Color::Rgb(6, 182, 212)),
            )));
        }
        self.push_empty_line();
    }

    fn flush_code_block(&mut self) {
        if self.code_lines.is_empty() {
            return;
        }
        let lines = std::mem::take(&mut self.code_lines);

        // Try syntax highlighting via syntect
        let ss = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();
        let syntax = ss
            .find_syntax_by_token(&self.code_lang)
            .or_else(|| ss.find_syntax_by_extension(&self.code_lang))
            .unwrap_or_else(|| ss.find_syntax_plain_text());
        let theme = &ts.themes["base16-ocean.dark"];

        let mut highlighter = HighlightLines::new(syntax, theme);
        let max_lines = 20usize;

        for line in lines.iter().take(max_lines) {
            match highlighter.highlight_line(line, &ss) {
                Ok(ranges) => {
                    let spans: Vec<Span> = ranges
                        .into_iter()
                        .map(|(style, text)| {
                            Span::styled(
                                text.to_string(),
                                Style::default().fg(syntect_color_to_ratatui(style.foreground)),
                            )
                        })
                        .collect();
                    // Prefix with " │ " for code block visual
                    let mut line_spans = vec![Span::styled(
                        " │ ",
                        Style::default().fg(Color::Rgb(80, 80, 90)),
                    )];
                    line_spans.extend(spans);
                    self.lines.push(Line::from(line_spans));
                }
                Err(_) => {
                    self.lines.push(Line::from(vec![
                        Span::styled(" │ ", Style::default().fg(Color::Rgb(80, 80, 90))),
                        Span::raw(line.clone()),
                    ]));
                }
            }
        }
        if lines.len() > max_lines {
            self.lines.push(Line::from(Span::styled(
                format!(" │ … {} more lines", lines.len() - max_lines),
                Style::default().fg(Color::Rgb(80, 80, 90)),
            )));
        }
        self.lines.push(Line::from(""));
    }

    fn flush_table(&mut self) {
        if self.table_cells.is_empty() && self.table_header.is_empty() {
            return;
        }

        // Compute column widths
        let col_count = self.table_header.len().max(
            self.table_cells.first().map(|r| r.len()).unwrap_or(0),
        );
        if col_count == 0 {
            return;
        }
        if col_count > 3 {
            // Too many columns for terminal → render as list
            for row in &self.table_cells {
                for (i, cell) in row.iter().enumerate() {
                    let label = if i < self.table_header.len() {
                        &self.table_header[i]
                    } else {
                        ""
                    };
                    self.lines.push(Line::from(vec![
                        Span::styled(
                            format!("  {}: ", label),
                            Style::default().fg(Color::Rgb(200, 200, 210)),
                        ),
                        Span::raw(cell.clone()),
                    ]));
                }
                self.lines.push(Line::from(""));
            }
            return;
        }

        let mut col_widths = vec![0usize; col_count];
        for (i, h) in self.table_header.iter().enumerate() {
            col_widths[i] = col_widths[i].max(UnicodeWidthStr::width(h.as_str()));
        }
        for row in &self.table_cells {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(UnicodeWidthStr::width(cell.as_str()));
                }
            }
        }
        // Cap columns to fit
        let separator_width = 3 * (col_count - 1) + 4; // " │ " between cols + padding
        let total_width: usize = col_widths.iter().sum::<usize>() + separator_width;
        if total_width > self.width {
            let scale = self.width as f64 / total_width as f64;
            for w in &mut col_widths {
                *w = ((*w as f64) * scale).max(4.0) as usize;
            }
        }

        let header_fmt = |cells: &[String]| -> Line {
            let mut spans = vec![Span::styled("  ", Style::default())];
            for (i, cell) in cells.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", Style::default().fg(Color::Rgb(80, 80, 90))));
                }
                let w = col_widths.get(i).copied().unwrap_or(10);
                let padded = format!("{:<width$}", cell, width = w);
                spans.push(Span::styled(padded, Style::default().add_modifier(Modifier::BOLD)));
            }
            Line::from(spans)
        };

        let row_fmt = |cells: &[String]| -> Line {
            let mut spans = vec![Span::styled("  ", Style::default())];
            for (i, cell) in cells.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", Style::default().fg(Color::Rgb(80, 80, 90))));
                }
                let w = col_widths.get(i).copied().unwrap_or(10);
                let padded = format!("{:<width$}", cell, width = w);
                spans.push(Span::raw(padded));
            }
            Line::from(spans)
        };

        if !self.table_header.is_empty() {
            self.lines.push(header_fmt(&self.table_header));
            let sep: String = col_widths
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let s = "─".repeat(*w);
                    if i == 0 { s } else { format!("─┼─{}", s) }
                })
                .collect::<Vec<_>>()
                .join("");
            self.lines.push(Line::from(Span::styled(
                format!("  {}", sep),
                Style::default().fg(Color::Rgb(80, 80, 90)),
            )));
        }
        for row in &self.table_cells {
            self.lines.push(row_fmt(row));
        }
    }

    fn flush_line(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.current);
        self.lines.push(Line::from(spans));
    }

    fn push_empty_line(&mut self) {
        self.lines.push(Line::from(""));
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn syntect_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}