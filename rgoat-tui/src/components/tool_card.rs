//! Tool-card widget — renders collapsible tool invocation panels.
//!
//! Three rendering modes are supported:
//! * `Single`  – full tool card with name, params summary, and syntax-highlighted output
//! * `Timeline` – batch mode showing N tools with duration and total time
//! * `Result`   – compact one-line success/failure indicator

// Tool-card widget is implemented but not yet wired into the main UI; allow
// its (currently unused) public API until it is integrated.
#![allow(dead_code)]

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget, Wrap},
};

use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use crate::components::theme::Theme;

// ── Public types ──────────────────────────────────────────────────

/// A single entry in a tool-timeline list.
pub struct TimelineEntry {
    pub name: String,
    pub success: bool,
    pub duration_ms: u64,
}

/// The rendering mode for a [`ToolCard`].
pub enum ToolCardMode {
    /// Full tool card — name, params summary, and syntax-highlighted output.
    /// Supports collapse / expand via the `collapsed` field.
    Single {
        name: String,
        params_summary: String,
        output: String,
        collapsed: bool,
    },
    /// Batch tool timeline — N tools with per-tool status + total duration.
    Timeline {
        tools: Vec<TimelineEntry>,
        total_ms: u64,
    },
    /// Compact one-line result — ✅/❌ icon + name + summary.
    Result {
        name: String,
        success: bool,
        summary: String,
    },
}

/// Stateless renderer for tool cards.
pub struct ToolCard;

// ── Implementation ────────────────────────────────────────────────

impl ToolCard {
    /// Render the tool card into the given `area`.
    ///
    /// Returns the `Rect` that was actually consumed (always equal to `area`).
    pub fn render(mode: &ToolCardMode, area: Rect, buf: &mut ratatui::buffer::Buffer) -> Rect {
        match mode {
            ToolCardMode::Single { .. } => Self::render_single(mode, area, buf),
            ToolCardMode::Timeline { .. } => Self::render_timeline(mode, area, buf),
            ToolCardMode::Result { .. } => Self::render_result(mode, area, buf),
        }
    }

    /// Estimate the minimum height (in rows) required to render this card.
    pub fn min_height(mode: &ToolCardMode) -> u16 {
        match mode {
            ToolCardMode::Single { collapsed, output, .. } => {
                if *collapsed {
                    3 // border + name line
                } else {
                    3 + output.lines().count().min(10) as u16
                }
            }
            ToolCardMode::Timeline { tools, .. } => {
                3 + tools.len().min(15) as u16
            }
            ToolCardMode::Result { .. } => 3,
        }
    }

    /// Toggle the collapsed state of a `Single`-mode card.
    ///
    /// Does nothing for `Timeline` or `Result` modes.
    pub fn toggle_collapsed(mode: &mut ToolCardMode) {
        if let ToolCardMode::Single { collapsed, .. } = mode {
            *collapsed = !*collapsed;
        }
    }

    // ── Private renderers ─────────────────────────────────────────

    fn render_single(
        mode: &ToolCardMode,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) -> Rect {
        let (name, params_summary, output, is_collapsed) = match mode {
            ToolCardMode::Single {
                name,
                params_summary,
                output,
                collapsed,
            } => (name, params_summary, output, *collapsed),
            _ => return area,
        };

        let expand_icon = if is_collapsed { "▶" } else { "▼" };

        // Build the title line: 🔧 <name> <icon> | <params>
        let title_line = Line::from(vec![
            Span::styled(
                format!("🔧 {} {}", name, expand_icon),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" | {}", params_summary),
                Style::default().fg(Theme::DIFF_CTX),
            ),
        ]);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Theme::style_tool_border())
            .title(title_line);

        let inner = block.inner(area);
        block.render(area, buf);

        // When expanded, render syntax-highlighted output (up to 10 lines).
        if !is_collapsed {
            let ss = SyntaxSet::load_defaults_newlines();
            let ts = ThemeSet::load_defaults();

            let syntax = ss
                .find_syntax_by_extension("txt")
                .unwrap_or_else(|| ss.find_syntax_plain_text());

            let theme = &ts.themes["base16-ocean.dark"];

            let mut highlighter = HighlightLines::new(syntax, theme);

            let highlighted_lines: Vec<Line<'_>> = output
                .lines()
                .take(10)
                .map(|line| {
                    match highlighter.highlight_line(line, &ss) {
                        Ok(ranges) => {
                            let spans: Vec<Span<'_>> = ranges
                                .into_iter()
                                .map(|(style, text)| {
                                    Span::styled(
                                        text.to_string(),
                                        Style::default()
                                            .fg(syntect_color_to_ratatui(style.foreground)),
                                    )
                                })
                                .collect();
                            Line::from(spans)
                        }
                        Err(_) => Line::from(Span::raw(line.to_string())),
                    }
                })
                .collect();

            let output_para = Paragraph::new(highlighted_lines).wrap(Wrap { trim: false });
            output_para.render(inner, buf);
        }

        area
    }

    fn render_timeline(
        mode: &ToolCardMode,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) -> Rect {
        let (tools, total_ms) = match mode {
            ToolCardMode::Timeline { tools, total_ms } => (tools, *total_ms),
            _ => return area,
        };

        let items: Vec<ListItem<'_>> = tools
            .iter()
            .map(|t| {
                let icon = if t.success { "✅" } else { "❌" };
                let text = format!(
                    "{} {} ({:.1}s)",
                    icon,
                    t.name,
                    t.duration_ms as f64 / 1000.0
                );
                ListItem::new(text)
            })
            .collect();

        let total_text = format!("Total: {:.1}s", total_ms as f64 / 1000.0);

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Theme::style_tool_border())
                .title("📊 Tool Timeline")
                .title_bottom(total_text),
        );

        list.render(area, buf);
        area
    }

    fn render_result(
        mode: &ToolCardMode,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) -> Rect {
        let (name, success, summary) = match mode {
            ToolCardMode::Result {
                name,
                success,
                summary,
            } => (name, *success, summary),
            _ => return area,
        };

        let icon = if success { "✅" } else { "❌" };
        let text = format!("{} {}: {}", icon, name, summary);

        let border_color = if success {
            Theme::DIFF_ADD
        } else {
            Theme::DIFF_DEL
        };

        let para = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );

        para.render(area, buf);
        area
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// Convert a `syntect` colour to the equivalent `ratatui` colour.
fn syntect_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}
