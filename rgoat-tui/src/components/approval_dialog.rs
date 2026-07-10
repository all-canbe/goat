//! Approval-dialog widget — interactive modal prompt for tool-approval requests.
//!
//! Renders a centered, bordered dialog that displays the tool details and
//! risk level, then captures user input (y / a / n / Esc) to produce an
//! [`ApprovalDecision`].
//!
//! Implemented as part of task **T04**.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};
use rgoat_core::security::approval::ApprovalDecision;

use crate::components::theme::Theme;

// ── Data types ──────────────────────────────────────────────────────────────

/// Approval-request data extracted from `AgentEvent::ApprovalRequired`.
pub struct ApprovalRequest {
    pub tool_name: String,
    pub tool_type: String,   // "file_write" | "shell" | "network" | "other"
    pub summary: String,
    pub risk_level: String,  // "HIGH" | "MEDIUM" | "LOW"
    pub command: String,
    pub path: String,
    pub url: String,
}

/// Dialog selection state.
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalChoice {
    /// User chose `y` — approve this one call only.
    #[allow(dead_code)]
    ApproveOnce,
    /// User chose `a` — approve this and all subsequent calls.
    #[allow(dead_code)]
    ApproveAll,
    /// User chose `n` or `Esc` — reject this call.
    #[allow(dead_code)]
    Reject,
    /// Awaiting user keypress.
    Pending,
}

// ── Dialog component ────────────────────────────────────────────────────────

/// Stateless approval-dialog renderer and key handler.
pub struct ApprovalDialog;

impl ApprovalDialog {
    /// Compute a centred rectangle for the dialog (70 % × 50 % of screen).
    pub fn dialog_area(screen: Rect) -> Rect {
        let width = (screen.width as f32 * 0.7) as u16;
        let height = (screen.height as f32 * 0.5) as u16;

        let x = (screen.width.saturating_sub(width)) / 2;
        let y = (screen.height.saturating_sub(height)) / 2;

        Rect::new(
            x.saturating_add(screen.x),
            y.saturating_add(screen.y),
            width,
            height,
        )
    }

    /// Render the approval dialog directly into the given buffer.
    ///
    /// The caller is responsible for clearing the area beforehand (via
    /// `Clear.render(area, buf)`) and for placing the dialog on top of the
    /// normal chat layout.
    pub fn render(
        request: &ApprovalRequest,
        choice: &ApprovalChoice,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        // ── Background clear ──
        Clear.render(area, buf);

        // ── Risk style ──
        let risk_label = match request.risk_level.to_uppercase().as_str() {
            "HIGH" => "🔴 HIGH",
            "MEDIUM" => "🟡 MEDIUM",
            _ => "🟢 LOW",
        };
        let risk_style = Theme::style_risk(&request.risk_level);

        // ── Block with border ──
        let title = format!("⚡ Approval Required — {}", request.tool_name);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Theme::style_approval_border())
            .title(Line::from(vec![Span::styled(
                title,
                Style::default().add_modifier(Modifier::BOLD),
            )]));

        let inner = block.inner(area);
        block.render(area, buf);

        // ── Content layout (9 rows) ──
        let content = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // risk level + type
                Constraint::Length(1), // spacer
                Constraint::Min(3),    // summary (wrapped)
                Constraint::Length(1), // spacer
                Constraint::Length(1), // detail: command
                Constraint::Length(1), // detail: path
                Constraint::Length(1), // detail: url
                Constraint::Length(1), // spacer
                Constraint::Length(1), // key hints
            ])
            .split(inner);

        // Row 0: risk label + tool type
        let mut header_spans = vec![
            Span::styled(risk_label, risk_style),
            Span::raw("  |  "),
            Span::styled(
                format!("Type: {}", request.tool_type),
                Style::default().fg(Theme::DIFF_CTX),
            ),
        ];

        // For HIGH risk, add an extra warning line
        let header_text = if request.risk_level.to_uppercase().as_str() == "HIGH" {
            header_spans.push(Span::raw("\n"));
            header_spans.push(Span::styled(
                " ⚠ This operation could modify files or execute commands!",
                Style::default()
                    .fg(Theme::RISK_HIGH)
                    .add_modifier(Modifier::BOLD),
            ));
            Line::from(header_spans)
        } else {
            Line::from(header_spans)
        };
        Paragraph::new(header_text).render(content[0], buf);

        // Row 2: summary (wrapped)
        let summary_para = Paragraph::new(request.summary.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Theme::ASSISTANT_COLOR));
        summary_para.render(content[2], buf);

        // Rows 4-6: detail lines (conditionally rendered)
        let mut detail_idx = 4;
        if !request.command.is_empty() {
            let cmd_line = Line::from(vec![
                Span::styled("  Command: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    &request.command,
                    Style::default().fg(Theme::USER_COLOR),
                ),
            ]);
            Paragraph::new(cmd_line).render(content[detail_idx], buf);
            detail_idx += 1;
        }
        if !request.path.is_empty() {
            let path_line = Line::from(vec![
                Span::styled("  Path:    ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    &request.path,
                    Style::default().fg(Theme::USER_COLOR),
                ),
            ]);
            Paragraph::new(path_line).render(content[detail_idx], buf);
            detail_idx += 1;
        }
        if !request.url.is_empty() {
            let url_line = Line::from(vec![
                Span::styled("  URL:     ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    &request.url,
                    Style::default().fg(Theme::USER_COLOR),
                ),
            ]);
            Paragraph::new(url_line).render(content[detail_idx], buf);
        }

        // Row 8: key hints
        let hint_spans: Vec<Span> = match choice {
            ApprovalChoice::Pending => vec![
                Span::styled(
                    "[y] Approve Once  ",
                    Style::default()
                        .fg(Theme::DIFF_ADD)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "[a] Approve All   ",
                    Style::default()
                        .fg(Theme::RISK_LOW)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "[n] Reject        ",
                    Style::default()
                        .fg(Theme::DIFF_DEL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("| "),
                Span::styled("Esc=reject", Style::default().fg(Theme::DIFF_CTX)),
            ],
            _ => vec![Span::styled(
                "Processing...",
                Style::default().fg(Theme::DIFF_CTX),
            )],
        };

        Paragraph::new(Line::from(hint_spans)).render(content[8], buf);
    }

    /// Translate a keypress into an approval decision, if the key is relevant.
    ///
    /// Returns `None` for keys that are not y / a / n / Esc (the caller
    /// should then ignore the key while the dialog is active).
    #[allow(dead_code)]
    pub fn handle_key(key: char) -> Option<ApprovalDecision> {
        match key.to_ascii_lowercase() {
            'y' => Some(ApprovalDecision {
                approved: true,
                approve_all: false,
            }),
            'a' => Some(ApprovalDecision {
                approved: true,
                approve_all: true,
            }),
            'n' | '\x1b' => Some(ApprovalDecision {
                approved: false,
                approve_all: false,
            }),
            _ => None,
        }
    }
}
