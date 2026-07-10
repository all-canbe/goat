//! Global colour constants and style factories for the RGoat TUI.
//!
//! Black-Red theme: dark background with red accents.

// Several palette constants / style factories are part of the public theme API
// but are not yet consumed by the current inline rendering; allow them for now.
#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};

/// Stateless namespace for theme colours and style constructors.
pub struct Theme;

impl Theme {
    // ── Core palette (Black-Red theme) ────────────────────────────

    /// Near-black background.
    pub const BG: Color = Color::Rgb(10, 10, 12);
    /// Slightly lighter black for panels.
    pub const BG_PANEL: Color = Color::Rgb(18, 18, 22);
    /// Border colour for panels.
    pub const BORDER: Color = Color::Rgb(50, 50, 55);
    /// Primary accent — red.
    pub const ACCENT: Color = Color::Rgb(220, 38, 38);
    /// Brighter red for emphasis.
    pub const ACCENT_BRIGHT: Color = Color::Rgb(239, 68, 68);
    /// Dimmed red.
    pub const ACCENT_DIM: Color = Color::Rgb(150, 30, 30);
    /// User message colour — red.
    pub const USER_COLOR: Color = Color::Rgb(248, 113, 113);
    /// Assistant message colour — white/light gray.
    pub const ASSISTANT_COLOR: Color = Color::Rgb(220, 220, 220);
    /// System/info message colour — dark gray.
    pub const SYSTEM_COLOR: Color = Color::Rgb(100, 100, 110);
    /// Error colour — bright red.
    pub const ERROR_COLOR: Color = Color::Rgb(239, 68, 68);
    /// Success colour — green (for contrast).
    pub const SUCCESS_COLOR: Color = Color::Rgb(34, 197, 94);
    /// Warning colour — yellow/orange.
    pub const WARNING_COLOR: Color = Color::Rgb(234, 179, 8);
    /// Tool call border — amber.
    pub const TOOL_BORDER: Color = Color::Rgb(245, 158, 11);
    /// Diff additions — green.
    pub const DIFF_ADD: Color = Color::Rgb(34, 197, 94);
    /// Diff deletions — red.
    pub const DIFF_DEL: Color = Color::Rgb(239, 68, 68);
    /// Diff hunk headers — cyan.
    pub const DIFF_HUNK: Color = Color::Rgb(6, 182, 212);
    /// Diff context — dark gray.
    pub const DIFF_CTX: Color = Color::Rgb(75, 75, 85);
    /// Status bar background — dark.
    pub const STATUS_BG: Color = Color::Rgb(15, 15, 18);
    /// Status bar foreground — light gray.
    pub const STATUS_FG: Color = Color::Rgb(180, 180, 190);
    /// Status bar accent — red.
    pub const STATUS_ACCENT: Color = Color::Rgb(220, 38, 38);
    /// Risk high — red.
    pub const RISK_HIGH: Color = Color::Rgb(239, 68, 68);
    /// Risk medium — yellow.
    pub const RISK_MEDIUM: Color = Color::Rgb(234, 179, 8);
    /// Risk low — green.
    pub const RISK_LOW: Color = Color::Rgb(34, 197, 94);
    /// Approval dialog border — magenta.
    pub const APPROVAL_BORDER: Color = Color::Rgb(192, 38, 211);
    /// Streaming text — italic gray.
    pub const STREAMING_COLOR: Color = Color::Rgb(160, 160, 170);
    /// Thought/process color — dark gray.
    pub const THOUGHT_COLOR: Color = Color::Rgb(100, 100, 110);

    // ── Style factories ───────────────────────────────────────────

    /// Red text for user messages.
    pub fn style_user() -> Style {
        Style::default()
            .fg(Self::USER_COLOR)
            .add_modifier(Modifier::BOLD)
    }

    /// White/light text for assistant messages.
    pub fn style_assistant() -> Style {
        Style::default().fg(Self::ASSISTANT_COLOR)
    }

    /// Dim red for streaming assistant text.
    pub fn style_streaming() -> Style {
        Style::default()
            .fg(Self::STREAMING_COLOR)
            .add_modifier(Modifier::ITALIC)
    }

    /// Dark gray for system/info messages.
    pub fn style_system() -> Style {
        Style::default().fg(Self::SYSTEM_COLOR)
    }

    /// Bright red for errors.
    pub fn style_error() -> Style {
        Style::default()
            .fg(Self::ERROR_COLOR)
            .add_modifier(Modifier::BOLD)
    }

    /// Green for success messages.
    pub fn style_success() -> Style {
        Style::default().fg(Self::SUCCESS_COLOR)
    }

    /// Yellow for warnings/tool calls.
    pub fn style_warning() -> Style {
        Style::default().fg(Self::WARNING_COLOR)
    }

    /// Amber border for tool-card widgets.
    pub fn style_tool_border() -> Style {
        Style::default().fg(Self::TOOL_BORDER)
    }

    /// Magenta border for approval-dialog widgets.
    pub fn style_approval_border() -> Style {
        Style::default().fg(Self::APPROVAL_BORDER)
    }

    /// Bold coloured text for risk-level labels.
    pub fn style_risk(level: &str) -> Style {
        let color = match level.to_uppercase().as_str() {
            "HIGH" => Self::RISK_HIGH,
            "MEDIUM" => Self::RISK_MEDIUM,
            _ => Self::RISK_LOW,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    /// Dark background with red accent for status bar.
    pub fn style_status_bar() -> Style {
        Style::default().fg(Self::STATUS_FG).bg(Self::STATUS_BG)
    }

    /// Red accent for status bar highlights.
    pub fn style_status_accent() -> Style {
        Style::default()
            .fg(Self::STATUS_ACCENT)
            .bg(Self::STATUS_BG)
            .add_modifier(Modifier::BOLD)
    }

    /// Green text for added lines in diffs.
    pub fn style_diff_add() -> Style {
        Style::default().fg(Self::DIFF_ADD)
    }

    /// Red text for deleted lines in diffs.
    pub fn style_diff_del() -> Style {
        Style::default().fg(Self::DIFF_DEL)
    }

    /// Cyan text for hunk headers in diffs.
    pub fn style_diff_hunk() -> Style {
        Style::default().fg(Self::DIFF_HUNK)
    }

    /// Dark-gray text for unchanged context lines in diffs.
    pub fn style_diff_ctx() -> Style {
        Style::default().fg(Self::DIFF_CTX)
    }

    /// Thought/process text style.
    pub fn style_thought() -> Style {
        Style::default()
            .fg(Self::THOUGHT_COLOR)
            .add_modifier(Modifier::ITALIC)
    }

    /// Panel/block background style.
    pub fn style_panel() -> Style {
        Style::default().bg(Self::BG_PANEL)
    }

    /// Title bar style — dark bg with red accent.
    pub fn style_title_bar() -> Style {
        Style::default()
            .fg(Self::ACCENT_BRIGHT)
            .bg(Self::BG_PANEL)
            .add_modifier(Modifier::BOLD)
    }

    /// Input area style.
    pub fn style_input() -> Style {
        Style::default().fg(Self::ASSISTANT_COLOR).bg(Self::BG)
    }

    /// Input prompt style (mode indicator).
    pub fn style_input_prompt() -> Style {
        Style::default()
            .fg(Self::ACCENT)
            .add_modifier(Modifier::BOLD)
    }

    /// Processing/spinner style.
    pub fn style_processing() -> Style {
        Style::default()
            .fg(Self::ACCENT_BRIGHT)
            .add_modifier(Modifier::BOLD)
    }
}
