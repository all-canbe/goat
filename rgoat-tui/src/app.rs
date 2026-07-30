//! TUI application — full Agent integration
//!
//! Layout:
//! ┌───────────────────────────────────────────────────┐
//! │  RGoat v0.1  |  session:xxx  |  mode  |  Ctrl-C  │
//! ├───────────────────────────────────────────────────┤
//! │  You > hello world                                │
//! │  Goat > I'm thinking...                           │
//! │  [tool] read_file: /src/main.rs                   │
//! │  [tool] ✓ result (12 lines)                       │
//! │  Goat > Here's what I found...                    │
//! │  [approval] Shell requires confirm → allowed      │
//! ├───────────────────────────────────────────────────┤
//! │  💤 idle | claude-sonnet-4 | ↑0 ↓0 | - | main    │
//! ├───────────────────────────────────────────────────┤
//! │ > ▌                                               │
//! └───────────────────────────────────────────────────┘

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::execute;
use crossterm::cursor::{Show, Hide, SetCursorStyle};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::sync::broadcast;
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;

use rgoat_core::agent::react::ReActAgent;
use rgoat_core::agent::types::AgentEvent;
use rgoat_core::agent::checkpoint::Checkpoint;
use rgoat_core::conversation::manager::ConversationManager;
use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::core::event_bus::Event as BusEvent;
use rgoat_core::provider::switch::ProviderSwitch;
use rgoat_core::provider::provider::LlmProvider;
use rgoat_core::provider::provider::{ChatOptions, ThinkingLevel};
use rgoat_core::security::approval::{AgentMode, ApprovalDecision, ApprovalResponder, ApprovalScope};

use crate::components::approval_dialog::{ApprovalChoice, ApprovalDialog, ApprovalRequest};
use crate::components::settings_dialog::{ItemKind, SettingsDialog, SettingsItem};
use crate::keybindings::{load_keybindings, render_hotkeys, KeyMap};

/// TUI focus target — determines which area receives navigation keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusTarget {
    Input,
    Chat,
}
use crate::components::theme::Theme;
use crate::components::status_bar::{StatusBar, StatusBarData};
use crate::components::markdown::MarkdownRenderer;

// ── UI Element (decoded from AgentEvent for display) ──

/// A single display element in the conversation scrollback.
///
/// A few fields/variants (e.g. `DiffView`, `params_summary`, `step`) are
/// populated by event handlers but not yet rendered on every code path; they
/// are retained as part of the display model and intentionally allowed to be
/// unused for now.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum UiElement {
    User { text: String },
    Assistant { text: String },
    /// Mutable streaming text — appended word-by-word via MessageDelta events.
    AssistantStream { text: String },
    Thought { step: usize, content: String, collapsed: bool },
    /// Merged tool call + result — single card for the complete tool lifecycle.
    ToolCall {
        name: String,
        success: bool,
        summary: String,
        output: String,
        collapsed: bool,
        elapsed_ms: u64,
        output_line_count: usize,
    },
    DiffView {
        path: String,
        additions: usize,
        deletions: usize,
        lines: Vec<DiffLine>,
    },
    ToolTimeline { tools: Vec<(String, bool, u64)>, total_ms: u64 },
    Approval { tool: String, decision: String },
    AskUser { question: String, header: String },
    System { text: String },
    Error { text: String },
}

/// A single line within a `DiffView` element.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum DiffLine {
    Add(String),
    Del(String),
    Context(String),
    Hunk(String),
}

impl UiElement {
    /// Render this element as a single `Line` for the chat list (compact mode).
    fn render_compact(&self, width: u16) -> Line<'static> {
        let w = width.saturating_sub(4) as usize;
        match self {
            UiElement::User { text } => Line::from(vec![
                Span::styled("You ▶ ", Theme::style_user()),
                Span::raw(clip(text, w)),
            ]),
            UiElement::Assistant { text } => Line::from(vec![
                Span::styled("Goat ▶ ", Theme::style_assistant().add_modifier(Modifier::BOLD)),
                Span::raw(clip(text, w)),
            ]),
            UiElement::AssistantStream { text } => Line::from(vec![
                Span::styled("Goat ▶ ", Theme::style_assistant().add_modifier(Modifier::BOLD)),
                Span::styled(clip(text, w), Theme::style_streaming()),
            ]),
            UiElement::Thought { content, collapsed, .. } => {
                if *collapsed {
                    Line::from(vec![
                        Span::styled("  ▸ 💭 ", Theme::style_thought()),
                        Span::styled("Thinking", Theme::style_thought()),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled("  💭 ", Theme::style_thought()),
                        Span::styled(clip(content, w), Theme::style_thought()),
                    ])
                }
            }
            UiElement::ToolCall { name, collapsed, output_line_count, .. } => {
                let hint = if *collapsed {
                    format!(" [{} lines]", output_line_count)
                } else {
                    String::new()
                };
                Line::from(vec![
                    Span::styled("  🔧 ", Theme::style_warning()),
                    Span::styled(name.clone(), Theme::style_warning().add_modifier(Modifier::BOLD)),
                    Span::styled(hint, Style::default().fg(Color::Rgb(120, 120, 130))),
                ])
            }
            UiElement::DiffView { path, additions, deletions, .. } => {
                let summary = format!("{} (+{}/−{})", path, additions, deletions);
                Line::from(vec![
                    Span::styled("  📄 ", Style::default().fg(Color::Rgb(6, 182, 212))),
                    Span::raw(clip(&summary, w)),
                ])
            }
            UiElement::ToolTimeline { tools, total_ms } => {
                let summary: String = tools
                    .iter()
                    .map(|(name, ok, ms)| {
                        let icon = if *ok { "✓" } else { "✗" };
                        format!("{}{}({}ms)", icon, name, ms)
                    })
                    .collect::<Vec<_>>()
                    .join(" → ");
                Line::from(vec![
                    Span::styled("  ⏱ ", Style::default().fg(Color::Rgb(234, 179, 8))),
                    Span::styled(format!("{} ({}ms total)", clip(&summary, w.saturating_sub(20)), total_ms), Style::default().fg(Color::Rgb(150, 150, 160))),
                ])
            }
            UiElement::Approval { tool, decision } => Line::from(vec![
                Span::styled("  🛡 ", Style::default().fg(Color::Rgb(192, 38, 211))),
                Span::styled(tool.clone(), Style::default().fg(Color::Rgb(192, 38, 211))),
                Span::raw(" → "),
                Span::styled(decision.clone(), Style::default().fg(Color::Rgb(192, 38, 211)).add_modifier(Modifier::BOLD)),
            ]),
            UiElement::AskUser { question, header } => Line::from(vec![
                Span::styled(format!("❓ [{}] ", header), Style::default().fg(Color::Rgb(6, 182, 212)).add_modifier(Modifier::BOLD)),
                Span::styled(question.clone(), Style::default().fg(Color::Rgb(120, 120, 130))),
            ]),
            UiElement::System { text } => Line::from(vec![
                Span::styled("[sys] ", Style::default().fg(Color::Rgb(234, 179, 8))),
                Span::styled(clip(text, w), Theme::style_system()),
            ]),
            UiElement::Error { text } => Line::from(vec![
                Span::styled("  ⚠ ", Theme::style_error()),
                Span::styled(clip(text, w), Theme::style_error()),
            ]),
        }
    }
}

// ── Message Queue (P2: pi 风格消息队列) ──

/// 排队消息的种类。steering 优先于 follow_up 交付。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QKind {
    /// 在当前 agent 轮次结束后立即交付（高优先级）。
    Steering,
    /// 在所有 steering 之后再交付（普通排队）。
    FollowUp,
}

/// 等待交付的用户消息。
#[derive(Debug, Clone)]
struct QueuedMessage {
    text: String,
    kind: QKind,
}

// ── Application State ──

struct App {
    agent: Arc<ReActAgent>,
    conversation: Arc<ConversationManager>,
    event_rx: broadcast::Receiver<BusEvent>,
    switch: Arc<ProviderSwitch>,
    review_provider: Option<Arc<dyn LlmProvider>>,

    // Session
    session_id: String,
    session_title: String,
    workspace: String,

    // Chat
    lines: Vec<UiElement>,
    input: String,
    mode: AgentMode,
    thinking_level: ThinkingLevel,
    is_processing: bool,
    current_step: usize,
    max_steps: usize,

    // Scroll — distance from the bottom of rendered chat_lines.
    // 0 = pinned to bottom (follow new messages); >0 = scrolled up by N lines.
    // Mirrors CodeWhale's approach: scroll-from-bottom is always relative to
    // total chat_lines, so position stays stable when new messages arrive.
    scroll_from_bottom: usize,

    // TUI focus: Input or Chat (TUI-3)
    focus: FocusTarget,

    // Scroll hint: transient message shown after scroll actions (TUI-5)
    scroll_hint: String,
    scroll_hint_expiry: Option<Instant>,

    // Search state (TUI-9): None when inactive, Some when searching
    search_query: String,
    search_active: bool,
    search_matches: Vec<(usize, usize)>, // (element_index, line_in_chat)
    search_current: usize,

    // Element→line mapping (TUI-8/9/10): populated during render
    element_line_starts: Vec<usize>,

    // Pending scroll target line (TUI-9): set by search, applied in render
    pending_scroll_to_line: Option<usize>,

    // Status
    status_msg: String,
    current_provider: String,

    // ── New fields for T02 ──

    /// Cumulative input tokens for the current request.
    input_tokens: u64,
    /// Cumulative output tokens for the current request.
    output_tokens: u64,
    /// When the current request began (None if idle).
    processing_start: Option<Instant>,
    /// Short git branch name for the workspace.
    git_branch: String,
    /// Channel for sending approval decisions back to the agent.
    approval_responder: ApprovalResponder,
    /// Active approval dialog: request data + current choice state.
    approval_dialog: Option<(ApprovalRequest, ApprovalChoice)>,
    /// Consecutive Ctrl-C presses (used for double-tap exit / cancel).
    ctrl_c_count: u8,
    pending_plan: Option<String>,
    /// M9: 取消传播 — TUI Ctrl+C 通过此 token 通知 Agent 停止
    cancel_token: CancellationToken,
    /// Index into `self.lines` of the current streaming AssistantStream element.
    streaming_idx: Option<usize>,
    /// Whether to show the command menu (triggered by `/` at start of input).
    show_command_menu: bool,
    /// Current cursor position (char index, byte-safe) within input.
    input_cursor: usize,
    /// Sticky visual column for vertical cursor movement (对齐 pi preferredVisualCol)。
    /// 上下移动时保持视觉列；左右移动/插入/删除后重置为 None。
    preferred_visual_col: Option<usize>,
    // ── P2: Kill ring (Emacs-style Ctrl+K/Y/Alt+Y)，对齐 pi editor.ts ──
    /// Kill ring，容量 10，最新在前。Ctrl+K 压入，Ctrl+Y 弹出，Alt+Y 轮转。
    kill_ring: Vec<String>,
    /// 上次 yank 的 (start_char, end_char)，用于 Alt+Y 替换。
    last_yank: Option<(usize, usize)>,
    // ── P2: Undo (Ctrl+Z)，对齐 pi editor.ts ──
    /// Undo 栈，容量 50，存 (text, cursor) 快照。
    undo_stack: Vec<(String, usize)>,
    /// 上次操作类型，用于 undo coalescing（连续 word char 输入合并为一个 undo 单元）。
    last_action_is_word_char: bool,
    // ── P2: History (Up/Down)，对齐 pi editor.ts navigateHistory ──
    /// 输入历史，容量 100，最新在前。
    history: Vec<String>,
    /// 当前浏览的历史索引，None = 不在浏览模式。
    history_index: Option<usize>,
    /// 进入 history 浏览前的草稿，Down 回到末尾时恢复。
    history_draft: Option<String>,
    /// P1: track tool execution start times for elapsed time calculation
    tool_start_times: HashMap<String, Instant>,
    /// Index of selected item in the command menu (-1 = none).
    command_menu_index: usize,
    /// Pre-loaded skills for /findskill command menu integration
    loaded_skills: Vec<(String, String)>,

    // ── D4: Work Sidebar (toggled with F2) ──
    /// Whether the work progress sidebar is visible.
    show_work_sidebar: bool,
    /// Progress text extracted from Thought events (plan/progress markers).
    sidebar_progress_text: String,
    /// Recent tool calls (tool_name + success), FIFO, max 5.
    sidebar_recent_tools: Vec<(String, bool)>,
    /// Current step number for the sidebar.
    sidebar_current_step: usize,
    /// Total step count for the sidebar (0 = unknown).
    sidebar_total_steps: usize,

    // ── P2: Message Queue ──
    /// 排队的用户消息（Enter 处理中 → steering；Alt+Enter → follow-up）。
    message_queue: Vec<QueuedMessage>,
    /// Finished 事件后待提交的 prompt（由 handle_agent_event 设置，事件循环消费）。
    /// 用此字段把 sync 的 handle_agent_event 与 async 的 submit_prompt 解耦。
    pending_submit: Option<String>,

    // ── T1-#4: File picker (@ file reference) ──
    file_picker_active: bool,
    file_picker_query: String,
    file_picker_matches: Vec<PathBuf>,
    file_picker_index: usize,
    /// Char index of the '@' trigger character in the input string.
    /// 语义与 `input_cursor` 一致（char index），所有 byte 操作通过 char_indices 转换。
    file_picker_trigger_pos: usize,

    // ── T3-#15: Settings dialog ──
    settings_dialog: Option<SettingsDialog>,

    // ── T3-#17: Keybindings (loaded from ~/.goat/keybindings.json) ──
    keybindings: Option<KeyMap>,
}

/// Helper: load skills from filesystem, returning (name, description) pairs for the command menu.
fn load_skills_for_menu(workspace: &str) -> Vec<(String, String)> {
    use rgoat_core::agent::react::ReActAgent;
    let formatted = ReActAgent::load_skills(workspace);
    formatted
        .iter()
        .filter_map(|s| {
            // Format: "- **name**: description"
            let inner = s.strip_prefix("- **")?;
            let (name, desc) = inner.split_once("**: ")?;
            Some((name.to_string(), desc.to_string()))
        })
        .collect()
}

/// Load find-skills SKILL.md content for the /findskill command.
fn find_skills_content() -> String {
    // Try multiple locations for find-skills SKILL.md
    let locations = [
        // Built-in skill in rgoat-tui crate
        concat!(env!("CARGO_MANIFEST_DIR"), "/../skills/find-skills/SKILL.md"),
        // Project skills
    ];
    for loc in &locations {
        if let Ok(content) = std::fs::read_to_string(loc) {
            return content;
        }
    }
    // Fallback: inline the key instructions
    "\
# Find Skills

Use `npx skills find [query]` to search for skills.
Use `npx skills add <package> -g -y` to install skills.
Check https://skills.sh/ for popular skills.
"
        .to_string()
}

impl App {
    fn new(
        agent: Arc<ReActAgent>,
        conversation: Arc<ConversationManager>,
        event_rx: broadcast::Receiver<BusEvent>,
        workspace: String,
        mode: AgentMode,
        switch: Arc<ProviderSwitch>,
        approval_responder: ApprovalResponder,
        cancel_token: CancellationToken,
        review_provider: Option<Arc<dyn LlmProvider>>,
    ) -> Self {
        // Attempt to resolve the git branch for the workspace directory.
        let git_branch = git2::Repository::open(&workspace)
            .ok()
            .and_then(|repo| {
                let head = repo.head().ok()?;
                head.shorthand().map(String::from)
            })
            .unwrap_or_default();

        Self {
            agent,
            conversation,
            event_rx,
            switch,
            review_provider,
            session_id: String::new(),
            session_title: String::new(),
            workspace,
            lines: Vec::new(),
            input: String::new(),
            mode,
            thinking_level: ThinkingLevel::Default,
            is_processing: false,
            current_step: 0,
            max_steps: 50,
            scroll_from_bottom: 0,
            focus: FocusTarget::Input,
            scroll_hint: String::new(),
            scroll_hint_expiry: None,
            search_query: String::new(),
            search_active: false,
            search_matches: Vec::new(),
            search_current: 0,
            element_line_starts: Vec::new(),
            pending_scroll_to_line: None,
            status_msg: String::from("Ready — type /help for commands"),
            current_provider: String::new(),
            // ── T02 new fields ──
            input_tokens: 0,
            output_tokens: 0,
            processing_start: None,
            git_branch,
            approval_responder,
            approval_dialog: None,
            ctrl_c_count: 0,
            pending_plan: None,
            cancel_token,
            streaming_idx: None,
            show_command_menu: false,
            input_cursor: 0,
            preferred_visual_col: None,
            kill_ring: Vec::new(),
            last_yank: None,
            undo_stack: Vec::new(),
            last_action_is_word_char: false,
            history: Vec::new(),
            history_index: None,
            history_draft: None,
            tool_start_times: HashMap::new(),
            command_menu_index: 0,
            loaded_skills: Vec::new(),
            // ── D4 ──
            show_work_sidebar: false,
            sidebar_progress_text: String::new(),
            sidebar_recent_tools: Vec::new(),
            sidebar_current_step: 0,
            sidebar_total_steps: 0,
            // ── P2 ──
            message_queue: Vec::new(),
            pending_submit: None,
            // ── T1-#4 ──
            file_picker_active: false,
            file_picker_query: String::new(),
            file_picker_matches: Vec::new(),
            file_picker_index: 0,
            file_picker_trigger_pos: 0,
            // ── T3 ──
            settings_dialog: None,
            keybindings: load_keybindings(),
        }
    }

    async fn init(&mut self) {
        self.current_provider = self.switch.current_name().await;

        // Pre-load skills for /findskill command menu
        self.loaded_skills = load_skills_for_menu(&self.workspace);

        match self.conversation.create_session(None, Some(&self.workspace)).await {
            Ok(s) => {
                self.session_id = s.id;
                self.session_title = s.title.clone();
                self.add_line(UiElement::System {
                    text: format!("Session started. Mode: {}", self.mode_display()),
                });
                // D2: 崩溃恢复检测 — 扫描所有会话，检测上次是否有异常终止
                if let Some(cp) = Checkpoint::detect_any_crash(&self.workspace).await {
                    self.add_line(UiElement::System {
                        text: format!(
                            "⚠ 检测到上次会话异常终止（步骤 {}）。使用 /resume {} 恢复。",
                            cp.step, cp.session_id
                        ),
                    });
                }
            }
            Err(e) => {
                self.add_line(UiElement::Error { text: format!("Session error: {}", e) });
            }
        }
    }

    fn mode_display(&self) -> &str {
        match self.mode {
            AgentMode::Agent => "agent",
            AgentMode::Plan => "plan",
            AgentMode::Flow => "flow",
            AgentMode::AcceptEdits => "accept-edits",
            AgentMode::Yolo => "yolo",
        }
    }

    fn cycle_mode(&mut self) {
        let next = match self.mode {
            AgentMode::Plan => AgentMode::Agent,
            AgentMode::Agent => AgentMode::Yolo,
            AgentMode::Yolo => AgentMode::Flow,
            AgentMode::Flow => AgentMode::AcceptEdits,
            AgentMode::AcceptEdits => AgentMode::Plan,
        };
        self.mode = next;
        self.agent.set_mode(next);
        self.status_msg = format!("Mode: {}", self.mode_display());
    }

    fn cycle_thinking(&mut self) {
        self.thinking_level = match self.thinking_level {
            ThinkingLevel::Default => ThinkingLevel::Low,
            ThinkingLevel::Low => ThinkingLevel::Medium,
            ThinkingLevel::Medium => ThinkingLevel::High,
            ThinkingLevel::High => ThinkingLevel::Max,
            ThinkingLevel::Max => ThinkingLevel::Default,
        };
        self.set_scroll_hint(format!("Thinking: {:?}", self.thinking_level));
    }

    /// Return the current provider/model name for the status bar.
    fn model_name(&self) -> Option<String> {
        if self.current_provider.is_empty() {
            None
        } else {
            Some(self.current_provider.clone())
        }
    }

    /// Build a `StatusBarData` snapshot from current app state.
    fn status_data(&self) -> StatusBarData {
        StatusBarData {
            mode: if self.is_processing {
                self.mode.to_string()
            } else {
                "idle".into()
            },
            model: self.model_name().unwrap_or_default(),
            session_title: self.session_title.clone(),
            thinking_level: format!("{:?}", self.thinking_level).to_lowercase(),
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            started_at: self.processing_start,
            git_branch: self.git_branch.clone(),
            is_processing: self.is_processing,
            scroll_info: String::new(),
            focus: String::new(),
            queued_count: self.message_queue.len(),
        }
    }

    /// Compute scroll info string for status bar.
    /// Returns "" when at bottom, "TOP" at top, "▲ start+1/total" in middle.
    fn compute_scroll_info(&self, total_lines: usize, visible_height: usize) -> String {
        if total_lines <= visible_height {
            return String::new();
        }
        let max_scroll = total_lines.saturating_sub(visible_height);
        if self.scroll_from_bottom == 0 {
            return String::new(); // at bottom
        }
        if self.scroll_from_bottom >= max_scroll {
            return String::from("TOP");
        }
        let start = max_scroll.saturating_sub(self.scroll_from_bottom);
        format!("▲ {}/{}", start + 1, total_lines)
    }

    fn add_line(&mut self, line: UiElement) {
        self.lines.push(line);
        // scroll_from_bottom=0 means user is at the bottom; new lines
        // will naturally push the viewport forward (no action needed).
    }

    /// P2: 把消息加入队列，并在 chat 区显示一行提示。
    fn enqueue_message(&mut self, kind: QKind, text: String) {
        let preview = clip(&text.replace('\n', " "), 30);
        let label = match kind {
            QKind::Steering => "steering",
            QKind::FollowUp => "follow-up",
        };
        self.add_line(UiElement::System {
            text: format!("📨 Queued ({}): {}", label, preview),
        });
        self.message_queue.push(QueuedMessage { text, kind });
        self.status_msg = format!("📨 {} queued ({} total)", label, self.message_queue.len());
    }

    /// P2: 把队列中所有消息合并回 input 框（用于中断后恢复）。
    /// 若 input 已有草稿，以换行分隔追加。
    fn restore_queue_to_input(&mut self) {
        if self.message_queue.is_empty() {
            return;
        }
        let restored: String = self.message_queue
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if self.input.is_empty() {
            self.input = restored;
        } else {
            self.input.push('\n');
            self.input.push_str(&restored);
        }
        self.input_cursor = self.input.chars().count();
        let n = self.message_queue.len();
        self.message_queue.clear();
        self.add_line(UiElement::System {
            text: format!("📨 Restored {} queued message(s) to input", n),
        });
    }

    /// P2: 从队列末尾取回一条消息到 input 框（Alt+Up）。返回是否成功。
    fn pop_queue_to_input(&mut self) -> bool {
        if let Some(msg) = self.message_queue.pop() {
            self.input = msg.text;
            self.input_cursor = self.input.chars().count();
            self.show_command_menu = self.input.starts_with('/');
            self.status_msg = format!(
                "📨 Popped queued message ({} left)",
                self.message_queue.len()
            );
            true
        } else {
            self.status_msg = "No queued messages".to_string();
            false
        }
    }

    /// Insert pasted text at the cursor position, advancing the cursor by the
    /// number of characters inserted.  Caller is responsible for length guards
    /// and NUL/CRLF sanitization.  Multi-line pastes preserve newlines so the
    /// editor supports true multi-line input (对齐 pi editor.ts bracketed paste)。
    fn handle_paste(&mut self, text: &str) {
        let char_count = self.input.chars().count();
        if self.input_cursor > char_count {
            self.input_cursor = char_count;
        }
        self.push_undo();
        self.last_action_is_word_char = false;
        let byte_pos = self.input
            .char_indices()
            .nth(self.input_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len());
        self.input.insert_str(byte_pos, text);
        self.input_cursor += text.chars().count();
        self.show_command_menu = self.input.starts_with('/');
    }

    /// 在 cursor 处插入换行符 `\n`，cursor 前进到新行首。
    /// 用于 Shift+Enter / `\+Enter` / 多行 paste 的换行插入。
    /// 对齐 pi editor.ts `addNewLine`。
    fn insert_newline_at_cursor(&mut self) {
        let char_count = self.input.chars().count();
        if self.input_cursor > char_count {
            self.input_cursor = char_count;
        }
        self.push_undo();
        self.last_action_is_word_char = false;
        let byte_pos = self.input
            .char_indices()
            .nth(self.input_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len());
        self.input.insert(byte_pos, '\n');
        self.input_cursor += 1;
        self.show_command_menu = false;
    }

    /// 压入 kill ring（容量 10，最新在前）。对齐 pi KillRing。
    fn push_kill_ring(&mut self, text: String) {
        if text.is_empty() { return; }
        self.kill_ring.insert(0, text);
        if self.kill_ring.len() > 10 { self.kill_ring.pop(); }
    }

    /// 压入 undo 快照（容量 50）。coalescing: 连续 word char 输入合并。
    fn push_undo(&mut self) {
        self.undo_stack.push((self.input.clone(), self.input_cursor));
        if self.undo_stack.len() > 50 { self.undo_stack.remove(0); }
    }

    /// 添加输入历史（容量 100，去重连续重复）。对齐 pi addToHistory。
    fn add_history(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() { return; }
        if self.history.first().map(|s| s.as_str()) == Some(trimmed) { return; }
        self.history.insert(0, trimmed.to_string());
        if self.history.len() > 100 { self.history.pop(); }
    }

    /// History 导航：-1 = Up（更旧），1 = Down（更新）。对齐 pi navigateHistory。
    fn navigate_history(&mut self, direction: i32) {
        if self.history.is_empty() { return; }
        let new_index = match self.history_index {
            None => {
                if direction == -1 {
                    // 首次进入 history，保存草稿
                    self.history_draft = Some(self.input.clone());
                    Some(0)
                } else {
                    return; // Down 但未浏览，无操作
                }
            }
            Some(idx) => {
                let new_idx = idx as i32 + direction;
                if new_idx < 0 { return; }
                if new_idx as usize >= self.history.len() {
                    // 超出范围，恢复草稿
                    self.history_index = None;
                    if let Some(draft) = self.history_draft.take() {
                        self.input = draft;
                        self.input_cursor = self.input.chars().count();
                    }
                    return;
                }
                Some(new_idx as usize)
            }
        };
        self.history_index = new_index;
        if let Some(idx) = new_index {
            self.input = self.history[idx].clone();
            self.input_cursor = self.input.chars().count();
            self.show_command_menu = self.input.starts_with('/');
        }
        self.preferred_visual_col = None;
        self.last_action_is_word_char = false;
    }

    async fn submit(&mut self) {
        let prompt = std::mem::take(&mut self.input);
        // ★ 无论走哪条路径，先重置输入相关状态
        self.input_cursor = 0;
        self.show_command_menu = false;
        self.command_menu_index = 0;

        if prompt.trim().is_empty() {
            return;
        }

        // P2: 记录输入历史（供 Up/Down 召回）
        self.add_history(&prompt);
        self.history_index = None;
        self.history_draft = None;

        // Shell passthrough: !!command (hidden) or !command (visible) — T1-#3
        if prompt.starts_with("!!") {
            let cmd = prompt[2..].trim().to_string();
            self.run_shell_hidden(&cmd).await;
            return;
        }
        if prompt.starts_with('!') {
            let cmd = prompt[1..].trim().to_string();
            self.run_shell_visible(&cmd).await;
            return;
        }

        // Handle slash commands locally (skip bare "/" — it opens the menu, isn't a command)
        if prompt.starts_with('/') && prompt.len() > 1 {
            self.handle_command(&prompt).await;
            return;
        }

        // Bare "/": just show the menu without treating it as a command
        if prompt == "/" {
            self.show_command_menu = true;
            return;
        }

        if self.is_processing {
            // P2: 处理中再按 Enter → 排队为 steering 消息（而非拒绝）。
            self.enqueue_message(QKind::Steering, prompt);
            return;
        }

        // Phase 3: 处理用户计划选择 (a/y/n)
        if let Some(plan_content) = self.pending_plan.take() {
            let choice = prompt.trim().to_lowercase();
            let exec_prompt = format!(
                "请按照以下计划逐步执行：\n\n{}",
                plan_content
            );
            match choice.as_str() {
                "a" => {
                    self.mode = AgentMode::Agent;
                    self.agent.set_mode(AgentMode::Agent);
                    self.add_line(UiElement::System { text: "🤖 Switched to Agent mode — step-by-step execution.".into() });
                    self.add_line(UiElement::User { text: "Execute plan (Agent mode)".into() });
                    self.submit_prompt(&exec_prompt).await;
                    return;
                }
                "y" => {
                    self.mode = AgentMode::Yolo;
                    self.agent.set_mode(AgentMode::Yolo);
                    self.add_line(UiElement::System { text: "🤖 Switched to YOLO mode — auto execution.".into() });
                    self.add_line(UiElement::User { text: "Execute plan (YOLO mode)".into() });
                    self.submit_prompt(&exec_prompt).await;
                    return;
                }
                "n" => {
                    self.add_line(UiElement::System { text: "📝 Continuing plan modification...".into() });
                    // 保持 Plan 模式，继续提交用户输入
                    self.add_line(UiElement::User { text: prompt.clone() });
                    self.submit_plan(&prompt).await;
                    return;
                }
                _ => {
                    // 非选择输入 → 放回 pending_plan，当作普通 prompt 处理
                    self.pending_plan = Some(plan_content);
                    self.add_line(UiElement::System { text: "Please choose: (a) Agent, (y) YOLO, or (n) Continue".into() });
                    return;
                }
            }
        }

        self.add_line(UiElement::User { text: prompt.clone() });
        // Plan mode: use PlanRunner for Explore→Plan→Phase 3
        if self.mode == AgentMode::Plan {
            self.submit_plan(&prompt).await;
        // Flow mode: use FlowPipeline for implement→review→fix loop
        } else if self.mode == AgentMode::Flow {
            if self.status_msg == "Mode: flow_plan" {
                self.submit_flow_plan(&prompt).await;
            } else {
                self.submit_flow(&prompt).await;
            }
        } else {
            self.submit_prompt(&prompt).await;
        }
    }

    /// Submit in Flow mode: implement → review → fix pipeline
    async fn submit_flow(&mut self, prompt: &str) {
        use rgoat_core::agent::flow::FlowPipeline;
        self.is_processing = true;
        self.status_msg = "Flow: implementing...".to_string();
        self.processing_start = Some(Instant::now());
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.streaming_idx = None;
        self.ctrl_c_count = 0;

        // 使用独立 review provider（若配置），否则 fallback 到主 provider
        let review_provider: Arc<dyn LlmProvider> = self.review_provider.clone()
            .unwrap_or_else(|| self.agent.provider.clone());

        // 创建 review agent（Flow 模式，非 Plan，以匹配审批自动批准行为）
        let mut review_config = self.agent.config.clone();
        review_config.max_steps = 10;
        let review_agent = Arc::new(ReActAgent::new(
            review_config,
            review_provider,
            self.agent.tools.clone(),
            self.agent.approval.clone(),
            self.agent.conversation.clone(),
            self.agent.event_bus.clone(),
            self.agent.cancellation.clone(),
            AgentMode::Flow,
            self.agent.paused.clone(),
            self.agent.approval_responder.clone(),
        ));

        let mut pipeline = FlowPipeline::new(self.agent.clone(), review_agent);

        // 设置 Gate 回调（通过 event_bus 发射事件，TUI 可响应）
        // Gate 回调返回 true 表示继续，false 表示暂停
        let event_bus = self.agent.event_bus.clone();
        let gate_callback: rgoat_core::agent::flow::GateCallback = Arc::new(move |_phase, info| {
            // 发射事件告知用户 Gate 状态
            let msg = match info.phase {
                rgoat_core::agent::flow::GatePhase::ImplComplete => {
                    format!("Gate: Implementation complete. {} | Verified: {} | Findings: {}",
                        info.changes, info.verified, info.findings_count)
                }
                rgoat_core::agent::flow::GatePhase::ReviewFindings => {
                    format!("Gate: Review found {} issue(s), {} critical. Verified: {}",
                        info.findings_count, info.critical_count, info.verified)
                }
                rgoat_core::agent::flow::GatePhase::PlanCompare => {
                    "Gate: Plan comparison — choose A (original) or B (reviewed)".to_string()
                }
            };
            event_bus.emit(
                rgoat_core::core::event_bus::EventType::FlowRoundComplete,
                "flow",
                serde_json::json!({ "gate_message": msg }),
            );
            "continue".to_string() // 默认继续（不阻塞）
        });
        pipeline.set_gate_callback(gate_callback);

        let session_id = self.session_id.clone();
        let workspace = self.workspace.clone();
        let prompt = prompt.to_string();

        tokio::spawn(async move {
            match pipeline.run(&session_id, &prompt, &workspace).await {
                Ok(result) => {
                    if result.passed {
                        tracing::info!("Flow passed after {} rounds", result.fix_rounds);
                    } else {
                        tracing::warn!("Flow did not pass after {} rounds", result.fix_rounds);
                    }
                    // 日志输出 Flow 报告
                    tracing::info!("Flow result: {}", result.final_answer);
                    if !result.verification_passed {
                        tracing::warn!("Verification errors: {:?}", result.verification_errors);
                    }
                    let _ = result;
                }
                Err(e) => {
                    tracing::error!("Flow pipeline error: {}", e);
                }
            }
        });
    }

    /// Submit in Plan-First Flow mode: plan → review plan → execute
    async fn submit_flow_plan(&mut self, prompt: &str) {
        use rgoat_core::agent::flow::FlowPipeline;
        self.is_processing = true;
        self.status_msg = "Flow Plan: generating plan...".to_string();
        self.processing_start = Some(Instant::now());
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.streaming_idx = None;
        self.ctrl_c_count = 0;

        let review_provider: Arc<dyn LlmProvider> = self.review_provider.clone()
            .unwrap_or_else(|| self.agent.provider.clone());

        let mut review_config = self.agent.config.clone();
        review_config.max_steps = 10;
        let review_agent = Arc::new(ReActAgent::new(
            review_config,
            review_provider,
            self.agent.tools.clone(),
            self.agent.approval.clone(),
            self.agent.conversation.clone(),
            self.agent.event_bus.clone(),
            self.agent.cancellation.clone(),
            AgentMode::Flow,
            self.agent.paused.clone(),
            self.agent.approval_responder.clone(),
        ));

        let pipeline = FlowPipeline::new(self.agent.clone(), review_agent);
        let session_id = self.session_id.clone();
        let workspace = self.workspace.clone();
        let prompt = prompt.to_string();

        tokio::spawn(async move {
            match pipeline.run_plan_first(&session_id, &prompt, &workspace).await {
                Ok(result) => {
                    if result.success {
                        tracing::info!("Plan-First Flow succeeded: {}", result.summary);
                    } else {
                        tracing::warn!("Plan-First Flow failed: {}", result.summary);
                    }
                    let _ = result;
                }
                Err(e) => {
                    tracing::error!("Plan-First Flow error: {}", e);
                }
            }
        });
    }

    /// Submit in Plan mode: Explore → Plan → Phase 3 user approval
    async fn submit_plan(&mut self, prompt: &str) {
        use rgoat_core::agent::plan_runner::PlanRunner;
        self.is_processing = true;
        self.status_msg = "Plan: exploring...".to_string();
        self.processing_start = Some(Instant::now());
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.streaming_idx = None;
        self.ctrl_c_count = 0;

        let runner = PlanRunner::new(
            self.agent.clone(),
            self.workspace.clone(),
            self.session_id.clone(),
        );

        let session_id = self.session_id.clone();
        let workspace = self.workspace.clone();
        let prompt = prompt.to_string();

        // 运行 PlanRunner（在后台）
        let result = runner.run(&prompt).await;

        self.is_processing = false;

        match result {
            Ok(plan_result) => {
                if plan_result.phase == rgoat_core::agent::plan_runner::PlanPhase::Cancelled {
                    self.add_line(UiElement::System { text: "⚠️ Plan mode cancelled.".into() });
                    return;
                }
                if plan_result.phase == rgoat_core::agent::plan_runner::PlanPhase::Error {
                    self.add_line(UiElement::System { text: "❌ Plan mode error.".into() });
                    return;
                }

                // 展示计划内容
                if !plan_result.plan_content.is_empty() {
                    self.add_line(UiElement::System { text: "─".repeat(50) });
                    self.add_line(UiElement::System { text: "📋 Plan complete!".into() });
                    self.add_line(UiElement::System { text: "─".repeat(50) });
                    if let Some(ref path) = plan_result.plan_path {
                        self.add_line(UiElement::System {
                            text: format!("📄 Plan saved: {}", path.display()),
                        });
                    }
                    // 展示计划内容摘要（前 500 字符）
                    let preview = if plan_result.plan_content.len() > 500 {
                        format!("{}...", &plan_result.plan_content[..500])
                    } else {
                        plan_result.plan_content.clone()
                    };
                    self.add_line(UiElement::System { text: preview });
                    self.add_line(UiElement::System { text: "─".repeat(50) });
                    self.add_line(UiElement::System {
                        text: "(a) Agent mode — step-by-step execution".into(),
                    });
                    self.add_line(UiElement::System {
                        text: "(y) YOLO mode — auto execution".into(),
                    });
                    self.add_line(UiElement::System {
                        text: "(n) Continue modifying plan".into(),
                    });
                    self.add_line(UiElement::System { text: "Type a/y/n to choose:".into() });
                    // 存储计划内容，等待用户选择
                    self.pending_plan = Some(plan_result.plan_content);
                }
            }
            Err(e) => {
                self.add_line(UiElement::System {
                    text: format!("❌ Plan error: {}", e),
                });
            }
        }
    }

    /// Submit a pre-built prompt directly to the agent (used by /findskill)
    async fn submit_prompt(&mut self, prompt: &str) {
        self.is_processing = true;
        self.current_step = 0;
        self.status_msg = "Processing...".to_string();
        self.processing_start = Some(Instant::now());
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.streaming_idx = None;
        self.ctrl_c_count = 0;

        let agent = self.agent.clone();
        let session_id = self.session_id.clone();
        let workspace = self.workspace.clone();
        let prompt = prompt.to_string();
        let thinking_level = self.thinking_level;

        tokio::spawn(async move {
            let options = ChatOptions {
                thinking_level: Some(thinking_level),
            };
            let result = agent.run_with_options(&session_id, &prompt, &workspace, &options).await;
            if let Err(e) = &result {
                tracing::error!("Agent error: {}", e);
            }
            let _ = result;
        });
    }

    /// Execute a shell command and display output in chat (T1-#3).
    async fn run_shell_visible(&mut self, cmd: &str) {
        self.add_line(UiElement::User { text: format!("!{}", cmd) });
        let output = Self::exec_shell(cmd).await;
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let text = if stderr.is_empty() {
                    stdout.to_string()
                } else {
                    format!("{}\n[stderr]\n{}", stdout, stderr)
                };
                self.add_line(UiElement::System { text: text.trim_end().to_string() });
            }
            Err(e) => self.add_line(UiElement::Error { text: format!("Shell error: {}", e) }),
        }
    }

    /// Execute a shell command, inject output into the next agent prompt (T1-#3).
    async fn run_shell_hidden(&mut self, cmd: &str) {
        let output = Self::exec_shell(cmd).await;
        let result = match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
            Err(e) => format!("[shell error: {}]", e),
        };
        let injected = format!("`!{}` 的输出：\n```\n{}\n```", cmd, result.trim_end());
        self.add_line(UiElement::User { text: injected.clone() });
        self.submit_prompt(&injected).await;
    }

    /// Execute a shell command with 30s timeout and platform-specific shell.
    async fn exec_shell(cmd: &str) -> Result<std::process::Output, String> {
        let future = async {
            #[cfg(windows)]
            {
                tokio::process::Command::new("cmd")
                    .arg("/C").arg(cmd)
                    .output().await
                    .map_err(|e| format!("{}", e))
            }
            #[cfg(not(windows))]
            {
                tokio::process::Command::new("sh")
                    .arg("-c").arg(cmd)
                    .output().await
                    .map_err(|e| format!("{}", e))
            }
        };
        tokio::time::timeout(Duration::from_secs(30), future)
            .await
            .unwrap_or(Err("Command timed out after 30s".to_string()))
    }

    async fn handle_command(&mut self, input: &str) {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts[0];

        match cmd {
            // ── Direct mode switching (CodeWhale style) ──
            "/yolo" => {
                self.mode = AgentMode::Yolo;
                self.agent.set_mode(AgentMode::Yolo);
                self.add_line(UiElement::System { text: "⚡ Switched to YOLO mode — all tool calls auto-approved.".into() });
                self.status_msg = "Mode: yolo".to_string();
            }
            "/agent" => {
                self.mode = AgentMode::Agent;
                self.agent.set_mode(AgentMode::Agent);
                self.add_line(UiElement::System { text: "🤖 Switched to Agent mode — each tool call requires confirmation.".into() });
                self.status_msg = "Mode: agent".to_string();
            }
            "/plan" => {
                self.mode = AgentMode::Plan;
                self.agent.set_mode(AgentMode::Plan);
                self.add_line(UiElement::System { text: "📋 Switched to Plan mode — agent will analyze and plan before executing.".into() });
                self.status_msg = "Mode: plan".to_string();
            }
            "/flow" => {
                self.mode = AgentMode::Flow;
                self.agent.set_mode(AgentMode::Flow);
                self.add_line(UiElement::System { text: "🌊 Switched to Flow mode — continuous autonomous execution.".into() });
                self.status_msg = "Mode: flow".to_string();
            }
            "/flow_plan" => {
                self.mode = AgentMode::Flow;
                self.agent.set_mode(AgentMode::Flow);
                self.add_line(UiElement::System { text: "📋 Plan-First Flow: generate plan → review → execute.".into() });
                self.status_msg = "Mode: flow_plan".to_string();
            }
            "/edits" => {
                self.mode = AgentMode::AcceptEdits;
                self.agent.set_mode(AgentMode::AcceptEdits);
                self.add_line(UiElement::System { text: "✏ Switched to Accept-Edits mode — file changes auto-accepted.".into() });
                self.status_msg = "Mode: accept-edits".to_string();
            }
            "/help" => {
                self.add_line(UiElement::System {
                    text: "Commands:\n  /yolo      ⚡ YOLO mode\n  /agent     🤖 Agent mode\n  /plan      📋 Plan mode\n  /flow      🌊 Flow mode\n  /edits     ✏ Accept-Edits\n  /model     📦 Models (API fetch)\n  /editprovider ⚙ Edit providers\n  /name <n>  Rename session\n  /export    Export session to markdown\n  /copy      Copy last response\n  /hotkeys   Show keybindings\n  /fork      ⑂ Fork current session (alias: /clone)\n  /reload    ⟳ Reload settings/skills/keybindings\n  /settings ⚙ Open settings dialog\n  /help      Show commands\n  /clear     Clear conversation\n  /new       New session\n  /resume <id> Resume\n  /sessions  List sessions\n  /session <id> Info\n  /findskill 🔍 Find & install skills\n  /mcp list  MCP servers\n  /compact   Compress\n  /exit      Quit".into(),
                });
            }
            "/name" => {
                let name = parts.get(1..).map(|s| s.join(" ")).unwrap_or_default();
                if name.is_empty() {
                    self.add_line(UiElement::System {
                        text: format!("Current session name: {}", self.session_title),
                    });
                } else {
                    match self.conversation.update_title(&self.session_id, &name).await {
                        Ok(_) => {
                            self.session_title = name.clone();
                            self.add_line(UiElement::System {
                                text: format!("Session renamed: {}", name),
                            });
                        }
                        Err(e) => self.add_line(UiElement::Error {
                            text: format!("Rename failed: {}", e),
                        }),
                    }
                }
            }
            "/export" => {
                let file = parts.get(1).map(|s| s.to_string())
                    .unwrap_or_else(|| format!("goat-session-{}.md", self.session_id.chars().take(8).collect::<String>()));
                match self.conversation.get_messages(&self.session_id).await {
                    Ok(messages) => {
                        let mut md = String::new();
                        md.push_str(&format!("# RGoat Session {}\n\n", self.session_id.chars().take(8).collect::<String>()));
                        for msg in messages {
                            match msg.role.as_str() {
                                "user" => md.push_str(&format!("## User\n\n{}\n\n", msg.content)),
                                "assistant" => md.push_str(&format!("## Assistant\n\n{}\n\n", msg.content)),
                                _ => md.push_str(&format!("### {}\n\n{}\n\n", msg.role, msg.content)),
                            }
                        }
                        match std::fs::write(&file, md) {
                            Ok(_) => self.add_line(UiElement::System { text: format!("Exported to {}", file) }),
                            Err(e) => self.add_line(UiElement::Error { text: format!("Export failed: {}", e) }),
                        }
                    }
                    Err(e) => self.add_line(UiElement::Error { text: format!("Export failed: {}", e) }),
                }
            }
            "/copy" => {
                self.copy_last_assistant();
            }
            "/hotkeys" => {
                self.add_line(UiElement::System {
                    text: render_hotkeys(self.keybindings.as_ref()),
                });
            }
            "/clear" => {
                self.lines.clear();
                self.scroll_from_bottom = 0;
                self.streaming_idx = None;
                self.add_line(UiElement::System { text: "Cleared.".into() });
            }
            "/model" => {
                // Fetch available models from current provider's /models API endpoint
                let details = self.switch.list_details().await;
                let current = details.iter().find(|d| d.is_current).cloned();
                match current {
                    Some(d) if !d.base_url.is_empty() => {
                        let models_url = format!("{}/models", d.base_url.trim_end_matches('/'));
                        let client = reqwest::Client::new();
                        let mut req = client.get(&models_url);
                        if !d.api_key.is_empty() {
                            req = req.header("Authorization", format!("Bearer {}", d.api_key));
                        }
                        match req.send().await {
                            Ok(resp) => {
                                match resp.json::<serde_json::Value>().await {
                                    Ok(json) => {
                                        let models: Vec<&str> = json.get("data")
                                            .and_then(|d| d.as_array())
                                            .map(|arr| {
                                                arr.iter()
                                                    .filter_map(|m| m.get("id").and_then(|id| id.as_str()))
                                                    .collect()
                                            })
                                            .unwrap_or_default();
                                        if models.is_empty() {
                                            self.add_line(UiElement::System {
                                                text: format!("No models returned from {}.\nResponse: {}", models_url, serde_json::to_string_pretty(&json).unwrap_or_default()),
                                            });
                                        } else {
                                            // M14: extract context_window from the current model's entry
                                            let model_data = json.get("data").and_then(|d| d.as_array());
                                            let ctx_window = model_data
                                                .and_then(|arr| {
                                                    arr.iter().find(|m| m.get("id").map(|i| i.as_str() == Some(&d.model)).unwrap_or(false))
                                                })
                                                .and_then(|m| m.get("context_window").or_else(|| m.get("max_tokens")).or_else(|| m.get("context_length")))
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(128_000) as usize;
                                            if ctx_window > 128_000 {
                                                // 模型中声明更大的窗口 → 动态增大阈值，通过 helper 更新内部上下文窗口
                                                self.agent.update_context_window(ctx_window);
                                            }

                                            let mut lines = vec![format!("Provider: {} | {}", d.name, models_url)];
                                            for m in &models {
                                                let marker = if *m == d.model { " ▶" } else { "  " };
                                                lines.push(format!("{} {}", marker, m));
                                            }
                                            lines.push("".into());
                                            lines.push(format!("{} models. Context window: {}k. Current: {}", models.len(), ctx_window / 1000, d.model));
                                            self.add_line(UiElement::System { text: lines.join("\n") });
                                            self.status_msg = format!("{} models ({}k ctx)", models.len(), ctx_window / 1000);
                                        }
                                    }
                                    Err(e) => {
                                        self.add_line(UiElement::Error { text: format!("Parse error: {}", e) });
                                    }
                                }
                            }
                            Err(e) => {
                                self.add_line(UiElement::Error { text: format!("HTTP error: {}", e) });
                            }
                        }
                    }
                    _ => {
                        self.add_line(UiElement::System {
                            text: "No provider configured. Set API keys and restart.".into(),
                        });
                    }
                }
            }
            "/new" => {
                // M10: 实际创建新会话
                match self.conversation.get_or_create_session("new", Some("New Session"), Some(&self.workspace)).await {
                    Ok(session) => {
                        let sid_short: String = session.id.chars().take(8).collect();
                        self.session_id = session.id;
                        self.session_title = session.title.clone();
                        self.add_line(UiElement::System {
                            text: format!("New session created: {} ({})", sid_short, session.title),
                        });
                        self.status_msg = "New session".to_string();
                    }
                    Err(e) => {
                        self.add_line(UiElement::Error { text: format!("Failed to create session: {}", e) });
                    }
                }
            }
            "/compact" => {
                // Manual context compression: keep last N messages, summarize the rest
                use rgoat_core::conversation::compressor::{CompressedMessage, CompressionConfig, CompressionLevel, ContextCompressor};
                match self.conversation.get_messages(&self.session_id).await {
                    Ok(messages) if messages.len() <= 10 => {
                        self.add_line(UiElement::System {
                            text: format!("Only {} messages — nothing to compact.", messages.len()),
                        });
                    }
                    Ok(messages) => {
                        let total = messages.len();
                        let keep = 20.min(total / 2); // Keep last ~half, max 20
                        let split = total - keep;
                        let before_id = messages[split].id;

                        // Generate summary of compressed messages
                        let cm: Vec<CompressedMessage> = messages[..split]
                            .iter()
                            .map(|m| CompressedMessage {
                                role: m.role.clone(),
                                content: m.content.clone(),
                                is_summary: false,
                            })
                            .collect();

                        let compressor = ContextCompressor::new(CompressionConfig::default());
                        let result = compressor.compress(&cm, CompressionLevel::Micro);
                        let summary = result.summary.as_deref().unwrap_or("[conversation context compressed]");

                        // Delete old messages from DB
                        match self.conversation.delete_messages(&self.session_id, before_id).await {
                            Ok(deleted) => {
                                self.add_line(UiElement::System {
                                    text: format!("⊟ Compacted: {} messages → summary + {} recent ({} deleted). {}",
                                        total - keep, keep, deleted, summary),
                                });
                                self.status_msg = format!("Compacted ({} kept)", keep);
                            }
                            Err(e) => {
                                self.add_line(UiElement::Error {
                                    text: format!("Compaction failed: {}", e),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        self.add_line(UiElement::Error {
                            text: format!("Failed to read messages: {}", e),
                        });
                    }
                }
            }
            "/resume" => {
                let arg = parts.get(1).copied().unwrap_or("");
                if arg.is_empty() {
                    self.add_line(UiElement::System {
                        text: "Usage: /resume <session_id>. Use /list to see available sessions.".into(),
                    });
                } else {
                    match self.conversation.get_session(arg).await {
                        Ok(Some(s)) => {
                            // 加载历史消息到 UI（加载成功后再更新 session_id/status_msg，避免失败时状态不一致）
                            match self.conversation.get_messages(&s.id).await {
                                Ok(messages) => {
                                    // 清空当前 UI 元素及搜索状态，避免新旧消息混合与搜索索引错位
                                    self.lines.clear();
                                    self.search_matches.clear();
                                    self.search_current = 0;
                                    let mut msg_count = 0;
                                    for msg in messages {
                                        // 跳过空内容的消息
                                        if msg.content.is_empty() {
                                            continue;
                                        }
                                        match msg.role.as_str() {
                                            "user" => {
                                                self.add_line(UiElement::User { text: msg.content });
                                                msg_count += 1;
                                            }
                                            "assistant" => {
                                                self.add_line(UiElement::Assistant { text: msg.content });
                                                msg_count += 1;
                                            }
                                            "tool" => {
                                                // tool 消息可能带 [error] 前缀，据此设置 success 字段
                                                let success = !msg.content.starts_with("[error]");
                                                let summary: String = msg.content.chars().take(100).collect();
                                                let output_line_count = msg.content.lines().count();
                                                self.add_line(UiElement::ToolCall {
                                                    name: "tool".into(),
                                                    success,
                                                    summary,
                                                    output: msg.content,
                                                    collapsed: true,
                                                    elapsed_ms: 0,
                                                    output_line_count,
                                                });
                                                msg_count += 1;
                                            }
                                            "system" => {
                                                self.add_line(UiElement::System { text: msg.content });
                                                msg_count += 1;
                                            }
                                            _ => {}
                                        }
                                    }
                                    // 加载成功，提交 session 状态
                                    self.session_id = s.id;
                                    self.session_title = s.title.clone();
                                    self.status_msg = format!("Resumed session: {}", s.title);
                                    // 注入续传提示
                                    self.add_line(UiElement::System {
                                        text: format!("─ Session resumed ({} messages loaded). Continue from where you left off. ─", msg_count),
                                    });
                                }
                                Err(e) => {
                                    self.add_line(UiElement::Error {
                                        text: format!("Failed to load messages: {}", e),
                                    });
                                }
                            }
                        }
                        Ok(None) => {
                            self.add_line(UiElement::Error {
                                text: format!("Session not found: {}", arg),
                            });
                        }
                        Err(e) => {
                            self.add_line(UiElement::Error {
                                text: format!("Failed to load session: {}", e),
                            });
                        }
                    }
                }
            }
            "/provider" => {
                let arg = parts.get(1).copied().unwrap_or("");
                if !arg.is_empty() {
                    // Direct switch: /provider deepseek
                    match self.switch.select(arg).await {
                        Ok(()) => {
                            self.current_provider = self.switch.current_name().await;
                            self.add_line(UiElement::System {
                                text: format!("Switched to provider: {}", arg),
                            });
                            self.status_msg = format!("Provider: {}", arg);
                        }
                        Err(e) => {
                            self.add_line(UiElement::Error { text: e });
                        }
                    }
                } else {
                    // List providers (二级菜单形式)
                    let details = self.switch.list_details().await;
                    let current = self.switch.current_name().await;
                    if details.len() <= 1 {
                        self.add_line(UiElement::System {
                            text: format!("Only one provider: {} (model: {})", current, details.first().map(|d| d.model.as_str()).unwrap_or("?")),
                        });
                    } else {
                        let mut lines = vec!["Configured providers:".to_string()];
                        for d in &details {
                            let marker = if d.is_current { " ▶" } else { "  " };
                            lines.push(format!("{} {} | model: {} | type: {}", marker, d.name, d.model, d.provider_type));
                        }
                        lines.push("".into());
                        lines.push("To switch: /provider <name>. To edit: /editprovider.".into());
                        self.add_line(UiElement::System { text: lines.join("\n") });
                    }
                }
            }
            "/editprovider" => {
                // Replaces old /providers: show configured providers + "Add" option (二级菜单)
                let details = self.switch.list_details().await;
                let mut lines = vec!["Configured providers:".to_string()];
                for d in &details {
                    let marker = if d.is_current { " ▶" } else { "  " };
                    lines.push(format!("{} {} | model: {} | type: {}", marker, d.name, d.model, d.provider_type));
                }
                lines.push("".into());
                lines.push("To add a provider: use CLI or edit the settings file.".into());
                lines.push("Set env vars (DEEPSEEK_API_KEY / OPENAI_API_KEY / ANTHROPIC_API_KEY) and restart.".into());
                self.add_line(UiElement::System { text: lines.join("\n") });
                self.status_msg = format!("{} providers configured", details.len());
            }
            "/exit" | "/quit" => {
                self.status_msg = "Press Ctrl-C or Esc to exit".to_string();
            }
            "/findskill" => {
                if self.is_processing {
                    self.status_msg = "Already processing...".to_string();
                    return;
                }
                // 执行方式：将 find-skills SKILL.md 内容注入为 agent prompt，告知其如何搜索/安装技能
                let arg = parts.get(1).copied().unwrap_or("");
                if arg.is_empty() {
                    // Load find-skills SKILL.md and inject it
                    let skill_content = find_skills_content();
                    let prompt = format!(
                        "I need help finding skills. Here is the find-skills guide:\n\n{}\n\n---\nPlease help me discover relevant skills. What kind of task do you need help with?",
                        skill_content
                    );
                    // Submit as a regular prompt to the agent
                    self.add_line(UiElement::User { text: "/findskill".to_string() });
                    self.submit_prompt(&prompt).await;
                } else {
                    let skill_content = find_skills_content();
                    let prompt = format!(
                        "I'm looking for skills related to: {}\n\nHere is the find-skills guide:\n\n{}\n\n---\nPlease search for relevant skills using the instructions above.",
                        arg, skill_content
                    );
                    self.add_line(UiElement::User { text: format!("/findskill {}", arg) });
                    self.submit_prompt(&prompt).await;
                }
            }
            "/mcp" => {
                let arg = parts.get(1).copied().unwrap_or("");
                if arg == "list" {
                    let home = std::env::var("HOME")
                        .or_else(|_| std::env::var("USERPROFILE"))
                        .unwrap_or_default();
                    let config_path = std::env::var("GOAT_MCP_CONFIG")
                        .unwrap_or_else(|_| format!("{}/.goat/mcp.json", home));
                    let path = std::path::Path::new(&config_path);
                    if path.exists() {
                        match std::fs::read_to_string(path) {
                            Ok(content) => {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                                    if let Some(servers) = json.get("mcpServers").and_then(|s| s.as_object()) {
                                        if servers.is_empty() {
                                            self.add_line(UiElement::System { text: "No MCP servers configured.".into() });
                                        } else {
                                            let mut lines = vec![format!("MCP servers ({}):", servers.len())];
                                            for (name, cfg) in servers {
                                                let cmd = cfg.get("command").and_then(|c| c.as_str()).unwrap_or("?");
                                                lines.push(format!("  {} → {}", name, cmd));
                                            }
                                            self.add_line(UiElement::System { text: lines.join("\n") });
                                            self.status_msg = format!("{} MCP servers", servers.len());
                                        }
                                    } else {
                                        self.add_line(UiElement::System { text: "MCP config has no servers section.".into() });
                                    }
                                } else {
                                    self.add_line(UiElement::Error { text: "Invalid MCP config JSON.".into() });
                                }
                            }
                            Err(e) => {
                                self.add_line(UiElement::Error { text: format!("Failed to read MCP config: {}", e) });
                            }
                        }
                    } else {
                        self.add_line(UiElement::System {
                            text: format!("No MCP config at {}. Create with:\n  {{\"mcpServers\": {{\"name\": {{\"command\": \"npx\", \"args\": [\"-y\", \"@server/pkg\"]}}}}}}", config_path),
                        });
                    }
                } else {
                    self.add_line(UiElement::System {
                        text: "Usage:\n  /mcp list  — List configured MCP servers".into(),
                    });
                }
            }
            "/sessions" => {
                // M10: 列出可用会话
                match self.conversation.list_sessions().await {
                    Ok(sessions) if sessions.is_empty() => {
                        self.add_line(UiElement::System { text: "No saved sessions.".into() });
                    }
                    Ok(sessions) => {
                        let mut lines = vec!["Saved sessions:".to_string()];
                        for s in &sessions {
                            lines.push(format!(
                                "  [{}] {} — {} msg, {}",
                                s.id.chars().take(8).collect::<String>(),
                                s.title,
                                s.message_count,
                                s.updated_at.format("%Y-%m-%d %H:%M"),
                            ));
                        }
                        self.add_line(UiElement::System { text: lines.join("\n") });
                        self.status_msg = format!("{} sessions", sessions.len());
                    }
                    Err(e) => {
                        self.add_line(UiElement::Error { text: format!("Failed: {}", e) });
                    }
                }
            }
            "/session" => {
                let arg = parts.get(1).copied().unwrap_or("");
                if arg.is_empty() {
                    self.add_line(UiElement::System { text: "Usage: /session <id>".into() });
                } else {
                    match self.conversation.get_session(arg).await {
                        Ok(Some(s)) => {
                            self.add_line(UiElement::System {
                                text: format!("Session: {} | {} messages | {}", s.title, s.message_count, s.updated_at.format("%Y-%m-%d %H:%M")),
                            });
                        }
                        Ok(None) => {
                            self.add_line(UiElement::Error { text: format!("Not found: {}", arg) });
                        }
                        Err(e) => {
                            self.add_line(UiElement::Error { text: format!("Failed: {}", e) });
                        }
                    }
                }
            }
            "/fork" | "/clone" => {
                if self.is_processing {
                    self.add_line(UiElement::Error {
                        text: "Cannot fork while agent is processing — press Esc to interrupt first.".into(),
                    });
                    return;
                }
                match self.conversation.fork_session(&self.session_id, None, None).await {
                    Ok(new_session) => {
                        let sid_short: String = new_session.id.chars().take(8).collect();
                        self.add_line(UiElement::System {
                            text: format!(
                                " ⑂ Forked to new session: {} ({} messages copied)",
                                sid_short, new_session.message_count
                            ),
                        });
                        self.session_id = new_session.id;
                        self.session_title = new_session.title.clone();
                        self.reload_messages().await;
                        self.status_msg = format!("Forked → {}", sid_short);
                    }
                    Err(e) => self.add_line(UiElement::Error {
                        text: format!("Fork failed: {}", e),
                    }),
                }
            }
            "/reload" => {
                if self.is_processing {
                    self.add_line(UiElement::Error {
                        text: "Cannot reload while agent is processing — press Esc to interrupt first.".into(),
                    });
                    return;
                }
                let mut reloaded: Vec<String> = Vec::new();
                match rgoat_core::core::config::Settings::load() {
                    Ok(s) => {
                        reloaded.push(format!(
                            "settings(provider={}, model={})",
                            s.provider,
                            s.model
                        ));
                    }
                    Err(e) => self.add_line(UiElement::Error {
                        text: format!("Settings reload failed: {}", e),
                    }),
                }
                self.loaded_skills = load_skills_for_menu(&self.workspace);
                reloaded.push(format!("skills({})", self.loaded_skills.len()));
                self.keybindings = load_keybindings();
                if let Some(kb) = &self.keybindings {
                    reloaded.push(format!("keybindings({})", kb.len()));
                } else {
                    reloaded.push("keybindings(default)".into());
                }
                self.add_line(UiElement::System {
                    text: format!("⟳ Reloaded: {}", reloaded.join(", ")),
                });
                self.add_line(UiElement::System {
                    text: "Note: provider/model/agent-config changes require restart to take full effect.".into(),
                });
                self.status_msg = "Reloaded".to_string();
            }
            "/settings" => {
                match rgoat_core::core::config::Settings::load() {
                    Ok(s) => {
                        let items = vec![
                            SettingsItem {
                                key: "provider".into(),
                                label: "Default Provider".into(),
                                value: s.provider.clone(),
                                kind: ItemKind::Text,
                            },
                            SettingsItem {
                                key: "model".into(),
                                label: "Default Model".into(),
                                value: s.model.clone(),
                                kind: ItemKind::Text,
                            },
                            SettingsItem {
                                key: "max_agent_turns".into(),
                                label: "Max Agent Turns".into(),
                                value: s.max_agent_turns.to_string(),
                                kind: ItemKind::Number { min: 1, max: 500, step: 1 },
                            },
                            SettingsItem {
                                key: "max_concurrency".into(),
                                label: "Max Concurrency".into(),
                                value: s.max_concurrency.to_string(),
                                kind: ItemKind::Number { min: 1, max: 16, step: 1 },
                            },
                            SettingsItem {
                                key: "max_depth".into(),
                                label: "Max Subagent Depth".into(),
                                value: s.max_depth.to_string(),
                                kind: ItemKind::Number { min: 1, max: 10, step: 1 },
                            },
                            SettingsItem {
                                key: "sub_model".into(),
                                label: "Sub-Agent Model".into(),
                                value: s.sub_model.clone().unwrap_or_default(),
                                kind: ItemKind::Text,
                            },
                            SettingsItem {
                                key: "review_model".into(),
                                label: "Flow Review Model".into(),
                                value: s.review_model.clone().unwrap_or_default(),
                                kind: ItemKind::Text,
                            },
                        ];
                        self.settings_dialog = Some(SettingsDialog::new(items));
                        self.status_msg = "Settings dialog opened".to_string();
                    }
                    Err(e) => self.add_line(UiElement::Error {
                        text: format!("Failed to load settings: {}", e),
                    }),
                }
            }
            _ => {
                self.add_line(UiElement::System {
                    text: format!("Unknown: {}. Use /help to see available commands.", cmd),
                });
            }
        }
    }

    /// Reload all messages of the current session into the chat area.
    /// Used by `/fork` and `/clone` after switching `session_id`.
    /// Mirrors the rendering logic of `/resume` (system messages are skipped to
    /// avoid duplicating the session-start banner).
    async fn reload_messages(&mut self) {
        self.lines.clear();
        self.scroll_from_bottom = 0;
        self.streaming_idx = None;
        self.search_matches.clear();
        self.search_current = 0;
        let messages = match self.conversation.get_messages(&self.session_id).await {
            Ok(m) => m,
            Err(e) => {
                self.add_line(UiElement::Error {
                    text: format!("Reload messages failed: {}", e),
                });
                return;
            }
        };
        for msg in messages {
            if msg.content.is_empty() {
                continue;
            }
            match msg.role.as_str() {
                "user" => self.add_line(UiElement::User { text: msg.content }),
                "assistant" => self.add_line(UiElement::Assistant { text: msg.content }),
                "tool" => {
                    let success = !msg.content.starts_with("[error]");
                    let summary: String = msg.content.chars().take(100).collect();
                    let output_line_count = msg.content.lines().count();
                    self.add_line(UiElement::ToolCall {
                        name: "tool".into(),
                        success,
                        summary,
                        output: msg.content,
                        collapsed: true,
                        elapsed_ms: 0,
                        output_line_count,
                    });
                }
                "system" => self.add_line(UiElement::System { text: msg.content }),
                _ => {}
            }
        }
    }

    /// Persist numeric edits from the settings dialog back to `setting.json`.
    /// Only fields present in `Settings` and edited via the dialog are written;
    /// string fields are read-only in the dialog and pass through unchanged.
    fn save_settings_from_dialog(&mut self) -> Result<(), String> {
        let dialog = self
            .settings_dialog
            .as_ref()
            .ok_or_else(|| "Settings dialog not open".to_string())?;
        let mut settings =
            rgoat_core::core::config::Settings::load().map_err(|e| e.to_string())?;
        for item in &dialog.items {
            match item.key.as_str() {
                "max_agent_turns" => {
                    settings.max_agent_turns =
                        item.value.parse().unwrap_or(settings.max_agent_turns);
                }
                "max_concurrency" => {
                    settings.max_concurrency =
                        item.value.parse().unwrap_or(settings.max_concurrency);
                }
                "max_depth" => {
                    settings.max_depth = item.value.parse().unwrap_or(settings.max_depth);
                }
                _ => {}
            }
        }
        settings.save().map_err(|e| e.to_string())?;
        if let Some(d) = self.settings_dialog.as_mut() {
            d.dirty = false;
        }
        Ok(())
    }

    fn scroll_down(&mut self, lines: usize) {
        // Move viewport down (toward the bottom).  scroll_from_bottom=0
        // means we are already pinned to the bottom.
        let before = self.scroll_from_bottom;
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(lines);
        if before != self.scroll_from_bottom {
            self.set_scroll_hint(format!("▼ Scrolled down {} lines", lines));
        } else if self.scroll_from_bottom == 0 {
            self.set_scroll_hint("▼ Following".into());
        }
    }

    fn scroll_up(&mut self, lines: usize) {
        // Move viewport up (away from the bottom).  No artificial cap —
        // the start calculation in ui() will clamp when scroll_from_bottom
        // exceeds the available scroll range.
        let before = self.scroll_from_bottom;
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(lines);
        if self.scroll_from_bottom != before {
            self.set_scroll_hint(format!("▲ Scrolled up {} lines", lines));
        }
    }

    /// Jump to the very top of the conversation.
    fn scroll_to_top(&mut self) {
        // Set to a large value — ui() will clamp to max_scroll.
        self.scroll_from_bottom = usize::MAX / 4;
        self.set_scroll_hint("TOP".into());
    }

    /// Jump to the very bottom (follow new messages).
    fn scroll_to_bottom(&mut self) {
        self.scroll_from_bottom = 0;
        self.set_scroll_hint("▼ Following".into());
    }

    /// Set a transient scroll hint that expires after 2 seconds (TUI-5).
    fn set_scroll_hint(&mut self, msg: String) {
        self.scroll_hint = msg;
        self.scroll_hint_expiry = Some(Instant::now() + Duration::from_secs(2));
    }

    /// Copy the last assistant message to the system clipboard (T1-#5).
    fn copy_last_assistant(&mut self) {
        let last = self.lines.iter().rev().find_map(|l| match l {
            UiElement::Assistant { text } | UiElement::AssistantStream { text } => Some(text.clone()),
            _ => None,
        });
        match last {
            Some(text) => {
                match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
                    Ok(_) => self.set_scroll_hint("📋 Copied last response".into()),
                    Err(_) => self.set_scroll_hint("Clipboard error".into()),
                }
            }
            None => self.set_scroll_hint("No assistant message to copy".into()),
        }
    }

    /// Open external editor for the current input (Ctrl+G).
    /// Uses $VISUAL, $EDITOR, or platform default (notepad on Windows, nano on Unix).
    fn open_external_editor(&mut self) {
        let tmp_path = std::env::temp_dir().join(format!(".goat-editor-{}.md", std::process::id()));

        // Write current input to temp file
        if let Err(e) = std::fs::write(&tmp_path, &self.input) {
            self.add_line(UiElement::Error { text: format!("Editor write failed: {}", e) });
            return;
        }

        // Resolve editor
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| {
                if cfg!(windows) { "notepad".to_string() } else { "nano".to_string() }
            });

        // Suspend TUI
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = execute!(io::stdout(), Show);

        // Run editor synchronously (blocks event loop, but TUI is suspended)
        let status = std::process::Command::new(&editor)
            .arg(&tmp_path)
            .status();

        // Resume TUI
        let _ = enable_raw_mode();
        let _ = execute!(io::stdout(), EnterAlternateScreen);
        let _ = execute!(io::stdout(), Hide);

        match status {
            Ok(s) if s.success() => {
                match std::fs::read_to_string(&tmp_path) {
                    Ok(content) => {
                        self.input = content;
                        self.input_cursor = self.input.chars().count();
                        self.set_scroll_hint("Loaded from external editor".into());
                    }
                    Err(e) => self.add_line(UiElement::Error {
                        text: format!("Read back failed: {}", e),
                    }),
                }
            }
            _ => {
                self.add_line(UiElement::Error {
                    text: "Editor exited abnormally".into(),
                });
            }
        }

        let _ = std::fs::remove_file(&tmp_path);
    }

    // ── T1-#4: File picker methods ──

    /// Scan the workspace directory for files (skip .git, target, node_modules, .goat).
    /// Returns up to 500 files sorted by path.
    fn scan_project_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        let workspace = &self.workspace;
        let walker = walkdir::WalkDir::new(workspace)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                // Skip hidden dirs and common build output dirs
                !(name == ".git" || name == "target" || name == "node_modules"
                    || name == ".goat" || name == ".rgoat")
            });
        for entry in walker {
            if files.len() >= 500 {
                break;
            }
            if let Ok(e) = entry {
                if e.file_type().is_file() {
                    files.push(e.path().to_path_buf());
                }
            }
        }
        files.sort();
        files
    }

    /// Simple case-insensitive substring match, sorted by match position.
    fn fuzzy_filter(files: &[PathBuf], query: &str) -> Vec<PathBuf> {
        let q = query.to_lowercase();
        let mut scored: Vec<(usize, PathBuf)> = files
            .iter()
            .filter_map(|p| {
                let name = p.to_string_lossy().to_lowercase();
                name.find(&q).map(|pos| (pos, p.clone()))
            })
            .collect();
        scored.sort_by_key(|(pos, _)| *pos);
        scored.into_iter().map(|(_, p)| p).collect()
    }

    /// Activate the file picker after '@' was typed.
    fn activate_file_picker(&mut self) {
        self.file_picker_active = true;
        self.file_picker_query.clear();
        self.file_picker_matches = self.scan_project_files();
        self.file_picker_index = 0;
    }

    /// Deactivate the file picker.
    fn deactivate_file_picker(&mut self) {
        self.file_picker_active = false;
        self.file_picker_query.clear();
        self.file_picker_matches.clear();
        self.file_picker_index = 0;
    }

    /// Insert a file reference at the cursor position, replacing the '@' trigger and query.
    fn insert_file_ref(&mut self, path: &PathBuf) {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        // Check for binary files (NUL byte)
        if content.contains('\0') {
            self.set_scroll_hint("Binary file — cannot insert".into());
            self.deactivate_file_picker();
            return;
        }
        // Truncate large files (>10KB)
        let truncated = content.len() > 10_240;
        let display = if truncated {
            let mut c = content;
            c.truncate(10_240);
            c.push_str("\n\n[truncated — file exceeds 10KB]");
            c
        } else {
            content
        };
        let rel = path.strip_prefix(&self.workspace).unwrap_or(path);
        let insertion = format!("```{}\n{}\n```", rel.display(), display);

        // Remove the '@' trigger and query from input, then insert the file reference.
        // file_picker_trigger_pos 是 char index（与 input_cursor 语义一致）。
        let trigger_char = self.file_picker_trigger_pos;
        let query_len_chars = 1 + self.file_picker_query.chars().count(); // '@' + query chars

        // char index → byte index 安全转换（避免非 char boundary 切片 panic）
        let trigger_byte = self.input
            .char_indices()
            .nth(trigger_char)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len());
        let query_byte_end = self.input[trigger_byte..]
            .chars()
            .take(query_len_chars)
            .fold(trigger_byte, |acc, c| acc + c.len_utf8());

        let before: String = self.input[..trigger_byte].to_string();
        let after: String = self.input[query_byte_end..].to_string();
        self.input = before + &insertion + "\n" + &after;
        // cursor 用 char 数（不是 byte 长度），与 input_cursor 全局语义一致
        self.input_cursor = trigger_char + insertion.chars().count() + 1;
        self.deactivate_file_picker();
    }

    /// Start a search session (TUI-9).
    fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
        self.search_matches.clear();
        self.search_current = 0;
    }

    /// Execute search across all elements, populating matches (TUI-9).
    /// Uses element_line_starts to map matches to line numbers.
    fn execute_search(&mut self) {
        self.search_matches.clear();
        self.search_current = 0;
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            return;
        }
        for (idx, elem) in self.lines.iter().enumerate() {
            let text = match elem {
                UiElement::User { text } => text,
                UiElement::Assistant { text } => text,
                UiElement::AssistantStream { text } => text,
                UiElement::Thought { content, .. } => content,
                UiElement::ToolCall { name, output, .. } => {
                    if name.to_lowercase().contains(&query) || output.to_lowercase().contains(&query) {
                        let line_start = self.element_line_starts.get(idx).copied().unwrap_or(0);
                        self.search_matches.push((idx, line_start));
                    }
                    continue;
                }
                UiElement::System { text } => text,
                UiElement::Error { text } => text,
                UiElement::DiffView { path, .. } => {
                    if path.to_lowercase().contains(&query) {
                        let line_start = self.element_line_starts.get(idx).copied().unwrap_or(0);
                        self.search_matches.push((idx, line_start));
                    }
                    continue;
                }
                _ => continue,
            };
            if text.to_lowercase().contains(&query) {
                let line_start = self.element_line_starts.get(idx).copied().unwrap_or(0);
                self.search_matches.push((idx, line_start));
            }
        }
        // Jump to first match
        if let Some(&(_, line)) = self.search_matches.first() {
            self.jump_to_line(line);
        }
    }

    /// Jump to the next search match (TUI-9).
    fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_current = (self.search_current + 1) % self.search_matches.len();
        let (_, line) = self.search_matches[self.search_current];
        self.jump_to_line(line);
        self.set_scroll_hint(format!("Match {}/{}", self.search_current + 1, self.search_matches.len()));
    }

    /// Jump to the previous search match (TUI-9).
    fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        if self.search_current == 0 {
            self.search_current = self.search_matches.len() - 1;
        } else {
            self.search_current -= 1;
        }
        let (_, line) = self.search_matches[self.search_current];
        self.jump_to_line(line);
        self.set_scroll_hint(format!("Match {}/{}", self.search_current + 1, self.search_matches.len()));
    }

    /// Scroll so that the given line is centered in the viewport.
    fn jump_to_line(&mut self, line: usize) {
        // We need total_lines and visible_height, but those are only known
        // during render. Use a rough estimate: set scroll_from_bottom so
        // the target line is visible. The render will clamp.
        // target_line should be at the center: start = line - visible/2
        // scroll_from_bottom = total - start - visible = total - line + visible/2 - visible
        //                   = total - line - visible/2
        // Since we don't know total/visible here, store the target and
        // apply it in the render loop.
        self.pending_scroll_to_line = Some(line);
    }

    /// Toggle collapsed state of the element at the given chat line (TUI-10).
    fn toggle_collapse_at_line(&mut self, click_line: usize) {
        // Find which element owns this line
        let mut found_idx: Option<usize> = None;
        for (idx, &start) in self.element_line_starts.iter().enumerate() {
            if start <= click_line {
                found_idx = Some(idx);
            } else {
                break;
            }
        }
        if let Some(idx) = found_idx {
            if let Some(elem) = self.lines.get_mut(idx) {
                match elem {
                    UiElement::ToolCall { collapsed, .. } => {
                        *collapsed = !*collapsed;
                    }
                    UiElement::Thought { collapsed, .. } => {
                        *collapsed = !*collapsed;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Move cursor to the previous line in multi-line input (TUI-2)。
    /// 使用 display width 而非 char index 作为列，对齐 pi preferredVisualCol：
    /// 中文/emoji 行间上下移动保持视觉列；左右移动后重置 sticky。
    fn move_cursor_line_up(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let chars: Vec<char> = self.input.chars().collect();
        let cursor = self.input_cursor.min(chars.len());

        // Find the start of the current line (previous '\n' before cursor)
        let line_start = chars[..cursor].iter().rposition(|&c| c == '\n')
            .map(|p| p + 1)
            .unwrap_or(0);

        // If we're already on the first line, go to start
        if line_start == 0 {
            self.input_cursor = 0;
            self.preferred_visual_col = None;
            return;
        }

        // 计算当前视觉列（display width，非 char index）
        let current_line: String = chars[line_start..cursor].iter().collect();
        let visual_col = UnicodeWidthStr::width(current_line.as_str());
        // sticky column：若已有 preferred，取较大值（保持上次的最远列）
        let target_col = self.preferred_visual_col.unwrap_or(visual_col).max(visual_col);
        self.preferred_visual_col = Some(target_col);

        // Find the end of the previous line (skip the '\n')
        let prev_line_end = line_start.saturating_sub(1);
        let prev_line_start = chars[..prev_line_end].iter().rposition(|&c| c == '\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let prev_line: String = chars[prev_line_start..=prev_line_end].iter().collect();

        // 在上一行按 grapheme 累加宽度，找到 target_col 落点
        let mut acc_col = 0usize;
        let mut acc_chars = 0usize;
        for grapheme in prev_line.graphemes(true) {
            let gw = UnicodeWidthStr::width(grapheme);
            if acc_col + gw > target_col { break; }
            acc_col += gw;
            acc_chars += grapheme.chars().count();
        }
        self.input_cursor = prev_line_start + acc_chars;
    }

    /// Move cursor to the next line in multi-line input (TUI-2)。
    /// 使用 display width 而非 char index 作为列，对齐 pi preferredVisualCol。
    fn move_cursor_line_down(&mut self) {
        let chars: Vec<char> = self.input.chars().collect();
        let cursor = self.input_cursor.min(chars.len());

        // Check if there's a '\n' after the cursor (i.e., a next line exists)
        let next_newline = chars[cursor..].iter().position(|&c| c == '\n')
            .map(|p| cursor + p);
        let Some(next_nl) = next_newline else { return };

        // Find the start of the current line
        let line_start = chars[..cursor].iter().rposition(|&c| c == '\n')
            .map(|p| p + 1)
            .unwrap_or(0);

        // 计算当前视觉列
        let current_line: String = chars[line_start..cursor].iter().collect();
        let visual_col = UnicodeWidthStr::width(current_line.as_str());
        let target_col = self.preferred_visual_col.unwrap_or(visual_col).max(visual_col);
        self.preferred_visual_col = Some(target_col);

        // Next line starts right after the '\n'
        let next_line_start = next_nl + 1;
        // Find end of next line (next '\n' or end of string)
        let next_line_end = chars[next_line_start..].iter().position(|&c| c == '\n')
            .map(|p| next_line_start + p)
            .unwrap_or(chars.len());
        let next_line: String = chars[next_line_start..next_line_end].iter().collect();

        // 在下一行按 grapheme 累加宽度，找到 target_col 落点
        let mut acc_col = 0usize;
        let mut acc_chars = 0usize;
        for grapheme in next_line.graphemes(true) {
            let gw = UnicodeWidthStr::width(grapheme);
            if acc_col + gw > target_col { break; }
            acc_col += gw;
            acc_chars += grapheme.chars().count();
        }
        self.input_cursor = next_line_start + acc_chars;
    }

    /// Return the list of available slash commands, followed by loaded skills.
    /// Format: (command, filter_key, description). Skills appear after a separator.
    fn command_list(&self) -> Vec<(String, String, String)> {
        let mut items: Vec<(String, String, String)> = vec![
            ("/yolo".into(),     "yolo".into(),     "⚡ YOLO mode — auto-approve all".into()),
            ("/agent".into(),    "agent".into(),    "🤖 Agent mode — confirm each tool".into()),
            ("/plan".into(),     "plan".into(),     "📋 Plan mode — analyze first".into()),
            ("/flow".into(),     "flow".into(),     "🌊 Flow mode — continuous autonomous".into()),
            ("/flow_plan".into(),"flow_plan".into(),"📋 Plan-First Flow — plan then execute".into()),
            ("/edits".into(),    "edits".into(),    "✏ Accept-Edits — auto-accept edits".into()),
            ("/model".into(),    "model".into(),     "📦 Models — fetch from provider API".into()),
            ("/editprovider".into(), "editprovider".into(), "⚙ Edit provider configuration".into()),
            ("/help".into(),     "help".into(),     "Show all commands".into()),
            ("/clear".into(),    "clear".into(),    "Clear current conversation".into()),
            ("/new".into(),      "new".into(),      "Start a new session".into()),
            ("/resume".into(),   "resume".into(),   "Resume a previous session".into()),
            ("/sessions".into(), "sessions".into(), "List all saved sessions".into()),
            ("/session".into(),  "session".into(),  "Show session info by ID".into()),
            ("/mcp".into(),      "mcp".into(),      "MCP server management".into()),
            ("/compact".into(),  "compact".into(),  "Compress conversation context".into()),
            ("/fork".into(),     "fork".into(),     "⑂ Fork current session (alias /clone)".into()),
            ("/clone".into(),    "clone".into(),    "⑂ Alias for /fork".into()),
            ("/reload".into(),   "reload".into(),   "⟳ Reload settings/skills/keybindings".into()),
            ("/settings".into(), "settings".into(), "⚙ Open settings dialog".into()),
            ("/exit".into(),     "exit".into(),     "Exit the TUI".into()),
        ];

        // Add skills if loaded — visible alongside commands, with [Skill] label
        if !self.loaded_skills.is_empty() {
            for (name, desc) in &self.loaded_skills {
                let cmd = format!("/{}", name);
                let key = format!("/{}", name);
                let skill_desc = format!("[Skill] {}", desc);
                items.push((cmd, key, skill_desc));
            }
        }
        items
    }

    /// Process one AgentEvent from the event bus.
    fn handle_agent_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::Started { mode: _, prompt: _ } => {
                self.status_msg = "Agent started...".to_string();
            }
            AgentEvent::Thought { step, content } => {
                self.current_step = *step;
                self.sidebar_current_step = *step;
                if !content.is_empty() {
                    // D4: capture plan/progress text for the work sidebar.
                    if content.contains("## 计划")
                        || content.contains("## 进度")
                        || content.contains("[verify]")
                    {
                        self.sidebar_progress_text = content.clone();
                    }
                    self.add_line(UiElement::Thought {
                        step: *step,
                        content: content.clone(),
                        collapsed: true,
                    });
                }
                self.status_msg = format!("Thinking (step {}/{})...", step + 1, self.max_steps);
            }
            AgentEvent::ToolCall { step, tool_name, .. } => {
                self.current_step = *step;
                // P1: record start time for elapsed time tracking
                self.tool_start_times.insert(tool_name.clone(), Instant::now());
                // Deferred: ToolResult will create the merged UiElement::ToolCall
                self.status_msg = format!("Running {} (step {}/{})...", tool_name, step + 1, self.max_steps);
            }
            AgentEvent::ToolResult { step, tool_name, success, output } => {
                self.current_step = *step;
                let summary = summarize(output, 80);
                let output_line_count = output.lines().count();
                // P1: track tool elapsed time
                let elapsed_ms = self
                    .tool_start_times
                    .remove(tool_name)
                    .map(|start| start.elapsed().as_millis() as u64)
                    .unwrap_or(0);
                self.add_line(UiElement::ToolCall {
                    name: tool_name.clone(),
                    success: *success,
                    summary,
                    output: output.clone(),
                    collapsed: true,
                    elapsed_ms,
                    output_line_count,
                });
                // D4: push to sidebar recent tools (FIFO, max 5).
                self.sidebar_recent_tools.push((tool_name.clone(), *success));
                if self.sidebar_recent_tools.len() > 5 {
                    self.sidebar_recent_tools.remove(0);
                }
                self.status_msg = format!("Tool {} {} (step {}/{})", tool_name, if *success { "ok" } else { "failed" }, step + 1, self.max_steps);
            }
            AgentEvent::Approval { tool_name, decision, message: _ } => {
                // In YOLO/edits mode, skip the initial "Ask" line — it's auto-approved silently
                if decision == "Ask" && (self.mode == AgentMode::Yolo || self.mode == AgentMode::AcceptEdits) {
                    // Don't add the Ask line; auto-approval message is shown via ApprovalRequired handler
                    return;
                }
                self.add_line(UiElement::Approval {
                    tool: tool_name.clone(),
                    decision: decision.clone(),
                });
            }
            AgentEvent::Message { role: _, content } => {
                if !content.is_empty() {
                    if let Some(idx) = self.streaming_idx {
                        // Replace the streaming element with final content
                        if let Some(elem) = self.lines.get_mut(idx) {
                            *elem = UiElement::Assistant { text: content.clone() };
                        } else {
                            // idx 失效（如 /clear 后），回退到追加
                            self.add_line(UiElement::Assistant { text: content.clone() });
                        }
                        self.streaming_idx = None;
                    } else {
                        // No active stream — add as new message
                        self.add_line(UiElement::Assistant { text: content.clone() });
                    }
                }
            }
            AgentEvent::StepCompleted { step, total_steps } => {
                self.current_step = *step;
                // D4: update sidebar step tracking.
                self.sidebar_current_step = *step;
                self.sidebar_total_steps = *total_steps;
            }
            AgentEvent::Finished { answer, steps } => {
                if let Some(idx) = self.streaming_idx {
                    if self.lines.get(idx).is_some() {
                        // Replace streaming element with final answer
                        if !answer.is_empty() {
                            if let Some(elem) = self.lines.get_mut(idx) {
                                *elem = UiElement::Assistant { text: answer.clone() };
                            }
                        } else {
                            // Remove empty streaming placeholder
                            self.lines.remove(idx);
                        }
                    } else if !answer.is_empty() {
                        // idx 失效，回退到追加
                        let should_add = match self.lines.last() {
                            Some(UiElement::Assistant { text }) if text == answer => false,
                            _ => true,
                        };
                        if should_add {
                            self.add_line(UiElement::Assistant { text: answer.clone() });
                        }
                    }
                    self.streaming_idx = None;
                } else if !answer.is_empty() {
                    // Only add if last line isn't already the same answer
                    let should_add = match self.lines.last() {
                        Some(UiElement::Assistant { text }) if text == answer => false,
                        _ => true,
                    };
                    if should_add {
                        self.add_line(UiElement::Assistant { text: answer.clone() });
                    }
                }

                // Generate ToolTimeline from tool results since the last User message
                let user_idx = self
                    .lines
                    .iter()
                    .rposition(|l| matches!(l, UiElement::User { .. }))
                    .unwrap_or(0);
                let tools: Vec<(String, bool, u64)> = self.lines[user_idx..]
                    .iter()
                    .filter_map(|l| match l {
                        UiElement::ToolCall { name, success, elapsed_ms, .. } => {
                            Some((name.clone(), *success, *elapsed_ms))
                        }
                        _ => None,
                    })
                    .collect();
                if !tools.is_empty() {
                    let total_ms: u64 = tools.iter().map(|(_, _, ms)| *ms).sum();
                    self.add_line(UiElement::ToolTimeline { tools, total_ms });
                }

                self.is_processing = false;
                self.processing_start = None;
                self.streaming_idx = None;
                // D4: finalise sidebar step count.
                self.sidebar_total_steps = *steps;
                self.status_msg = format!("Done in {} steps.", steps);
                // P2: 交付排队消息（steering 优先，其次 follow-up）。
                // is_processing 已置 false，submit_prompt 可重新触发 agent。
                if let Some(msg) = dequeue_next_from(&mut self.message_queue) {
                    let label = match msg.kind {
                        QKind::Steering => "steering",
                        QKind::FollowUp => "follow-up",
                    };
                    self.add_line(UiElement::System {
                        text: format!("📨 Delivering queued {} message…", label),
                    });
                    self.pending_submit = Some(msg.text);
                }
            }
            AgentEvent::Error { message } => {
                self.add_line(UiElement::Error { text: message.clone() });
                self.is_processing = false;
                self.processing_start = None;
                self.streaming_idx = None;
                self.status_msg = "Error occurred.".to_string();
            }
            AgentEvent::MessageDelta { delta } => {
                // M7 改进: 字符级流式渲染 — 逐字追加，不插入人工空格
                if let Some(idx) = self.streaming_idx {
                    if let Some(UiElement::AssistantStream { text }) = self.lines.get_mut(idx) {
                        text.push_str(delta);
                        self.status_msg = format!("Streaming: {}…", {
                            let t: String = text.chars().take(40).collect();
                            t
                        });
                        return;
                    }
                }
                // No active stream — start one.
                let idx = self.lines.len();
                self.lines.push(UiElement::AssistantStream {
                    text: delta.clone(),
                });
                self.streaming_idx = Some(idx);
                self.status_msg = format!("Streaming: {}…", delta);
            }
            AgentEvent::Usage { input_tokens, output_tokens } => {
                self.input_tokens = *input_tokens;
                self.output_tokens = *output_tokens;
                self.status_msg = format!("Usage: {}↑ {}↓ tokens", input_tokens, output_tokens);
            }
            AgentEvent::Cancelled { partial_answer: _ } => {
                self.add_line(UiElement::Error { text: "Agent cancelled by user.".to_string() });
                self.is_processing = false;
                self.processing_start = None;
                self.streaming_idx = None;
                self.status_msg = "Cancelled.".to_string();
            }
            AgentEvent::ApprovalRequired {
                tool_name,
                tool_type,
                summary,
                risk_level,
                command,
                path,
                url,
                ..
            } => {
                // In YOLO mode: auto-approve all tool calls without dialog
                if self.mode == AgentMode::Yolo {
                    let decision = ApprovalDecision {
                        approved: true,
                        approve_all: true,
                        scope: ApprovalScope::Session,
                    };
                    // Send decision back through the oneshot channel
                    if let Ok(mut guard) = self.approval_responder.try_lock() {
                        if let Some(sender) = guard.take() {
                            let _ = sender.send(decision);
                        }
                    }
                    self.add_line(UiElement::System {
                        text: format!("⚡ Auto-approved: {}", tool_name),
                    });
                    self.is_processing = true;
                    self.status_msg = "Auto-approving...".to_string();
                } else {
                    // In other modes: show approval dialog
                    let request = ApprovalRequest {
                        tool_name: tool_name.clone(),
                        tool_type: tool_type.clone(),
                        summary: summary.clone(),
                        risk_level: risk_level.clone(),
                        command: command.clone().unwrap_or_default(),
                        path: path.clone().unwrap_or_default(),
                        url: url.clone().unwrap_or_default(),
                    };
                    self.approval_dialog = Some((request, ApprovalChoice::Pending));
                    self.is_processing = false;
                    self.add_line(UiElement::System {
                        text: format!("🛡 Approval required for: {}", tool_name),
                    });
                    self.status_msg = format!("Approval needed: {} ({})", tool_name, risk_level);
                }
            }
            AgentEvent::ContextCompacted { level, messages_before, messages_after, estimated_tokens } => {
                // M6: 压缩完成 — 显示一行紧凑提示
                self.add_line(UiElement::System {
                    text: format!(
                        "⊟ Context compacted ({level}): {before} → {after} messages (~{tokens}k tokens)",
                        level = level,
                        before = messages_before,
                        after = messages_after,
                        tokens = estimated_tokens / 1000,
                    ),
                });
                self.status_msg = format!("Context compacted ({})", level);
            }
            AgentEvent::ToolFailed { tool_name, error, failure_type } => {
                // M15: 工具失败分类提示
                let hint = match failure_type.as_str() {
                    "retryable" => " 🌐 retryable",
                    "argument" => " ⚠ bad args",
                    _ => " ❌ fatal",
                };
                let short_error = if error.len() > 120 {
                    format!("{}…", &error[..120])
                } else {
                    error.clone()
                };
                self.add_line(UiElement::System {
                    text: format!("✗ {} failed{}: {}", tool_name, hint, short_error),
                });
            }
            AgentEvent::FileChanged { .. } => {
                // D1-T01: TUI 暂不处理文件变更事件（桌面端 Changes 面板消费）
            }
        }
    }
}

// ── TUI Entry Point ──

pub async fn run_tui(
    agent: Arc<ReActAgent>,
    conversation: Arc<ConversationManager>,
    event_rx: broadcast::Receiver<BusEvent>,
    workspace: String,
    mode: AgentMode,
    switch: Arc<ProviderSwitch>,
    approval_responder: ApprovalResponder,
    cancel_token: CancellationToken,
    review_provider: Option<Arc<dyn LlmProvider>>,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let _ = execute!(stdout, SetCursorStyle::BlinkingBar);
    let _ = execute!(stdout, Show);
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(agent, conversation, event_rx, workspace, mode, switch, approval_responder, cancel_token, review_provider);
    app.init().await;

    let result = run_event_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        // ── Poll Agent Events ──
        loop {
            match app.event_rx.try_recv() {
                Ok(bus_event) => {
                    if bus_event.source == "agent" {
                        if let Ok(agent_event) = serde_json::from_value::<AgentEvent>(bus_event.data.clone()) {
                            app.handle_agent_event(&agent_event);
                        }
                    } else if bus_event.source == "ask_user" {
                        if let Ok(data) = serde_json::from_value::<serde_json::Value>(bus_event.data) {
                            let question = data["question"].as_str().unwrap_or("").to_string();
                            let header = data["header"].as_str().unwrap_or("Question").to_string();
                            let options = data["options"].as_array();

                            app.add_line(UiElement::AskUser {
                                header: header.clone(),
                                question: question.clone(),
                            });

                            if let Some(opts) = options {
                                for (i, opt) in opts.iter().enumerate() {
                                    let label = opt["label"].as_str().unwrap_or("");
                                    let desc = opt["description"].as_str().unwrap_or("");
                                    app.add_line(UiElement::System {
                                        text: format!("  [{}/{}] {} — {}", i + 1, opts.len(), label, desc),
                                    });
                                }
                            }
                            app.add_line(UiElement::System {
                                text: "  (respond by typing your answer in the input line — TUI reply mechanism is wip)".into(),
                            });
                        }
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    // P1: don't pollute chat — show in status bar only
                    app.status_msg = format!("⚠ bus lagged ({} skipped)", n);
                }
                Err(_) => break,
            }
        }

        // P2: 交付排队消息（由 Finished 事件设置 pending_submit）。
        // 放在事件排空之后、绘制之前，确保新提交在本次绘制中可见。
        if let Some(prompt) = app.pending_submit.take() {
            app.add_line(UiElement::User { text: prompt.clone() });
            app.submit_prompt(&prompt).await;
        }

        // ── Cursor visibility follows processing state ──
        if app.is_processing {
            let _ = execute!(terminal.backend_mut(), Hide);
        } else {
            let _ = execute!(terminal.backend_mut(), Show);
            let _ = execute!(terminal.backend_mut(), SetCursorStyle::BlinkingBar);
        }

        // ── Draw ──
        terminal.draw(|f| ui(f, app))?;

        // ── Poll Keyboard ──
        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
                Event::Paste(text) => {
                    // Bracketed paste: 保留换行（多行输入支持），仅清理 NUL 字节并归一化 CRLF。
                    // 80ms paste-burst guard 已移除 — bracketed paste 是精确区分粘贴/手输的机制，
                    // 不再需要启发式判断。
                    let cleaned: String = text
                        .chars()
                        .filter(|c| *c != '\0')
                        .collect::<String>()
                        .replace("\r\n", "\n")
                        .replace('\r', "\n");
                    if cleaned.is_empty() {
                        continue;
                    }
                    // Input length guard — prevent OOM / O(n²) performance collapse
                    let char_count = app.input.chars().count();
                    const INPUT_MAX: usize = 10_000;
                    let paste_chars = cleaned.chars().count();
                    if char_count + paste_chars > INPUT_MAX {
                        let available = INPUT_MAX.saturating_sub(char_count);
                        if available == 0 {
                            app.status_msg = "Input full (10,000 chars limit)".to_string();
                            continue;
                        }
                        let truncated: String = cleaned.chars().take(available).collect();
                        app.status_msg = format!("Paste truncated to fit 10k char limit ({} remaining)", available);
                        app.handle_paste(&truncated);
                        continue;
                    }
                    app.handle_paste(&cleaned);
                }
                Event::Mouse(mouse) => {
                    const SCROLL_LINES: usize = 3;
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            app.scroll_down(SCROLL_LINES);
                        }
                        MouseEventKind::ScrollUp => {
                            app.scroll_up(SCROLL_LINES);
                        }
                        // TUI-10: click to toggle collapse on ToolCard/ToolResult
                        MouseEventKind::Down(mouse_button) if mouse_button == MouseButton::Left => {
                            // Calculate which chat line was clicked.
                            // mouse.row is absolute screen row; chat area starts at chunks[1].y
                            // We need the layout — approximate: title=1, then chat starts at row 1.
                            // The exact offset depends on layout, but mouse.row - 1 gives
                            // the line within the chat area (title bar is 1 line).
                            let chat_area_top = 1u16; // title bar is 1 line
                            if mouse.row >= chat_area_top {
                                let click_line = (mouse.row - chat_area_top) as usize;
                                app.toggle_collapse_at_line(click_line);
                            }
                        }
                        _ => {}
                    }
                }
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Release {
                        continue;
                    }

                    // ── Settings dialog key handling (T3-#15) ──
                    // Placed BEFORE global Ctrl+C so Ctrl+C inside the dialog
                    // discards & closes instead of triggering exit logic.
                    if app.settings_dialog.is_some() {
                        let ctrl_s = key.code == KeyCode::Char('s')
                            && key.modifiers.contains(KeyModifiers::CONTROL);
                        let ctrl_c = key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL);
                        if ctrl_s {
                            match app.save_settings_from_dialog() {
                                Ok(()) => {
                                    app.add_line(UiElement::System {
                                        text: "💾 Settings saved to ~/.goat/setting.json — /reload or restart to apply.".into(),
                                    });
                                    app.settings_dialog = None;
                                    app.status_msg = "Settings saved".to_string();
                                }
                                Err(e) => {
                                    app.add_line(UiElement::Error {
                                        text: format!("Save failed: {}", e),
                                    });
                                }
                            }
                            continue;
                        }
                        if ctrl_c {
                            // Ctrl+C inside settings dialog: discard & close
                            app.settings_dialog = None;
                            app.status_msg = "Settings dialog closed (discarded)".to_string();
                            continue;
                        }
                        match key.code {
                            KeyCode::Up => {
                                if let Some(d) = app.settings_dialog.as_mut() {
                                    d.move_up();
                                }
                                continue;
                            }
                            KeyCode::Down => {
                                if let Some(d) = app.settings_dialog.as_mut() {
                                    d.move_down();
                                }
                                continue;
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                if let Some(d) = app.settings_dialog.as_mut() {
                                    d.inc();
                                }
                                continue;
                            }
                            KeyCode::Char('-') | KeyCode::Char('_') => {
                                if let Some(d) = app.settings_dialog.as_mut() {
                                    d.dec();
                                }
                                continue;
                            }
                            KeyCode::Esc => {
                                app.settings_dialog = None;
                                app.status_msg = "Settings dialog closed".to_string();
                                continue;
                            }
                            _ => {
                                // Consume all other keys while dialog is open
                                continue;
                            }
                        }
                    }

                    // ── Ctrl+C double-tap (global) ──
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        app.ctrl_c_count += 1;
                        if app.ctrl_c_count >= 2 {
                            return Ok(());
                        }
                        // M9 P0 修复: 通知 Agent 的 CancellationToken
                        app.cancel_token.cancel();
                        if app.is_processing {
                            app.add_line(UiElement::System {
                                text: "⏸ Ctrl+C pressed once — cancelling current request (press again to exit)"
                                    .into(),
                            });
                            app.is_processing = false;
                            app.processing_start = None;
                            app.streaming_idx = None;
                            app.status_msg = "Cancelled.".to_string();
                        } else {
                            app.add_line(UiElement::System {
                                text: "⏸ Press Ctrl+C again to exit, or Esc to exit now.".into(),
                            });
                        }
                        continue;
                    }

                    // Reset Ctrl+C counter on any other key
                    app.ctrl_c_count = 0;

                    // ── Ctrl+O: toggle focus between Input and Chat (TUI-3) ──
                    if key.code == KeyCode::Char('o')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        app.focus = match app.focus {
                            FocusTarget::Input => FocusTarget::Chat,
                            FocusTarget::Chat => FocusTarget::Input,
                        };
                        continue;
                    }

                    // ── Approval dialog key handling ──
                    if let Some((_, ref choice)) = app.approval_dialog {
                        if *choice == ApprovalChoice::Pending {
                            let decision = match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    Some(ApprovalDecision {
                                        approved: true,
                                        approve_all: false,
                                        scope: ApprovalScope::Once,
                                    })
                                }
                                KeyCode::Char('a') | KeyCode::Char('A') => {
                                    Some(ApprovalDecision {
                                        approved: true,
                                        approve_all: true,
                                        scope: ApprovalScope::Session,
                                    })
                                }
                                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                    Some(ApprovalDecision {
                                        approved: false,
                                        approve_all: false,
                                        scope: ApprovalScope::Once,
                                    })
                                }
                                _ => None,
                            };
    
                            if let Some(decision) = decision {
                                // Send decision back through the oneshot channel
                                if let Ok(mut guard) = app.approval_responder.try_lock() {
                                    if let Some(sender) = guard.take() {
                                        let _ = sender.send(decision.clone());
                                    }
                                }
                                let action = if decision.approved {
                                    "Approved"
                                } else {
                                    "Rejected"
                                };
                                app.add_line(UiElement::System {
                                    text: format!("🛡 {} — resuming agent…", action),
                                });
                                app.approval_dialog = None;
                                app.is_processing = true;
                                app.status_msg = format!("{} — resuming…", action);
                            }
                            continue; // Consume ALL keys while dialog is pending
                        }
                    }

                    // D4: F2 toggles the work progress sidebar (works in any focus).
                    if key.code == KeyCode::F(2) {
                        app.show_work_sidebar = !app.show_work_sidebar;
                        app.status_msg = if app.show_work_sidebar {
                            "Work sidebar shown".into()
                        } else {
                            "Work sidebar hidden".into()
                        };
                        continue;
                    }

                    // ── T1-#4: File picker key handling ──
                    if app.file_picker_active {
                        match key.code {
                            KeyCode::Esc => {
                                app.deactivate_file_picker();
                            }
                            KeyCode::Enter => {
                                if !app.file_picker_matches.is_empty() {
                                    let idx = app.file_picker_index.min(app.file_picker_matches.len() - 1);
                                    let path = app.file_picker_matches[idx].clone();
                                    app.insert_file_ref(&path);
                                } else {
                                    app.deactivate_file_picker();
                                }
                            }
                            KeyCode::Up => {
                                if app.file_picker_index > 0 {
                                    app.file_picker_index -= 1;
                                }
                            }
                            KeyCode::Down => {
                                let max = app.file_picker_matches.len().saturating_sub(1);
                                if app.file_picker_index < max {
                                    app.file_picker_index += 1;
                                }
                            }
                            KeyCode::Backspace => {
                                app.file_picker_query.pop();
                                if app.file_picker_query.is_empty() {
                                    app.deactivate_file_picker();
                                } else {
                                    app.file_picker_matches = App::fuzzy_filter(&app.scan_project_files(), &app.file_picker_query);
                                    app.file_picker_index = 0;
                                }
                            }
                            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.file_picker_query.push(c);
                                let all_files = app.scan_project_files();
                                app.file_picker_matches = App::fuzzy_filter(&all_files, &app.file_picker_query);
                                app.file_picker_index = 0;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // ── Normal key handling ──
                    // Chat focus: navigation keys go to conversation, Esc returns to Input
                    if app.focus == FocusTarget::Chat {
                        // TUI-9: search mode — when active, intercept keys for search input
                        if app.search_active {
                            match key.code {
                                KeyCode::Esc => {
                                    app.search_active = false;
                                    app.search_query.clear();
                                    app.search_matches.clear();
                                }
                                KeyCode::Enter => {
                                    app.execute_search();
                                    if app.search_matches.is_empty() {
                                        app.set_scroll_hint("No matches".into());
                                    } else {
                                        app.set_scroll_hint(
                                            format!("Found {} matches", app.search_matches.len())
                                        );
                                    }
                                }
                                KeyCode::Backspace => {
                                    app.search_query.pop();
                                }
                                KeyCode::Char('n') => {
                                    if !app.search_matches.is_empty() {
                                        app.search_next();
                                    }
                                }
                                KeyCode::Char('N') => {
                                    if !app.search_matches.is_empty() {
                                        app.search_prev();
                                    }
                                }
                                KeyCode::Char(c) => {
                                    app.search_query.push(c);
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // TUI-9: '/' enters search mode
                        if key.code == KeyCode::Char('/') && key.modifiers.is_empty() {
                            app.start_search();
                            continue;
                        }

                        // vim-style: Ctrl+U/D (half page), Ctrl+B/F (full page)
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            match key.code {
                                KeyCode::Char('u') => { app.scroll_up(15); continue; }
                                KeyCode::Char('d') => { app.scroll_down(15); continue; }
                                KeyCode::Char('b') => { app.scroll_up(30); continue; }
                                KeyCode::Char('f') => { app.scroll_down(30); continue; }
                                _ => {}
                            }
                        }
                        match key.code {
                            KeyCode::Esc => {
                                app.focus = FocusTarget::Input;
                            }
                            // vim-style j/k
                            KeyCode::Char('j') => app.scroll_down(1),
                            KeyCode::Char('k') => app.scroll_up(1),
                            // G = jump to bottom, g = jump to top
                            KeyCode::Char('G') => app.scroll_to_bottom(),
                            KeyCode::Char('g') => app.scroll_to_top(),
                            KeyCode::Up => app.scroll_up(1),
                            KeyCode::Down => app.scroll_down(1),
                            KeyCode::PageUp => app.scroll_up(10),
                            KeyCode::PageDown => app.scroll_down(10),
                            KeyCode::Home => app.scroll_to_top(),
                            KeyCode::End => app.scroll_to_bottom(),
                            _ => {}
                        }
                        continue;
                    }

                    match (key.code, key.modifiers) {
                        // Esc: close menu / interrupt agent / restore queue / exit
                        (KeyCode::Esc, _) => {
                            if app.show_command_menu {
                                app.show_command_menu = false;
                            } else if app.is_processing {
                                // P2: 中断 agent；若有排队消息则恢复到 input。
                                app.cancel_token.cancel();
                                app.is_processing = false;
                                app.processing_start = None;
                                app.streaming_idx = None;
                                if !app.message_queue.is_empty() {
                                    app.restore_queue_to_input();
                                }
                                app.add_line(UiElement::System {
                                    text: "⏸ Interrupted — agent cancelled.".into(),
                                });
                                app.status_msg = "Interrupted.".to_string();
                            } else if app.input.is_empty() && app.message_queue.is_empty() {
                                return Ok(());
                            } else if !app.input.is_empty() {
                                app.input.clear();
                                app.input_cursor = 0;  // P0 fix: reset cursor on clear
                                app.show_command_menu = false;
                                // P2: 退出 history 浏览状态（对齐 pi Esc 行为）
                                app.history_index = None;
                                app.history_draft = None;
                                app.last_action_is_word_char = false;
                            } else {
                                // input 空但队列非空：恢复队列到 input
                                app.restore_queue_to_input();
                            }
                        }
    
                        // Shift+Tab: cycle thinking level (must be before Tab)
                        (KeyCode::Tab, m) if m.contains(KeyModifiers::SHIFT) => {
                            app.cycle_thinking();
                        }

                        // Tab: auto-complete command from menu, path completion, or cycle mode
                        (KeyCode::Tab, _) => {
                            if app.show_command_menu && app.input.starts_with('/') {
                                let input_lower = app.input.to_lowercase();
                                let matches: Vec<_> = app.command_list()
                                    .into_iter()
                                    .filter(|(full, _, _)| full.to_lowercase().starts_with(&input_lower))
                                    .collect();
                                if !matches.is_empty() {
                                    let idx = app.command_menu_index.min(matches.len() - 1);
                                    app.input = matches[idx].0.to_string();
                                    app.input_cursor = app.input.chars().count();
                                    app.show_command_menu = false;
                                }
                            } else if !app.input.starts_with('/') {
                                // Try path completion
                                if let Some(prefix) = extract_path_prefix(&app.input, app.input_cursor) {
                                    // Only trigger path completion if prefix looks like a path
                                    let looks_like_path = prefix.contains('/')
                                        || prefix.contains('.')
                                        || prefix.contains('\\');
                                    if looks_like_path {
                                        if let Some(matches) = complete_path(&prefix) {
                                            if matches.len() == 1 {
                                                // Unique match: replace prefix in input
                                                let full = &matches[0];
                                                let before_prefix: String = app.input.chars().take(app.input_cursor.saturating_sub(prefix.chars().count())).collect();
                                                let after_cursor: String = app.input.chars().skip(app.input_cursor).collect();
                                                app.input = format!("{}{}{}", before_prefix, full, after_cursor);
                                                app.input_cursor = before_prefix.chars().count() + full.chars().count();
                                            } else {
                                                // Multiple matches: show completions
                                                let display: Vec<String> = matches.iter().take(20).cloned().collect();
                                                let suffix = if matches.len() > 20 { " …" } else { "" };
                                                app.add_line(UiElement::System {
                                                    text: format!("Completions: {}{}", display.join("  "), suffix),
                                                });
                                            }
                                        }
                                    } else {
                                        app.cycle_mode();
                                    }
                                } else {
                                    app.cycle_mode();
                                }
                            } else {
                                app.cycle_mode();
                            }
                        }
    
                        // Shift+Enter: 插入换行（多行输入）。
                        // crossterm 在 Kitty 协议终端能识别 SHIFT 修饰符；其他终端走 \+Enter fallback。
                        (KeyCode::Enter, m) if m.contains(KeyModifiers::SHIFT) => {
                            app.insert_newline_at_cursor();
                        }

                        // 某些终端的 Shift+Enter 发送单字符 LF（\n），crossterm 解析为 Char('\n')。
                        (KeyCode::Char('\n'), _) => {
                            app.insert_newline_at_cursor();
                        }

                        // Alt+Enter: 排队为 follow-up 消息（任何时候都排队）
                        (KeyCode::Enter, m) if m.contains(KeyModifiers::ALT) => {
                            let prompt = std::mem::take(&mut app.input);
                            app.input_cursor = 0;
                            app.show_command_menu = false;
                            app.command_menu_index = 0;
                            if prompt.trim().is_empty() {
                                // 空输入不排队，放回 input
                                app.input = prompt;
                            } else {
                                app.enqueue_message(QKind::FollowUp, prompt);
                            }
                        }

                        // Submit
                        (KeyCode::Enter, _) => {
                            // \+Enter workaround（终端不支持 Shift+Enter 时的标准 fallback，对齐 pi editor.ts:807）：
                            // cursor 前一字符是 `\` 时，删除 `\` 并插入换行，而非提交。
                            {
                                let chars: Vec<char> = app.input.chars().collect();
                                let cursor = app.input_cursor.min(chars.len());
                                if cursor > 0 && chars[cursor - 1] == '\\' {
                                    let byte_pos = app.input
                                        .char_indices()
                                        .nth(cursor - 1)
                                        .map(|(i, _)| i)
                                        .unwrap_or(app.input.len());
                                    app.input.remove(byte_pos);  // 删 `\`
                                    app.input_cursor -= 1;
                                    app.insert_newline_at_cursor();
                                    continue;
                                }
                            }
                            // If command menu is open and there's a selection, fill the command
                            if app.show_command_menu && app.input.starts_with('/') {
                                let input_lower = app.input.to_lowercase();
                                let commands = app.command_list();
                                let matches: Vec<&(String, String, String)> = commands
                                    .iter()
                                    .filter(|(full, _, _)| full.to_lowercase().starts_with(&input_lower))
                                    .collect();
                                if !matches.is_empty() {
                                    let idx = app.command_menu_index.min(matches.len() - 1);
                                    app.input = matches[idx].0.clone();
                                    app.input_cursor = app.input.chars().count();
                                    app.show_command_menu = false;
                                    // Don't submit yet — let user see the filled command and press Enter again
                                    continue;
                                }
                            }
                            app.show_command_menu = false;
                            if app.input.trim().is_empty() {
                                // No input — toggle collapse of the last ToolCall or Thought
                                if let Some(collapsed) = app.lines.iter_mut().rev().find_map(|l| {
                                        match l {
                                            UiElement::ToolCall { collapsed, .. } => Some(collapsed),
                                            _ => None,
                                        }
                                    }) {
                                    *collapsed = !*collapsed;
                                } else if let Some(collapsed) = app.lines.iter_mut().rev().find_map(|l| {
                                    match l {
                                        UiElement::Thought { collapsed, .. } => Some(collapsed),
                                        _ => None,
                                    }
                                }) {
                                    *collapsed = !*collapsed;
                                }
                            }
                            app.submit().await;
                        }
    
                        // Alt+Backspace: delete previous word (Emacs, 对齐 pi deleteWordBackwards)
                        // 与 Ctrl+W 行为一致，压 kill ring + push_undo
                        (KeyCode::Backspace, m) if m.contains(KeyModifiers::ALT) => {
                            let cursor = app.input_cursor.min(app.input.chars().count());
                            let chars: Vec<char> = app.input.chars().collect();
                            let mut i = cursor;
                            while i > 0 && chars[i - 1].is_whitespace() { i -= 1; }
                            while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '-') {
                                i -= 1;
                            }
                            if i < cursor {
                                app.push_undo();
                                app.last_action_is_word_char = false;
                                let pre_byte: usize = app.input.chars().take(i).collect::<String>().len();
                                let del_byte: usize = app.input.chars().take(cursor).collect::<String>().len();
                                let deleted: String = app.input[pre_byte..del_byte].to_string();
                                app.push_kill_ring(deleted);
                                let post: String = app.input.chars().skip(cursor).collect();
                                app.input.truncate(pre_byte);
                                app.input.push_str(&post);
                                app.input_cursor = i;
                                app.show_command_menu = app.input.starts_with('/');
                                app.preferred_visual_col = None;
                            }
                        }

                        // Backspace
                        (KeyCode::Backspace, _) => {
                            let char_count = app.input.chars().count();
                            if !app.input.is_empty() && app.input_cursor > 0 && app.input_cursor <= char_count {
                                app.push_undo();
                                app.last_action_is_word_char = false;
                                let char_indices: Vec<(usize, char)> = app.input.char_indices().collect();
                                let (byte_pos, _) = char_indices[app.input_cursor - 1];
                                app.input.remove(byte_pos);
                                app.input_cursor -= 1;
                            } else if app.input_cursor > char_count {
                                // 防御性修复：cursor 超出范围时强制校正
                                app.input_cursor = char_count;
                            }
                            // Update command menu visibility: show if input starts with /
                            app.show_command_menu = app.input.starts_with('/');
                        }

                        // Delete (forward delete)
                        (KeyCode::Delete, _) => {
                            let char_count = app.input.chars().count();
                            if !app.input.is_empty() && app.input_cursor < char_count {
                                app.push_undo();
                                app.last_action_is_word_char = false;
                                let char_indices: Vec<(usize, char)> = app.input.char_indices().collect();
                                let (byte_pos, _) = char_indices[app.input_cursor];
                                app.input.remove(byte_pos);
                            }
                            if app.input_cursor > char_count {
                                app.input_cursor = char_count;
                            }
                            app.show_command_menu = app.input.starts_with('/');
                        }

                        // ── Ctrl+letter handlers (BEFORE general Char to avoid inserting text) ──
                        (KeyCode::Char(c), m) if m.contains(KeyModifiers::CONTROL) => {
                            match c {
                                // Ctrl+Z: undo（对齐 pi editor.ts undo）
                                'z' => {
                                    if let Some((prev_input, prev_cursor)) = app.undo_stack.pop() {
                                        app.input = prev_input;
                                        app.input_cursor = prev_cursor;
                                        app.show_command_menu = app.input.starts_with('/');
                                        app.preferred_visual_col = None;
                                        app.last_action_is_word_char = false;
                                    }
                                }
                                // Ctrl+U: readline 语义 — 删除当前行 cursor 前的内容（多行时只删当前行）。
                                // 删除内容压入 kill ring（对齐 pi editor.ts deleteToLineStart）。
                                'u' => {
                                    let chars: Vec<char> = app.input.chars().collect();
                                    let cursor = app.input_cursor.min(chars.len());
                                    let line_start = chars[..cursor].iter().rposition(|&c| c == '\n')
                                        .map(|p| p + 1).unwrap_or(0);
                                    if cursor > line_start {
                                        app.push_undo();
                                        app.last_action_is_word_char = false;
                                        let pre_byte: usize = app.input.chars().take(line_start).collect::<String>().len();
                                        let del_byte: usize = app.input.chars().take(cursor).collect::<String>().len();
                                        let deleted: String = app.input[pre_byte..del_byte].to_string();
                                        app.push_kill_ring(deleted);
                                        let after: String = app.input.chars().skip(cursor).collect();
                                        app.input.truncate(pre_byte);
                                        app.input.push_str(&after);
                                        app.input_cursor = line_start;
                                    }
                                    app.show_command_menu = app.input.starts_with('/');
                                    app.preferred_visual_col = None;
                                }
                                // Ctrl+K: kill to line end（对齐 pi editor.ts deleteToLineEnd）。
                                // 删除 cursor 到当前行尾的内容，压入 kill ring。
                                'k' => {
                                    let chars: Vec<char> = app.input.chars().collect();
                                    let cursor = app.input_cursor.min(chars.len());
                                    let line_end = chars[cursor..].iter().position(|&c| c == '\n')
                                        .map(|p| cursor + p).unwrap_or(chars.len());
                                    if line_end > cursor {
                                        app.push_undo();
                                        app.last_action_is_word_char = false;
                                        let pre_byte: usize = app.input.chars().take(cursor).collect::<String>().len();
                                        let end_byte: usize = app.input.chars().take(line_end).collect::<String>().len();
                                        let deleted: String = app.input[pre_byte..end_byte].to_string();
                                        app.push_kill_ring(deleted);
                                        let after: String = app.input.chars().skip(line_end).collect();
                                        app.input.truncate(pre_byte);
                                        app.input.push_str(&after);
                                        app.show_command_menu = app.input.starts_with('/');
                                        app.preferred_visual_col = None;
                                    }
                                }
                                // Ctrl+Y: yank（从 kill ring 取最新项，对齐 pi editor.ts yank）。
                                'y' => {
                                    if let Some(text) = app.kill_ring.first().cloned() {
                                        let char_count = app.input.chars().count();
                                        if app.input_cursor > char_count {
                                            app.input_cursor = char_count;
                                        }
                                        app.push_undo();
                                        app.last_action_is_word_char = false;
                                        let byte_pos = app.input.char_indices().nth(app.input_cursor)
                                            .map(|(i, _)| i).unwrap_or(app.input.len());
                                        let start_char = app.input_cursor;
                                        app.input.insert_str(byte_pos, &text);
                                        app.input_cursor += text.chars().count();
                                        let end_char = app.input_cursor;
                                        app.last_yank = Some((start_char, end_char));
                                        app.show_command_menu = app.input.starts_with('/');
                                        app.preferred_visual_col = None;
                                    }
                                }
                                // Ctrl+A: 当前行行首（readline 语义，对齐 pi editor.ts cursorLineStart）。
                                'a' => {
                                    let chars: Vec<char> = app.input.chars().collect();
                                    let cursor = app.input_cursor.min(chars.len());
                                    let line_start = chars[..cursor].iter().rposition(|&c| c == '\n')
                                        .map(|p| p + 1).unwrap_or(0);
                                    app.input_cursor = line_start;
                                    app.preferred_visual_col = None;
                                }
                                // Ctrl+E: 当前行行尾（readline 语义，对齐 pi editor.ts cursorLineEnd）。
                                'e' => {
                                    let chars: Vec<char> = app.input.chars().collect();
                                    let cursor = app.input_cursor.min(chars.len());
                                    let line_end = chars[cursor..].iter().position(|&c| c == '\n')
                                        .map(|p| cursor + p).unwrap_or(chars.len());
                                    app.input_cursor = line_end;
                                    app.preferred_visual_col = None;
                                }
                                // Ctrl+W: delete previous word（对齐 pi deleteWordBackwards，压 kill ring）。
                                'w' => {
                                    let cursor = app.input_cursor.min(app.input.chars().count());
                                    let prefix: String = app.input.chars().take(cursor).collect();
                                    let trimmed_len = prefix.trim_end().len();
                                    if trimmed_len > 0 {
                                        let word_start = prefix[..trimmed_len]
                                            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                                            .map(|p| p + 1)
                                            .unwrap_or(0);
                                        if word_start < cursor {
                                            app.push_undo();
                                            app.last_action_is_word_char = false;
                                            let pre_byte: usize = app.input.chars().take(word_start).collect::<String>().len();
                                            let del_byte: usize = app.input.chars().take(cursor).collect::<String>().len();
                                            let deleted: String = app.input[pre_byte..del_byte].to_string();
                                            app.push_kill_ring(deleted);
                                            let post: String = app.input.chars().skip(cursor).collect();
                                            app.input.truncate(pre_byte);
                                            app.input.push_str(&post);
                                            app.input_cursor = word_start;
                                        }
                                    }
                                    app.show_command_menu = app.input.starts_with('/');
                                    app.preferred_visual_col = None;
                                }
                                // Ctrl+X: copy last assistant response (T1-#5)
                                'x' => {
                                    app.copy_last_assistant();
                                }
                                // Ctrl+G: open external editor
                                'g' => {
                                    app.open_external_editor();
                                }
                                _ => {}
                            }
                        }

                        // Alt+Y: yank-pop（替换上次 yank 的内容为 kill ring 下一个，对齐 pi yankPop）
                        // 必须在通用 Char 分支之前，否则会被 (KeyCode::Char(c), _) 吞掉。
                        (KeyCode::Char('y'), m) if m.contains(KeyModifiers::ALT) => {
                            if app.kill_ring.len() >= 2 {
                                if let Some((start_char, end_char)) = app.last_yank {
                                    // 轮转 kill ring：把第一个移到末尾，取新的第一个
                                    let first = app.kill_ring.remove(0);
                                    app.kill_ring.push(first);
                                    let text = app.kill_ring[0].clone();
                                    let pre_byte: usize = app.input.chars().take(start_char).collect::<String>().len();
                                    let after: String = app.input.chars().skip(end_char).collect();
                                    app.input.truncate(pre_byte);
                                    app.input.push_str(&text);
                                    app.input.push_str(&after);
                                    let new_end = start_char + text.chars().count();
                                    app.input_cursor = new_end;
                                    app.last_yank = Some((start_char, new_end));
                                    app.show_command_menu = app.input.starts_with('/');
                                    app.preferred_visual_col = None;
                                    app.last_action_is_word_char = false;
                                }
                            }
                        }
                        // Alt+D: delete word forward (Emacs, 对齐 pi deleteWordForward)
                        (KeyCode::Char('d'), m) if m.contains(KeyModifiers::ALT) => {
                            let cursor = app.input_cursor.min(app.input.chars().count());
                            let chars: Vec<char> = app.input.chars().collect();
                            let mut i = cursor;
                            while i < chars.len() && chars[i].is_whitespace() { i += 1; }
                            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-') {
                                i += 1;
                            }
                            if i > cursor {
                                app.push_undo();
                                app.last_action_is_word_char = false;
                                let pre_byte: usize = app.input.chars().take(cursor).collect::<String>().len();
                                let del_byte: usize = app.input.chars().take(i).collect::<String>().len();
                                let deleted: String = app.input[pre_byte..del_byte].to_string();
                                app.push_kill_ring(deleted);
                                let post: String = app.input.chars().skip(i).collect();
                                app.input.truncate(pre_byte);
                                app.input.push_str(&post);
                                app.show_command_menu = app.input.starts_with('/');
                                app.preferred_visual_col = None;
                            }
                        }

                        // Character input (non-Ctrl)
                        (KeyCode::Char(c), _) => {
                            let char_count = app.input.chars().count();
                            if app.input_cursor > char_count {
                                app.input_cursor = char_count;
                            }
                            // Input length guard — prevents O(n²) performance collapse
                            // from massive pastes and OOM from unbounded input growth.
                            // 10 KiB ≈ most reasonable prompts.  CodeWhale uses a similar cap.
                            if char_count >= 10000 {
                                app.status_msg = "Input limit reached (10,000 chars)".to_string();
                                continue;
                            }
                            // Undo coalescing: 连续 word char（非空白）合并为一个 undo 单元。
                            // 对齐 pi editor.ts UndoStack 的 coalescing 策略。
                            let is_word_char = !c.is_whitespace();
                            if !is_word_char || !app.last_action_is_word_char {
                                app.push_undo();
                            }
                            app.last_action_is_word_char = is_word_char;
                            let byte_pos = app.input.char_indices().nth(app.input_cursor).map(|(i, _)| i).unwrap_or(app.input.len());
                            app.input.insert(byte_pos, c);
                            app.input_cursor += 1;
                            app.show_command_menu = app.input.starts_with('/');

                            // T1-#4: trigger file picker on '@' (when picker is not already active)
                            if c == '@' && !app.file_picker_active {
                                // input_cursor 已前进到 @ 之后，@ 的 char index = cursor - 1
                                app.file_picker_trigger_pos = app.input_cursor.saturating_sub(1);
                                app.activate_file_picker();
                            }
                        }
    
                        // Ctrl+Left: 向前跳一词（跳过空白，再跳过连续 word char）
                        (KeyCode::Left, m) if m.contains(KeyModifiers::CONTROL) => {
                            let chars: Vec<char> = app.input.chars().collect();
                            let mut i = app.input_cursor.min(chars.len());
                            // 跳过空白
                            while i > 0 && chars[i - 1].is_whitespace() { i -= 1; }
                            // 跳过连续 word char（alphanumeric + _ + -）
                            while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '-') {
                                i -= 1;
                            }
                            app.input_cursor = i;
                            app.preferred_visual_col = None;
                        }
                        // Ctrl+Right: 向后跳一词（跳过连续 word char，再跳过空白）
                        (KeyCode::Right, m) if m.contains(KeyModifiers::CONTROL) => {
                            let chars: Vec<char> = app.input.chars().collect();
                            let mut i = app.input_cursor.min(chars.len());
                            // 跳过连续 word char
                            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-') {
                                i += 1;
                            }
                            // 跳过空白
                            while i < chars.len() && chars[i].is_whitespace() { i += 1; }
                            app.input_cursor = i;
                            app.preferred_visual_col = None;
                        }
                        // Cursor movement inside input
                        (KeyCode::Left, _) => {
                            if app.input_cursor > 0 {
                                app.input_cursor -= 1;
                            }
                            app.preferred_visual_col = None;
                        }
                        (KeyCode::Right, _) => {
                            let char_count = app.input.chars().count();
                            if app.input_cursor < char_count {
                                app.input_cursor += 1;
                            }
                            app.preferred_visual_col = None;
                        }
                        (KeyCode::Home, _) => {
                            // 当前行行首（对齐 pi cursorLineStart）
                            let chars: Vec<char> = app.input.chars().collect();
                            let cursor = app.input_cursor.min(chars.len());
                            let line_start = chars[..cursor].iter().rposition(|&c| c == '\n')
                                .map(|p| p + 1).unwrap_or(0);
                            app.input_cursor = line_start;
                            app.preferred_visual_col = None;
                        }
                        (KeyCode::End, _) => {
                            // 当前行行尾（对齐 pi cursorLineEnd）
                            let chars: Vec<char> = app.input.chars().collect();
                            let cursor = app.input_cursor.min(chars.len());
                            let line_end = chars[cursor..].iter().position(|&c| c == '\n')
                                .map(|p| cursor + p).unwrap_or(chars.len());
                            app.input_cursor = line_end;
                            app.preferred_visual_col = None;
                        }

                        // Alt+Up: 取回最后一条排队消息到 input
                        (KeyCode::Up, m) if m.contains(KeyModifiers::ALT) => {
                            app.pop_queue_to_input();
                        }

                        // Up: 命令菜单导航 / 多行跨行 / 单行历史导航（对齐 pi editor.ts:821）
                        (KeyCode::Up, _) => {
                            if app.show_command_menu {
                                if app.command_menu_index > 0 {
                                    app.command_menu_index -= 1;
                                }
                            } else if app.input.contains('\n') {
                                // Multi-line input: move cursor to previous line
                                app.move_cursor_line_up();
                            } else {
                                // 单行：历史导航（替代原 scroll_up(1)）
                                app.navigate_history(-1);
                            }
                        }
                        (KeyCode::Down, _) => {
                            if app.show_command_menu {
                                let input_lower = app.input.to_lowercase();
                                let match_count = app.command_list()
                                    .into_iter()
                                    .filter(|(full, _, _)| full.to_lowercase().starts_with(&input_lower))
                                    .count();
                                if app.command_menu_index < match_count.saturating_sub(1) {
                                    app.command_menu_index += 1;
                                }
                            } else if app.input.contains('\n') {
                                // Multi-line input: move cursor to next line
                                app.move_cursor_line_down();
                            } else {
                                // 单行：历史导航（替代原 scroll_down(1)）
                                app.navigate_history(1);
                            }
                        }
                        // PgUp / PgDn — page scrolling (CodeWhale-style)
                        (KeyCode::PageUp, _) => app.scroll_up(10),
                        (KeyCode::PageDown, _) => app.scroll_down(10),
    
                        _ => {}
                    }
                }
            _ => {}
            }
        }
    }
}

// ── UI Rendering ──

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Compute command menu height (visible only when user types /)
    let menu_height: u16 = if app.show_command_menu {
        let input_lower = app.input.to_lowercase();
        let match_count = app.command_list()
            .into_iter()
            .filter(|(full, _, _)| full.to_lowercase().starts_with(&input_lower))
            .count() as u16;
        // Header(1) + separator(1) + visible items(max 15) + scroll indicators(max 2) + border(2)
        let content_height = match_count.min(15) + 4;
        content_height.min(20)
    } else {
        0
    };

    // T1-#4: file picker height (visible when '@' picks files)
    let file_picker_height: u16 = if app.file_picker_active {
        let match_count = app.file_picker_matches.len() as u16;
        // border(2) + query line(1) + visible items(max 10)
        let content_height = match_count.min(10) + 3;
        content_height.min(15)
    } else {
        0
    };

    // Compute dynamic input height using CodeWhale's composer_height formula.
    // This is the single source of truth for how many rows the input panel
    // needs, matching the wrap logic used for rendering and cursor.
    // 宽度必须与 layout_input_with_scroll 一致（input_inner_width = area.width - 4 border），
    // 否则行数估算与实际 wrap 行数漂移，导致 input 区高度与内容不匹配。
    const MAX_INPUT_ROWS: usize = 10;
    const MIN_INPUT_ROWS: usize = 3;
    let input_prompt = format!("{} ▶ ", app.mode_display());
    let full_input = format!("{}{}", input_prompt, app.input);
    let input_inner_width_u16 = area.width.saturating_sub(4);
    let input_rows = composer_height(
        &full_input,
        input_inner_width_u16,
        MAX_INPUT_ROWS as u16 + 2,
        MIN_INPUT_ROWS,
        MAX_INPUT_ROWS,
    ) as u16;

    // Split: title(1) | chat(min 3) | status(1) | [file_picker(dynamic)] | [menu(dynamic)] | input(dynamic)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                     // title bar
            Constraint::Min(3),                        // chat area
            Constraint::Length(1),                     // status bar
            Constraint::Length(file_picker_height),    // file picker (0 when hidden) — T1-#4
            Constraint::Length(menu_height),           // command menu (0 when hidden)
            Constraint::Length(input_rows),            // input line (dynamic)
        ])
        .split(area);

    // ── Title Bar ──
    let session_short = if app.session_id.len() > 8 {
        app.session_id.chars().take(8).collect::<String>()
    } else {
        app.session_id.clone()
    };
    let provider_short = &app.current_provider;
    let status_icon = if app.is_processing { "◉" } else { "◎" };
    let title = format!(
        " rgoat {} | session:{} | provider:{} | mode:{} | {} ",
        rgoat_core::VERSION,
        session_short,
        provider_short,
        app.mode_display(),
        status_icon,
    );
    let title_p = Paragraph::new(title).style(Theme::style_title_bar());
    f.render_widget(title_p, chunks[0]);

    // D4: when the work sidebar is visible, split the chat area horizontally.
    // Sidebar width = 30% of chat area, clamped to [25, 40] columns.
    let (chat_area, sidebar_area) = if app.show_work_sidebar {
        let sw = (chunks[1].width as usize * 30 / 100).clamp(25, 40) as u16;
        let sub = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(sw)])
            .split(chunks[1]);
        (sub[0], Some(sub[1]))
    } else {
        (chunks[1], None)
    };

    // ── Chat Area ──
    let mut chat_lines: Vec<Line<'static>> = Vec::new();
    let chat_width = chat_area.width.saturating_sub(4);

    // TUI-8: record element→line mapping for search and click support
    app.element_line_starts.clear();

    for (_elem_idx, elem) in app.lines.iter().enumerate() {
        app.element_line_starts.push(chat_lines.len());
        match elem {
            UiElement::ToolCall { name, success, summary, output, collapsed, elapsed_ms, output_line_count: _ } => {
                let icon = if *success {
                    if *collapsed { "▸" } else { "▾" }
                } else {
                    "✗"
                };
                let color = if *success { Theme::SUCCESS_COLOR } else { Theme::ERROR_COLOR };
                let timing = if *elapsed_ms > 0 {
                    format!(" ({}ms)", elapsed_ms)
                } else {
                    String::new()
                };
                chat_lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(icon, Style::default().fg(color)),
                    Span::styled(" ", Style::default()),
                    Span::styled("🔧 ", Style::default().fg(color)),
                    Span::styled(name.clone(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    Span::styled(timing, Style::default().fg(Color::Rgb(120, 120, 130))),
                ]));
                if *collapsed {
                    chat_lines.push(Line::from(vec![
                        Span::styled("     ", Style::default()),
                        Span::styled(summary.clone(), Style::default().fg(Color::Rgb(140, 140, 150))),
                    ]));
                } else if !output.is_empty() {
                    for ol in output.lines().take(20) {
                        chat_lines.push(Line::from(vec![
                            Span::styled("     ", Style::default()),
                            Span::styled(ol.to_string(), Style::default().fg(Color::Rgb(140, 140, 150))),
                        ]));
                    }
                    if output.lines().count() > 20 {
                        chat_lines.push(Line::from(vec![
                            Span::styled("     ", Style::default()),
                            Span::styled(
                                format!("… {} more lines (Enter to collapse)", output.lines().count().saturating_sub(20)),
                                Style::default().fg(Color::Rgb(80, 80, 90)),
                            ),
                        ]));
                    }
                }
                chat_lines.push(Line::from(""));
            }
            UiElement::DiffView { path, additions, deletions, lines: diff_lines } => {
                chat_lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled("📄 ", Style::default().fg(Color::Rgb(6, 182, 212))),
                    Span::styled(path.clone(), Style::default().fg(Color::Rgb(6, 182, 212)).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!(" (+{}/−{})", additions, deletions),
                        Style::default().fg(Color::Rgb(100, 100, 110)),
                    ),
                ]));
                for dl in diff_lines.iter().take(8) {
                    let (prefix, content, style) = match dl {
                        DiffLine::Add(s) => ("  + ", s.clone(), Theme::style_diff_add()),
                        DiffLine::Del(s) => ("  - ", s.clone(), Theme::style_diff_del()),
                        DiffLine::Hunk(s) => ("  @@", s.clone(), Theme::style_diff_hunk()),
                        DiffLine::Context(s) => ("    ", s.clone(), Theme::style_diff_ctx()),
                    };
                    chat_lines.push(Line::from(Span::styled(
                        format!("{}{}", prefix, content),
                        style,
                    )));
                }
                if diff_lines.len() > 8 {
                    chat_lines.push(Line::from(Span::styled(
                        format!("  … {} more lines", diff_lines.len() - 8),
                        Style::default().fg(Color::Rgb(80, 80, 90)),
                    )));
                }
                chat_lines.push(Line::from(""));
            }
            UiElement::User { text } => {
                // User message — simple prefix, no border
                let header = format!("> {}", "You");
                chat_lines.push(Line::from(vec![
                    Span::styled(header, Theme::style_user()),
                ]));
                for line in text.lines() {
                    chat_lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(line.to_string(), Theme::style_user()),
                    ]));
                }
                chat_lines.push(Line::from(""));
            }
            UiElement::Assistant { text } => {
                // Assistant message — Markdown rendered
                let header = format!("◆ {}", "Goat");
                chat_lines.push(Line::from(vec![
                    Span::styled(header, Theme::style_assistant().add_modifier(Modifier::BOLD)),
                ]));
                if !text.is_empty() {
                    // Pad content with 2-space indent
                    let md_lines = MarkdownRenderer::render(text, chat_width);
                    for line in md_lines {
                        chat_lines.push(line);
                    }
                }
                chat_lines.push(Line::from(""));
            }
            UiElement::AssistantStream { text } => {
                // Streaming assistant message
                let header = format!("◆ {}", "Goat");
                chat_lines.push(Line::from(vec![
                    Span::styled(header, Theme::style_assistant().add_modifier(Modifier::BOLD)),
                ]));
                for line in text.lines() {
                    chat_lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(line.to_string(), Theme::style_streaming()),
                    ]));
                }
                chat_lines.push(Line::from(""));
            }
            _ => {
                chat_lines.push(elem.render_compact(chat_width));
            }
        }
    }

    // P0 v2 fix: scroll-from-bottom model (matches CodeWhale's approach).
    // scroll_from_bottom=0 means "pinned to the bottom" (auto-follow).
    // >0 means "scrolled up N lines from the bottom".
    // Because the offset is always relative to total_lines, the user's
    // view stays stable when new messages arrive in the background.
    let total_lines = chat_lines.len();
    let visible_height = chat_area.height as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);

    // TUI-9: apply pending scroll-to-line from search
    if let Some(target_line) = app.pending_scroll_to_line.take() {
        let center = visible_height / 2;
        let desired_start = target_line.saturating_sub(center);
        let clamped_start = desired_start.min(max_scroll);
        app.scroll_from_bottom = max_scroll.saturating_sub(clamped_start);
    }

    let start = max_scroll.saturating_sub(app.scroll_from_bottom);
    let end = (start + visible_height).min(total_lines);
    let visible_lines: Vec<Line> = chat_lines[start..end].to_vec();

    // Chat border: highlight when Chat has focus (TUI-3)
    let chat_border_color = if app.focus == FocusTarget::Chat {
        Color::Rgb(120, 160, 255) // bright blue when focused
    } else {
        Color::Rgb(30, 30, 35)    // subtle dark when not focused
    };

    // TUI-6: title shows real-time scroll position
    let title = if app.scroll_from_bottom == 0 {
        format!(" Conversation ({}) — following ", app.lines.len())
    } else if app.scroll_from_bottom >= max_scroll {
        format!(" Conversation ({}) — TOP ", app.lines.len())
    } else {
        format!(" Conversation ({}) [▲ {}/{}] ", app.lines.len(), start + 1, total_lines)
    };

    let chat = Paragraph::new(Text::from(visible_lines))
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(chat_border_color))
                .title(title)
                .title_style(Style::default().fg(Color::Rgb(100, 100, 110)))
        )
        .style(Style::default().bg(Theme::BG));
    f.render_widget(chat, chat_area);

    // D4: render the work progress sidebar when visible.
    if let Some(sidebar_rect) = sidebar_area {
        let data = crate::components::WorkSidebarData {
            current_step: app.sidebar_current_step,
            total_steps: app.sidebar_total_steps,
            progress_text: app.sidebar_progress_text.clone(),
            recent_tools: app.sidebar_recent_tools.clone(),
            is_processing: app.is_processing,
        };
        crate::components::WorkSidebar::render(&data, sidebar_rect, f);
    }

    // ── Status Bar (T02: component-ised) ──
    // TUI-5: check scroll hint expiry
    if let Some(expiry) = app.scroll_hint_expiry {
        if Instant::now() >= expiry {
            app.scroll_hint.clear();
            app.scroll_hint_expiry = None;
        }
    }
    let mut sd = app.status_data();
    // TUI-9: show search query in status bar when searching
    if app.search_active {
        let match_info = if app.search_matches.is_empty() {
            String::new()
        } else {
            format!(" ({}/{})", app.search_current + 1, app.search_matches.len())
        };
        sd.scroll_info = format!("Search: {}{}", app.search_query, match_info);
    } else {
        sd.scroll_info = app.compute_scroll_info(total_lines, visible_height);
        if !app.scroll_hint.is_empty() {
            sd.scroll_info = app.scroll_hint.clone();
        }
    }
    sd.focus = if app.focus == FocusTarget::Chat { "CHAT" } else { "INPUT" }.into();
    let status = StatusBar::render(&sd);
    f.render_widget(status, chunks[2]);

    // ── File Picker (shown when typing @ for file references) — T1-#4 ──
    if app.file_picker_active && file_picker_height > 0 {
        let mut picker_lines: Vec<Line> = Vec::new();

        // Query line
        picker_lines.push(Line::from(vec![
            Span::styled(" @", Style::default().fg(Theme::ACCENT_BRIGHT).add_modifier(Modifier::BOLD)),
            Span::styled(&app.file_picker_query, Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled("_", Style::default().fg(Theme::ACCENT).add_modifier(Modifier::SLOW_BLINK)),
        ]));
        picker_lines.push(Line::from(Span::styled(
            "─".repeat((chunks[3].width.saturating_sub(4)) as usize),
            Style::default().fg(Color::Rgb(60, 60, 70)),
        )));

        if app.file_picker_matches.is_empty() {
            picker_lines.push(Line::from(Span::styled(
                "  No matching files",
                Style::default().fg(Color::Rgb(100, 100, 110)),
            )));
        } else {
            let total = app.file_picker_matches.len();
            const VISIBLE: usize = 10;
            let selected = app.file_picker_index.min(total.saturating_sub(1));
            let (start, end) = if total <= VISIBLE {
                (0, total)
            } else if selected < VISIBLE / 2 {
                (0, VISIBLE)
            } else if selected + VISIBLE / 2 >= total {
                (total - VISIBLE, total)
            } else {
                (selected - VISIBLE / 2, selected + VISIBLE / 2 + 1)
            };
            if start > 0 {
                picker_lines.push(Line::from(Span::styled(
                    format!("  ↑ {} more", start),
                    Style::default().fg(Color::Rgb(80, 80, 90)),
                )));
            }
            for (i, path) in app.file_picker_matches.iter().enumerate().take(end).skip(start) {
                let is_selected = i == selected;
                let style = if is_selected {
                    Style::default().fg(Theme::ACCENT_BRIGHT).add_modifier(Modifier::BOLD).bg(Color::Rgb(40, 40, 50))
                } else {
                    Style::default().fg(Color::Rgb(180, 180, 190))
                };
                let marker = if is_selected { "> " } else { "  " };
                let rel = path.strip_prefix(&app.workspace).unwrap_or(path);
                picker_lines.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Theme::ACCENT)),
                    Span::styled(format!("{}", rel.display()), style),
                ]));
            }
            if end < total {
                picker_lines.push(Line::from(Span::styled(
                    format!("  ↓ {} more", total - end),
                    Style::default().fg(Color::Rgb(80, 80, 90)),
                )));
            }
        }

        let picker_para = Paragraph::new(Text::from(picker_lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(60, 60, 70)))
                    .title(" File Picker (↑↓ Enter Esc) ")
                    .title_style(Style::default().fg(Color::Rgb(100, 100, 110)))
            )
            .style(Style::default().bg(Theme::BG_PANEL));
        f.render_widget(picker_para, chunks[3]);
    }

    // ── Command Menu (shown when input starts with /) ──
    if app.show_command_menu && menu_height > 0 {
        let input_lower = app.input.to_lowercase();
        let commands = app.command_list();
        let matches: Vec<&(String, String, String)> = commands
            .iter()
            .filter(|(full, _, _)| full.to_lowercase().starts_with(&input_lower))
            .collect();

        let mut menu_lines: Vec<Line> = Vec::new();
        
        // Header
        menu_lines.push(Line::from(vec![
            Span::styled(" Commands ", Style::default().fg(Theme::ACCENT_BRIGHT).add_modifier(Modifier::BOLD)),
            Span::styled("(Tab to complete, Esc to close)", Style::default().fg(Color::Rgb(100, 100, 110))),
        ]));
        menu_lines.push(Line::from(Span::styled(
            "─".repeat((chunks[4].width.saturating_sub(4)) as usize),
            Style::default().fg(Color::Rgb(60, 60, 70)),
        )));

        if matches.is_empty() {
            menu_lines.push(Line::from(Span::styled(
                "  No matching commands",
                Style::default().fg(Color::Rgb(100, 100, 110)),
            )));
        } else {
            let selected_idx = app.command_menu_index.min(matches.len() - 1);
            // Scroll window: keep selected item in view (max 15 visible items)
            const VISIBLE: usize = 15;
            let total = matches.len();
            let (start, end) = if total <= VISIBLE {
                (0, total)
            } else if selected_idx < VISIBLE / 2 {
                (0, VISIBLE)
            } else if selected_idx + VISIBLE / 2 >= total {
                (total - VISIBLE, total)
            } else {
                (selected_idx - VISIBLE / 2, selected_idx + VISIBLE / 2 + 1)
            };
            // Scroll indicator
            if start > 0 {
                menu_lines.push(Line::from(Span::styled(
                    format!("  ↑ {} more", start),
                    Style::default().fg(Color::Rgb(80, 80, 90)),
                )));
            }
            let mut skill_sep_added = false;
            for (abs_i, (full, _, desc)) in matches.iter().enumerate().take(end).skip(start) {
                // Insert separator before first skill entry
                if !skill_sep_added && desc.starts_with("[Skill]") {
                    menu_lines.push(Line::from(Span::styled(
                        " ── Skills ──",
                        Style::default().fg(Color::Rgb(100, 100, 110)),
                    )));
                    skill_sep_added = true;
                }
                let is_selected = abs_i == selected_idx;
                let cmd_style = if is_selected {
                    Style::default().fg(Theme::ACCENT_BRIGHT).add_modifier(Modifier::BOLD).bg(Color::Rgb(40, 40, 50))
                } else {
                    Style::default().fg(Theme::ASSISTANT_COLOR)
                };
                let marker = if is_selected { "> " } else { "  " };
                menu_lines.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Theme::ACCENT)),
                    Span::styled(format!("{}", full), cmd_style),
                    Span::styled("  ", Style::default()),
                    Span::styled(desc.as_str(), Style::default().fg(Color::Rgb(120, 120, 130))),
                ]));
            }
            if end < total {
                menu_lines.push(Line::from(Span::styled(
                    format!("  ↓ {} more", total - end),
                    Style::default().fg(Color::Rgb(80, 80, 90)),
                )));
            }
        }

        let menu_para = Paragraph::new(Text::from(menu_lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(60, 60, 70)))
            )
            .style(Style::default().bg(Theme::BG_PANEL));
        f.render_widget(menu_para, chunks[4]);
    }

    // ── Input Line (CodeWhale 1:1 port) ──
    // layout_input_with_scroll is the single source of truth for both rendering
    // and cursor position — they can never drift.
    let input_inner_width = input_inner_width_u16 as usize;
    let input_inner_height = input_rows.saturating_sub(2).max(1); // subtract block borders
    let input_rows_budget = composer_input_rows_budget(input_inner_height);
    let cursor_off = input_prompt.chars().count() + app.input_cursor;
    let (visible_lines, cursor_row, cursor_col, _scroll_offset) =
        layout_input_with_scroll(&full_input, cursor_off, input_inner_width.max(1), input_rows_budget);
    let top_padding = composer_top_padding(visible_lines.len(), input_rows_budget);
    let mut lines = Vec::new();
    for _ in 0..top_padding {
        lines.push(Line::from(""));
    }
    // 渲染 visible_lines，cursor 所在行用反转视频 fake cursor（对齐 pi editor.ts:557）。
    // fake cursor 保证光标隐藏或终端不支持硬件 cursor 时仍可见。
    let cursor_style = Style::default().add_modifier(Modifier::REVERSED);
    for (i, line) in visible_lines.iter().enumerate() {
        if i == cursor_row {
            let graphs: Vec<&str> = line.graphemes(true).collect();
            let mut spans: Vec<Span> = Vec::new();
            let mut col_acc = 0usize;
            let mut cursor_drawn = false;
            for g in &graphs {
                let gw = UnicodeWidthStr::width(*g);
                if !cursor_drawn && col_acc == cursor_col {
                    spans.push(Span::styled(*g, cursor_style));
                    cursor_drawn = true;
                } else {
                    spans.push(Span::raw(*g));
                }
                col_acc += gw;
            }
            // cursor 在行尾或超出：画反转空格
            if !cursor_drawn {
                spans.push(Span::styled(" ", cursor_style));
            }
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(line.as_str()));
        }
    }
    let border_color = match app.thinking_level {
        ThinkingLevel::Default => Color::Rgb(100, 100, 110),  // 灰
        ThinkingLevel::Low => Color::Rgb(59, 130, 246),       // 蓝
        ThinkingLevel::Medium => Color::Rgb(168, 85, 247),    // 紫
        ThinkingLevel::High => Color::Rgb(234, 179, 8),       // 黄
        ThinkingLevel::Max => Color::Rgb(220, 38, 38),        // 红
    };
    let input_widget = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(" Input (Shift+Enter 换行 · \\+Enter fallback · / 命令 · ↑↓ 历史) ")
                .title_style(Style::default().fg(Color::Rgb(100, 100, 110)))
        )
        .style(Theme::style_input());
    f.render_widget(input_widget, chunks[5]);

    // ── Cursor position — uses the same layout_input_with_scroll result ──
    let cursor_x = chunks[5]
        .x
        .saturating_add(2)
        .saturating_add(cursor_col as u16);
    let cursor_y = chunks[5]
        .y
        .saturating_add(1)
        .saturating_add((top_padding + cursor_row) as u16);
    f.set_cursor_position((cursor_x, cursor_y));

    // ── Approval Dialog Overlay ──
    if let Some((ref request, ref choice)) = app.approval_dialog {
        let dialog_area = ApprovalDialog::dialog_area(f.area());
        ApprovalDialog::render(request, choice, dialog_area, f.buffer_mut());
    }

    // ── Settings Dialog Overlay (T3-#15) ──
    if let Some(ref dialog) = app.settings_dialog {
        let dialog_area = SettingsDialog::dialog_area(f.area());
        dialog.render(dialog_area, f.buffer_mut());
    }
}

// ── Helpers ──

/// Extract the last whitespace-delimited token from input (up to cursor).
/// Returns None if the token is empty.
fn extract_path_prefix(input: &str, cursor: usize) -> Option<String> {
    let prefix: String = input.chars().take(cursor).collect();
    let last_token = prefix.rsplit(|c: char| c.is_whitespace()).next()?;
    if last_token.is_empty() { return None; }
    Some(last_token.to_string())
}

/// Try to complete a path prefix by scanning the filesystem.
/// Returns a list of matching full paths (relative or absolute).
fn complete_path(prefix: &str) -> Option<Vec<String>> {
    let path = std::path::Path::new(prefix);
    let (dir, name_prefix) = if path.is_dir() {
        (path.to_path_buf(), String::new())
    } else {
        let parent = path.parent()?.to_path_buf();
        let fname = path.file_name()?.to_string_lossy().to_string();
        (parent, fname)
    };
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut matches: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&name_prefix) {
                let full = dir.join(&name).to_string_lossy().to_string();
                // Normalize backslashes to forward slashes for consistency
                Some(full.replace('\\', "/"))
            } else {
                None
            }
        })
        .collect();
    if matches.is_empty() {
        None
    } else {
        matches.sort();
        Some(matches)
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// P2: 从队列中取出下一条要交付的消息（steering 优先于 follow-up）。
/// 纯逻辑，便于在不构造 App 的情况下单元测试。
fn dequeue_next_from(queue: &mut Vec<QueuedMessage>) -> Option<QueuedMessage> {
    let idx = queue
        .iter()
        .position(|m| m.kind == QKind::Steering)
        .or_else(|| queue.iter().position(|m| m.kind == QKind::FollowUp))?;
    Some(queue.remove(idx))
}

fn summarize(text: &str, max: usize) -> String {
    let first_line = text.lines().next().unwrap_or("");
    let total_lines = text.lines().count();
    if total_lines <= 1 {
        clip(first_line, max)
    } else {
        format!("{} ({} lines)", clip(first_line, max.saturating_sub(10)), total_lines)
    }
}

/// Hard-wrap `text` by display width so the rendered lines exactly match the
/// cursor position calculation.  Uses grapheme clusters (与 `cursor_row_col` 一致)
/// so multi-byte / emoji / 组合字符的行列计算与光标定位不会漂移。
fn wrap_input_lines(input: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![String::new()];
    let mut line_width = 0usize;
    for grapheme in input.graphemes(true) {
        if grapheme == "\n" {
            lines.push(String::new());
            line_width = 0;
            continue;
        }
        let w = UnicodeWidthStr::width(grapheme);
        if line_width + w > width && !lines.last().unwrap().is_empty() {
            lines.push(String::new());
            line_width = 0;
        }
        lines.last_mut().unwrap().push_str(grapheme);
        line_width += w;
    }
    if lines.len() == 1 && lines[0].is_empty() {
        return lines;
    }
    while lines.len() > 1 && lines.last().unwrap().is_empty() {
        lines.pop();
    }
    lines
}

/// Compute the visual line/column of the cursor (in character units) inside a
/// text that was hard-wrapped at `max_width` display columns.
/// Uses grapheme clusters so multi-byte / emoji input stays aligned.
fn cursor_row_col(input: &str, cursor_chars: usize, max_width: usize) -> (usize, usize) {
    let mut row = 0usize;
    let mut col = 0usize;
    let mut char_idx = 0usize;

    for grapheme in input.graphemes(true) {
        if char_idx >= cursor_chars {
            break;
        }
        let grapheme_chars = grapheme.chars().count();
        let next_char_idx = char_idx.saturating_add(grapheme_chars);
        let cursor_inside = cursor_chars < next_char_idx;

        if grapheme == "\n" {
            row += 1;
            col = 0;
            char_idx = next_char_idx;
            if cursor_inside {
                break;
            }
            continue;
        }

        let grapheme_width = grapheme.width();
        if col + grapheme_width > max_width && col != 0 {
            row += 1;
            col = 0;
        }
        col += grapheme_width;
        if cursor_inside {
            break;
        }
        char_idx = next_char_idx;
    }

    (row, col)
}

// ── CodeWhale 1:1 port: layout helpers ──────────────────────────────────────

fn layout_input(
    input: &str,
    cursor: usize,
    width: usize,
    max_height: usize,
) -> (Vec<String>, usize, usize) {
    let (visible, visible_cursor_row, visible_cursor_col, _) =
        layout_input_with_scroll(input, cursor, width, max_height);
    (visible, visible_cursor_row, visible_cursor_col)
}

fn layout_input_with_scroll(
    input: &str,
    cursor: usize,
    width: usize,
    max_height: usize,
) -> (Vec<String>, usize, usize, usize) {
    let mut lines = wrap_input_lines(input, width);
    if lines.is_empty() {
        lines.push(String::new());
    }
    let (cursor_row, cursor_col) = cursor_row_col(input, cursor, width.max(1));

    let max_height = max_height.max(1);
    let mut start = 0usize;
    if cursor_row >= max_height {
        start = cursor_row + 1 - max_height;
    }
    if start + max_height > lines.len() {
        start = lines.len().saturating_sub(max_height);
    }
    let visible = lines
        .into_iter()
        .skip(start)
        .take(max_height)
        .collect::<Vec<_>>();
    let visible_cursor_row = cursor_row.saturating_sub(start);

    (
        visible,
        visible_cursor_row,
        cursor_col.min(width.saturating_sub(1)),
        start,
    )
}

fn composer_input_rows_budget(inner_height: u16) -> usize {
    usize::from(inner_height).max(1)
}

fn composer_top_padding(content_lines: usize, rows_budget: usize) -> usize {
    rows_budget.saturating_sub(content_lines.clamp(1, rows_budget))
}

fn composer_height(
    input: &str,
    width: u16,
    available_height: u16,
    min_rows: usize,
    max_rows: usize,
) -> u16 {
    let content_width = usize::from(width.max(1));
    let mut line_count = wrap_input_lines(input, content_width).len();
    if line_count == 0 {
        line_count = 1;
    }
    line_count = line_count.max(min_rows);
    let max_height = usize::from(available_height.clamp(1, max_rows as u16));
    line_count.clamp(1, max_height).try_into().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_input_lines_empty() {
        assert_eq!(wrap_input_lines("", 10), vec![""]);
    }

    #[test]
    fn wrap_input_lines_single_line_no_wrap() {
        assert_eq!(wrap_input_lines("hello", 10), vec!["hello"]);
    }

    #[test]
    fn wrap_input_lines_exact_width() {
        // 5 chars, width 5 -> should fit on one line
        assert_eq!(wrap_input_lines("hello", 5), vec!["hello"]);
    }

    #[test]
    fn wrap_input_lines_exceed_width() {
        // 6 chars, width 5 -> should wrap to 2 lines
        assert_eq!(wrap_input_lines("abcdef", 5), vec!["abcde", "f"]);
    }

    #[test]
    fn wrap_input_lines_multiple_wraps() {
        let result = wrap_input_lines("abcdefghij", 3);
        assert_eq!(result, vec!["abc", "def", "ghi", "j"]);
    }

    #[test]
    fn wrap_input_lines_newlines_preserved() {
        let result = wrap_input_lines("ab\ncd\n", 10);
        assert_eq!(result, vec!["ab", "cd"]);
    }

    #[test]
    fn wrap_input_lines_unicode_width() {
        // '你' has width 2
        let result = wrap_input_lines("你好世界", 4);
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn wrap_input_lines_trailing_newline() {
        let result = wrap_input_lines("abc\n", 10);
        assert_eq!(result, vec!["abc"]);
    }

    #[test]
    fn cursor_row_col_basic() {
        let (row, col) = cursor_row_col("abcdef", 2, 10);
        assert_eq!(row, 0);
        assert_eq!(col, 2);
    }

    #[test]
    fn cursor_row_col_after_newline() {
        let (row, col) = cursor_row_col("ab\ncd", 3, 10);
        assert_eq!(row, 1);
        assert_eq!(col, 0);
    }

    #[test]
    fn cursor_row_col_wrap_boundary() {
        // 5 chars, width 5, cursor at end
        let (row, col) = cursor_row_col("abcde", 5, 5);
        assert_eq!(row, 0);
        assert_eq!(col, 5);
    }

    #[test]
    fn cursor_row_col_wrap_overflow() {
        // 6 chars, width 5 -> cursor at 5 is last char of line 0, cursor at 6 is first char of line 1
        let (row0, col0) = cursor_row_col("abcdef", 5, 5);
        assert_eq!(row0, 0);
        assert_eq!(col0, 5);

        let (row1, col1) = cursor_row_col("abcdef", 6, 5);
        assert_eq!(row1, 1);
        assert_eq!(col1, 1);
    }

    #[test]
    fn cursor_row_col_consistency_with_wrap() {
        // This is the key invariant: cursor position should align with wrapped lines
        let text = "abcdefghij";
        let width = 3;
        let wrapped = wrap_input_lines(text, width);
        assert_eq!(wrapped, vec!["abc", "def", "ghi", "j"]);

        // Cursor at end of each wrapped line
        for (i, line) in wrapped.iter().enumerate() {
            let cursor_pos = wrapped.iter().take(i + 1).map(|s| s.len()).sum::<usize>();
            let (row, col) = cursor_row_col(text, cursor_pos, width);
            assert_eq!(row, i, "cursor at char {} should be at row {}", cursor_pos, i);
            assert_eq!(col, line.len(), "cursor at char {} should be at col {}", cursor_pos, line.len());
        }
    }

    #[test]
    fn composer_height_empty_input() {
        assert_eq!(composer_height("", 10, 10, 3, 10), 3);
    }

    #[test]
    fn composer_height_single_line() {
        assert_eq!(composer_height("hello", 10, 10, 3, 10), 3);
    }

    #[test]
    fn composer_height_wraps_to_multiple() {
        // 6 chars at width 5 -> 2 lines
        assert_eq!(composer_height("abcdef", 5, 10, 3, 10), 3);
    }

    #[test]
    fn composer_height_min_clamp() {
        assert_eq!(composer_height("", 10, 10, 5, 10), 5);
    }

    #[test]
    fn composer_height_max_clamp() {
        // 20 chars at width 5 -> 4 lines, but max is 3
        assert_eq!(composer_height("abcdefghijklmnopqrst", 5, 10, 1, 3), 3);
    }

    #[test]
    fn composer_input_rows_budget() {
        assert_eq!(super::composer_input_rows_budget(5), 5);
        assert_eq!(super::composer_input_rows_budget(1), 1);
        assert_eq!(super::composer_input_rows_budget(0), 1);
    }

    #[test]
    fn composer_top_padding() {
        // budget 5, content 3 lines -> padding 2
        assert_eq!(super::composer_top_padding(3, 5), 2);
        // budget 3, content 5 lines -> padding 0 (clamped)
        assert_eq!(super::composer_top_padding(5, 3), 0);
        // budget 3, content 3 lines -> padding 0
        assert_eq!(super::composer_top_padding(3, 3), 0);
    }

    // ── TUI-2: multi-line cursor movement tests ──
    // These test the pure logic via helper functions that don't require
    // a full App instance.

    #[test]
    fn cursor_line_up_logic() {
        // "line1\nline2\nline3", cursor=12 (end of line3)
        // Up → col 5 in line2 → position 11
        let input = "line1\nline2\nline3";
        let chars: Vec<char> = input.chars().collect();
        let cursor = 12;
        let line_start = chars[..cursor].iter().rposition(|&c| c == '\n').map(|p| p + 1).unwrap_or(0);
        assert_eq!(line_start, 12); // line3 starts at index 12
        let prev_line_end = line_start - 1; // skip \n
        let prev_line_start = chars[..prev_line_end].iter().rposition(|&c| c == '\n').map(|p| p + 1).unwrap_or(0);
        assert_eq!(prev_line_start, 6); // line2 starts at index 6
        let prev_line_len = prev_line_end - prev_line_start;
        assert_eq!(prev_line_len, 5); // "line2" is 5 chars
    }

    #[test]
    fn cursor_line_down_logic() {
        // "line1\nline2\nline3", cursor=3 (in line1)
        // Down → col 3 in line2 → position 9
        let input = "line1\nline2\nline3";
        let chars: Vec<char> = input.chars().collect();
        let cursor = 3;
        let next_nl = chars[cursor..].iter().position(|&c| c == '\n').map(|p| cursor + p);
        assert_eq!(next_nl, Some(5)); // \n at index 5
        let next_line_start = next_nl.unwrap() + 1;
        assert_eq!(next_line_start, 6); // line2 starts at 6
        let col = cursor; // col in line1
        // next_line has 5 chars, col 3 fits
        let new_cursor = next_line_start + col;
        assert_eq!(new_cursor, 9);
    }

    // ── TUI-1: scroll info computation tests ──

    #[test]
    fn scroll_info_at_bottom() {
        // scroll_from_bottom=0 → ""
        let scroll_from_bottom: usize = 0;
        let total_lines: usize = 100;
        let visible_height: usize = 20;
        let max_scroll = total_lines.saturating_sub(visible_height);
        let info = if total_lines <= visible_height {
            String::new()
        } else if scroll_from_bottom == 0 {
            String::new()
        } else if scroll_from_bottom >= max_scroll {
            "TOP".to_string()
        } else {
            let start = max_scroll.saturating_sub(scroll_from_bottom);
            format!("▲ {}/{}", start + 1, total_lines)
        };
        assert_eq!(info, "");
    }

    #[test]
    fn scroll_info_at_top() {
        let scroll_from_bottom: usize = 80;
        let total_lines: usize = 100;
        let visible_height: usize = 20;
        let max_scroll = total_lines.saturating_sub(visible_height);
        let info = if scroll_from_bottom >= max_scroll {
            "TOP".to_string()
        } else {
            String::new()
        };
        assert_eq!(info, "TOP");
    }

    #[test]
    fn scroll_info_in_middle() {
        let scroll_from_bottom: usize = 40;
        let total_lines: usize = 100;
        let visible_height: usize = 20;
        let max_scroll = total_lines.saturating_sub(visible_height);
        let start = max_scroll.saturating_sub(scroll_from_bottom);
        let info = format!("▲ {}/{}", start + 1, total_lines);
        assert_eq!(info, "▲ 41/100");
    }

    // ── TUI-3: FocusTarget enum tests ──

    #[test]
    fn focus_target_toggle() {
        let mut focus = FocusTarget::Input;
        focus = match focus {
            FocusTarget::Input => FocusTarget::Chat,
            FocusTarget::Chat => FocusTarget::Input,
        };
        assert_eq!(focus, FocusTarget::Chat);
        focus = match focus {
            FocusTarget::Input => FocusTarget::Chat,
            FocusTarget::Chat => FocusTarget::Input,
        };
        assert_eq!(focus, FocusTarget::Input);
    }

    // ── P2: message queue dequeue logic tests ──

    fn qmsg(text: &str, kind: QKind) -> QueuedMessage {
        QueuedMessage { text: text.to_string(), kind }
    }

    #[test]
    fn dequeue_empty_returns_none() {
        let mut queue: Vec<QueuedMessage> = Vec::new();
        assert!(dequeue_next_from(&mut queue).is_none());
    }

    #[test]
    fn dequeue_single_steering() {
        let mut queue = vec![qmsg("hello", QKind::Steering)];
        let msg = dequeue_next_from(&mut queue).expect("non-empty");
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.kind, QKind::Steering);
        assert!(queue.is_empty());
    }

    #[test]
    fn dequeue_single_followup() {
        let mut queue = vec![qmsg("world", QKind::FollowUp)];
        let msg = dequeue_next_from(&mut queue).expect("non-empty");
        assert_eq!(msg.text, "world");
        assert_eq!(msg.kind, QKind::FollowUp);
        assert!(queue.is_empty());
    }

    #[test]
    fn dequeue_steering_before_followup() {
        // follow-up 排在前面，但 steering 应优先取出
        let mut queue = vec![
            qmsg("first-followup", QKind::FollowUp),
            qmsg("second-steering", QKind::Steering),
        ];
        let msg = dequeue_next_from(&mut queue).expect("non-empty");
        assert_eq!(msg.text, "second-steering");
        assert_eq!(msg.kind, QKind::Steering);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn dequeue_preserves_steering_order() {
        // 多条 steering 按插入顺序取
        let mut queue = vec![
            qmsg("s1", QKind::Steering),
            qmsg("s2", QKind::Steering),
            qmsg("f1", QKind::FollowUp),
        ];
        let m1 = dequeue_next_from(&mut queue).unwrap();
        assert_eq!(m1.text, "s1");
        let m2 = dequeue_next_from(&mut queue).unwrap();
        assert_eq!(m2.text, "s2");
        let m3 = dequeue_next_from(&mut queue).unwrap();
        assert_eq!(m3.text, "f1");
        assert!(queue.is_empty());
    }

    #[test]
    fn dequeue_followup_only_in_order() {
        let mut queue = vec![
            qmsg("f1", QKind::FollowUp),
            qmsg("f2", QKind::FollowUp),
        ];
        assert_eq!(dequeue_next_from(&mut queue).unwrap().text, "f1");
        assert_eq!(dequeue_next_from(&mut queue).unwrap().text, "f2");
        assert!(dequeue_next_from(&mut queue).is_none());
    }

    #[test]
    fn dequeue_steering_interleaved_with_followup() {
        // steering 插队到 follow-up 之前
        let mut queue = vec![
            qmsg("f1", QKind::FollowUp),
            qmsg("f2", QKind::FollowUp),
            qmsg("s1", QKind::Steering),
            qmsg("f3", QKind::FollowUp),
        ];
        assert_eq!(dequeue_next_from(&mut queue).unwrap().text, "s1");
        assert_eq!(dequeue_next_from(&mut queue).unwrap().text, "f1");
        assert_eq!(dequeue_next_from(&mut queue).unwrap().text, "f2");
        assert_eq!(dequeue_next_from(&mut queue).unwrap().text, "f3");
        assert!(queue.is_empty());
    }

    // ── P0.1/P0.2/P0.3 v2: 多行输入渲染 + cursor 定位测试 ──
    // App 方法（navigate_history / push_undo / push_kill_ring）依赖复杂构造，
    // 此处通过自由函数验证多行渲染与 cursor 计算的正确性，App 方法行为由手动 TUI 测试覆盖。

    #[test]
    fn wrap_input_lines_cjk_mixed() {
        // CJK (width 2) + ASCII (width 1) 混合换行
        let result = wrap_input_lines("你好abc", 4);
        // '你'(2) + '好'(2) = 4 → 第一行满；'a'(1)+'b'(1)+'c'(1) = 3 → 第二行
        assert_eq!(result, vec!["你好", "abc"]);
    }

    #[test]
    fn wrap_input_lines_multiline_with_cjk() {
        // 显式 \n + CJK 换行
        let result = wrap_input_lines("你好\n世界\n", 10);
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn cursor_row_col_multiline_second_line() {
        // "ab\ncd" cursor 在 'c' (char index 3) → row 1, col 0
        let (row, col) = cursor_row_col("ab\ncd", 3, 10);
        assert_eq!(row, 1);
        assert_eq!(col, 0);
    }

    #[test]
    fn cursor_row_col_multiline_end_of_first_line() {
        // "ab\ncd" cursor 在 '\n' 之后 (char index 3) → row 1, col 0
        // cursor 在 'b' (char index 1) → row 0, col 1
        let (row, col) = cursor_row_col("ab\ncd", 1, 10);
        assert_eq!(row, 0);
        assert_eq!(col, 1);
    }

    #[test]
    fn cursor_row_col_cjk_width_accounted() {
        // "你好" cursor 在 '好' (char index 1) → col 应为 2（'你' 占 2 列）
        let (row, col) = cursor_row_col("你好", 1, 10);
        assert_eq!(row, 0);
        assert_eq!(col, 2);
    }

    #[test]
    fn layout_input_with_scroll_multiline_cursor_visible() {
        // 多行 input，cursor 在第 2 行，max_height 足够大 → cursor 行可见
        let input = "line1\nline2\nline3";
        let (visible, cursor_row, _cursor_col, start) =
            layout_input_with_scroll(input, 7, 80, 10); // cursor 在 'l' of line2
        assert_eq!(start, 0);
        assert_eq!(visible.len(), 3);
        assert_eq!(cursor_row, 1);
    }

    #[test]
    fn layout_input_with_scroll_scrolls_to_cursor() {
        // 5 行 input，max_height=2，cursor 在最后一行 → start 滚动到 cursor 可见
        let input = "l1\nl2\nl3\nl4\nl5";
        // char indices: 0='l'1='1'2='\n'3='l'4='2'5='\n'6='l'7='3'8='\n'9='l'10='4'11='\n'12='l'13='5'
        // cursor=12 → 'l' of l5 → row=4, col=0
        let (visible, cursor_row, _cursor_col, start) =
            layout_input_with_scroll(input, 12, 80, 2);
        assert_eq!(start, 3); // 从第 3 行开始显示（l4, l5）
        assert_eq!(visible, vec!["l4", "l5"]);
        assert_eq!(cursor_row, 1); // cursor 在可见区的第 1 行
    }

    #[test]
    fn composer_top_padding_empty_input() {
        // 空内容（1 行）+ budget 3 → padding 2（顶对齐填充）
        assert_eq!(super::composer_top_padding(1, 3), 2);
    }

    #[test]
    fn wrap_input_lines_emoji_zwj() {
        // emoji + CJK 混合，grapheme 分割正确（不崩 panic）
        let result = wrap_input_lines("a😀b", 2);
        // 'a'(1) → 第一行；'😀'(2) → 第二行；'b'(1) → 第三行
        assert_eq!(result, vec!["a", "😀", "b"]);
    }
}
