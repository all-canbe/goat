//! Diff-view widget — renders syntax-highlighted diffs with scroll support.
//!
//! Displays file edits with colour-coded additions (+), deletions (-),
//! context lines ( ), and hunk headers (@@), plus a vertical scrollbar.

// Diff-view widget is implemented but not yet wired into the main UI; allow
// its (currently unused) public API until it is integrated.
#![allow(dead_code)]

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};

use crate::components::theme::Theme;

// ── DiffViewState ─────────────────────────────────────────────────

/// Mutable scroll state for a diff view.
pub struct DiffViewState {
    /// Current scroll position (top visible line index).
    pub scroll: usize,
    /// Number of lines currently visible in the viewport.
    pub height: usize,
    /// Total number of lines in the diff.
    pub total_lines: usize,
}

impl DiffViewState {
    /// Create a fresh scroll state.
    pub fn new() -> Self {
        Self {
            scroll: 0,
            height: 0,
            total_lines: 0,
        }
    }

    /// Scroll down by one line (clamped to available content).
    pub fn scroll_down(&mut self) {
        if self.scroll + self.height < self.total_lines {
            self.scroll += 1;
        }
    }

    /// Scroll up by one line (clamped to zero).
    pub fn scroll_up(&mut self) {
        if self.scroll > 0 {
            self.scroll -= 1;
        }
    }

    /// Return the current scroll position as a fraction [0.0, 1.0].
    pub fn scroll_percent(&self) -> f64 {
        if self.total_lines <= self.height {
            return 0.0;
        }
        self.scroll as f64 / (self.total_lines - self.height) as f64
    }
}

impl Default for DiffViewState {
    fn default() -> Self {
        Self::new()
    }
}

// ── DiffLine ──────────────────────────────────────────────────────

/// A single annotated line in a unified diff.
#[derive(Debug, Clone)]
pub enum DiffLine {
    /// Added line (green, `+` prefix).
    Add(String),
    /// Deleted line (red, `-` prefix).
    Del(String),
    /// Unchanged context line (dark grey, no prefix).
    Context(String),
    /// Hunk header (cyan, bold, `@@` line).
    Hunk(String),
}

// ── DiffViewData ──────────────────────────────────────────────────

/// The data payload for rendering a diff view.
pub struct DiffViewData {
    /// Display path of the file being diffed.
    pub file_path: String,
    /// Total number of added lines.
    pub additions: usize,
    /// Total number of deleted lines.
    pub deletions: usize,
    /// All lines in the diff.
    pub lines: Vec<DiffLine>,
}

// ── DiffView ──────────────────────────────────────────────────────

/// Stateless renderer for diff views.
pub struct DiffView;

impl DiffView {
    /// Render the diff into the given `area`, updating `state` with
    /// current scroll metrics.
    pub fn render(
        data: &DiffViewData,
        state: &mut DiffViewState,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        state.total_lines = data.lines.len();

        // Build the title block: " <path> +<adds>/-<dels> "
        let title = format!(
            " {} +{}/-{} ",
            data.file_path, data.additions, data.deletions
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Theme::style_diff_hunk())
            .title(title);

        let inner = block.inner(area);
        block.render(area, buf);

        // How many lines fit in the viewport?
        state.height = inner.height as usize;

        // Render visible lines (respecting scroll offset).
        let visible_lines: Vec<Line<'_>> = data
            .lines
            .iter()
            .skip(state.scroll)
            .take(state.height)
            .enumerate()
            .map(|(i, line)| Self::render_line(line, i + state.scroll + 1))
            .collect();

        let para = Paragraph::new(visible_lines);
        para.render(inner, buf);

        // Scrollbar (only when content exceeds viewport).
        if state.total_lines > state.height {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"));

            let mut scroll_state =
                ScrollbarState::new(state.total_lines).position(state.scroll);

            scrollbar.render(inner, buf, &mut scroll_state);
        }
    }

    /// Format a single [`DiffLine`] as a ratatui `Line` with line number.
    fn render_line(line: &DiffLine, line_number: usize) -> Line<'static> {
        let num_str = format!("{:>4}", line_number);

        match line {
            DiffLine::Add(text) => Line::from(vec![
                Span::styled(
                    format!("{} + ", num_str),
                    Style::default().fg(Theme::DIFF_ADD),
                ),
                Span::styled(text.clone(), Theme::style_diff_add()),
            ]),
            DiffLine::Del(text) => Line::from(vec![
                Span::styled(
                    format!("{} - ", num_str),
                    Style::default().fg(Theme::DIFF_DEL),
                ),
                Span::styled(text.clone(), Theme::style_diff_del()),
            ]),
            DiffLine::Context(text) => Line::from(vec![
                Span::styled(
                    format!("{}   ", num_str),
                    Style::default().fg(Theme::DIFF_CTX),
                ),
                Span::styled(text.clone(), Theme::style_diff_ctx()),
            ]),
            DiffLine::Hunk(text) => Line::from(vec![
                Span::styled("     ", Theme::style_diff_hunk()),
                Span::styled(
                    text.clone(),
                    Theme::style_diff_hunk().add_modifier(Modifier::BOLD),
                ),
            ]),
        }
    }
}
