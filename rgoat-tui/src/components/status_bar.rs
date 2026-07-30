//! Bottom status bar widget.
//!
//! Displays a single-line summary of the current session state:
//! mode icon, model name, token usage, elapsed time, and git branch.
//! Black-red theme version.

use ratatui::{
    text::{Line, Span},
    widgets::Paragraph,
};
use std::time::{Duration, Instant};

use crate::components::theme::Theme;

/// Snapshot of data needed to render the status bar.
#[derive(Debug, Clone)]
pub struct StatusBarData {
    /// `"plan"` | `"act"` | `"idle"`
    pub mode: String,
    /// Provider / model identifier, e.g. `"claude-sonnet-4"`
    pub model: String,
    /// Session title (truncated to 20 chars on render).
    pub session_title: String,
    /// Thinking level: "default" | "low" | "medium" | "high" | "max".
    pub thinking_level: String,
    /// Cumulative input tokens for the current request.
    pub input_tokens: u64,
    /// Cumulative output tokens for the current request.
    pub output_tokens: u64,
    /// When the current request began (used for elapsed timer).
    pub started_at: Option<Instant>,
    /// Short git branch name, e.g. `"main"`.
    pub git_branch: String,
    /// Whether the agent is actively processing a request.
    pub is_processing: bool,
    /// Scroll position info, e.g. `"▲ 25/156"` or `"TOP"`. Empty when at bottom.
    pub scroll_info: String,
    /// Focus indicator: "INPUT" or "CHAT".
    pub focus: String,
    /// P2: 排队消息数（>0 时状态栏显示 📨N）。
    pub queued_count: usize,
}

impl StatusBarData {
    /// Create an empty / idle status snapshot.
    pub fn new() -> Self {
        Self {
            mode: "idle".into(),
            model: String::new(),
            session_title: String::new(),
            thinking_level: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            started_at: None,
            git_branch: String::new(),
            is_processing: false,
            scroll_info: String::new(),
            focus: String::from("INPUT"),
            queued_count: 0,
        }
    }

    /// Wall-clock time since `started_at`, or zero if not set.
    pub fn elapsed(&self) -> Duration {
        self.started_at
            .map(|t| t.elapsed())
            .unwrap_or_default()
    }

    /// Human-readable duration string.
    ///
    /// Examples: `"1.2s"`, `"345ms"`.
    fn format_duration(d: Duration) -> String {
        if d.as_secs() > 0 {
            format!("{:.1}s", d.as_secs_f64())
        } else {
            format!("{}ms", d.as_millis())
        }
    }
}

impl Default for StatusBarData {
    fn default() -> Self {
        Self::new()
    }
}

/// Stateless status-bar renderer.
pub struct StatusBar;

impl StatusBar {
    /// Produce a `Paragraph` widget ready for `Frame::render_widget`.
    pub fn render(data: &StatusBarData) -> Paragraph<'static> {
        let elapsed = if data.is_processing {
            StatusBarData::format_duration(data.elapsed())
        } else {
            "-".into()
        };

        let mode_icon = match data.mode.as_str() {
            "plan" => "▣ plan",
            "act" | "agent" => "◉ act",
            "flow" => "◉ flow",
            "yolo" => "◉ yolo",
            "accept-edits" => "◉ edits",
            _ => "◉ idle",
        };

        let tokens = if data.input_tokens > 0 || data.output_tokens > 0 {
            format!(
                "↑{} ↓{}",
                Self::format_tokens(data.input_tokens),
                Self::format_tokens(data.output_tokens)
            )
        } else {
            "↑0 ↓0".into()
        };

        let branch = if data.git_branch.is_empty() {
            "—".into()
        } else {
            data.git_branch.clone()
        };

        let text = Line::from({
            let mut spans = vec![
                Span::styled(" ", Theme::style_status_bar()),
                Span::styled(mode_icon, Theme::style_status_accent()),
                Span::styled(" | ", Theme::style_status_bar()),
                Span::styled(data.model.clone(), Theme::style_status_bar()),
            ];
            if !data.session_title.is_empty() {
                let title = if data.session_title.chars().count() > 20 {
                    format!("{}…", data.session_title.chars().take(19).collect::<String>())
                } else {
                    data.session_title.clone()
                };
                spans.push(Span::styled(" | ", Theme::style_status_bar()));
                spans.push(Span::styled(title, Theme::style_status_accent()));
            }
            if !data.thinking_level.is_empty() {
                spans.push(Span::styled(" | ", Theme::style_status_bar()));
                spans.push(Span::styled(
                    format!("think:{}", data.thinking_level),
                    Theme::style_status_accent(),
                ));
            }
            spans.push(Span::styled(" | ", Theme::style_status_bar()));
            spans.push(Span::styled(tokens, Theme::style_status_bar()));
            spans.push(Span::styled(" | ", Theme::style_status_bar()));
            spans.push(Span::styled(elapsed, Theme::style_status_bar()));
            spans.push(Span::styled(" | ", Theme::style_status_bar()));
            spans.push(Span::styled(branch, Theme::style_status_bar()));
            if !data.scroll_info.is_empty() {
                spans.push(Span::styled(" | ", Theme::style_status_bar()));
                spans.push(Span::styled(data.scroll_info.clone(), Theme::style_status_accent()));
            }
            if data.queued_count > 0 {
                spans.push(Span::styled(" | ", Theme::style_status_bar()));
                spans.push(
                    Span::styled(
                        format!("📨{}", data.queued_count),
                        Theme::style_status_accent(),
                    ),
                );
            }
            spans.push(Span::styled(" | ", Theme::style_status_bar()));
            spans.push(Span::styled(data.focus.clone(), Theme::style_status_accent()));
            spans.push(Span::styled(" ", Theme::style_status_bar()));
            spans
        });

        Paragraph::new(text).style(Theme::style_status_bar())
    }

    /// Format a token count for compact display (e.g. `1500` → `"1.5K"`).
    fn format_tokens(n: u64) -> String {
        if n >= 1000 {
            format!("{:.1}K", n as f64 / 1000.0)
        } else {
            n.to_string()
        }
    }
}
