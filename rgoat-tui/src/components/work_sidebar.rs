//! Work Sidebar widget — renders a right-side progress panel.
//!
//! Displays current step, a text progress bar, recent tool calls, and
//! the latest plan/progress text extracted from Thought events.
//! Toggled with F2.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::components::theme::Theme;

/// Snapshot of data needed to render the work sidebar.
#[derive(Debug, Clone, Default)]
pub struct WorkSidebarData {
    /// Current step number.
    pub current_step: usize,
    /// Total steps (0 means unknown).
    pub total_steps: usize,
    /// Progress text extracted from Thought events (plan/progress markers).
    pub progress_text: String,
    /// Recent tool calls (tool_name + success), FIFO, max 5.
    pub recent_tools: Vec<(String, bool)>,
    /// Whether the agent is actively processing.
    pub is_processing: bool,
}

/// Stateless work-sidebar renderer.
pub struct WorkSidebar;

impl WorkSidebar {
    /// Render the sidebar into `area` using `f`.
    pub fn render(data: &WorkSidebarData, area: Rect, f: &mut Frame) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Theme::BORDER))
            .title(" Work Progress ")
            .title_style(Style::default().fg(Theme::ACCENT_BRIGHT).add_modifier(Modifier::BOLD))
            .style(Style::default().bg(Theme::BG_PANEL));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let dim = Style::default().fg(Theme::STATUS_FG);
        let bold_accent = Style::default().fg(Theme::ACCENT).add_modifier(Modifier::BOLD);
        let mut lines: Vec<Line> = Vec::new();

        // ── Status line ──
        let status_span = if data.is_processing {
            Span::styled("◉ processing", Theme::style_processing())
        } else {
            Span::styled("◎ idle", Style::default().fg(Theme::SYSTEM_COLOR))
        };
        lines.push(Line::from(status_span));
        lines.push(Line::from(""));

        // ── Step / progress bar ──
        let step_line = if data.total_steps > 0 {
            let filled = data.current_step.min(data.total_steps);
            let bar_len = 14usize;
            let done = (filled * bar_len / data.total_steps.max(1)).min(bar_len);
            let bar = format!("[{}{}] {}/{}", "#".repeat(done), "-".repeat(bar_len - done), filled, data.total_steps);
            Line::from(vec![Span::styled("Steps ", dim), Span::styled(bar, Style::default().fg(Theme::ACCENT_BRIGHT))])
        } else {
            Line::from(vec![
                Span::styled("Step ", dim),
                Span::styled(data.current_step.to_string(), Style::default().fg(Theme::ACCENT_BRIGHT)),
            ])
        };
        lines.push(step_line);
        lines.push(Line::from(""));

        // ── Recent tools ──
        lines.push(Line::from(Span::styled("Recent Tools", bold_accent)));
        if data.recent_tools.is_empty() {
            lines.push(Line::from(Span::styled("  (none)", Style::default().fg(Theme::SYSTEM_COLOR))));
        } else {
            // Newest first: iterate in reverse so the most recent tool is on top.
            for (name, ok) in data.recent_tools.iter().rev() {
                let (icon, color) = if *ok { ("✓", Theme::SUCCESS_COLOR) } else { ("✗", Theme::ERROR_COLOR) };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", icon), Style::default().fg(color)),
                    Span::styled(name.clone(), Style::default().fg(Theme::ASSISTANT_COLOR)),
                ]));
            }
        }
        lines.push(Line::from(""));

        // ── Progress text ──
        if !data.progress_text.is_empty() {
            lines.push(Line::from(Span::styled("Progress", bold_accent)));
            for raw in data.progress_text.lines().take(10) {
                lines.push(Line::from(Span::styled(raw.to_string(), dim)));
            }
        }

        let para = Paragraph::new(lines).style(Style::default().bg(Theme::BG_PANEL)).wrap(Wrap { trim: false });
        f.render_widget(para, inner);
    }
}
