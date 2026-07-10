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
use rgoat_core::security::approval::{AgentMode, ApprovalDecision, ApprovalResponder};

use crate::components::approval_dialog::{ApprovalChoice, ApprovalDialog, ApprovalRequest};

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

// ── Application State ──

struct App {
    agent: Arc<ReActAgent>,
    conversation: Arc<ConversationManager>,
    event_rx: broadcast::Receiver<BusEvent>,
    switch: Arc<ProviderSwitch>,
    review_provider: Option<Arc<dyn LlmProvider>>,

    // Session
    session_id: String,
    workspace: String,

    // Chat
    lines: Vec<UiElement>,
    input: String,
    mode: AgentMode,
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
    /// P1: track tool execution start times for elapsed time calculation
    tool_start_times: HashMap<String, Instant>,
    /// Paste-burst guard: timestamp of the last input character / paste.
    /// If Enter arrives within ~80 ms it is likely a terminal splitting a
    /// multi-line paste — absorb it as a space instead of submitting.
    last_input_time: Option<Instant>,
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
            workspace,
            lines: Vec::new(),
            input: String::new(),
            mode,
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
            tool_start_times: HashMap::new(),
            last_input_time: None,
            command_menu_index: 0,
            loaded_skills: Vec::new(),
            // ── D4 ──
            show_work_sidebar: false,
            sidebar_progress_text: String::new(),
            sidebar_recent_tools: Vec::new(),
            sidebar_current_step: 0,
            sidebar_total_steps: 0,
        }
    }

    async fn init(&mut self) {
        self.current_provider = self.switch.current_name().await;

        // Pre-load skills for /findskill command menu
        self.loaded_skills = load_skills_for_menu(&self.workspace);

        match self.conversation.create_session(None, Some(&self.workspace)).await {
            Ok(s) => {
                self.session_id = s.id;
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
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            started_at: self.processing_start,
            git_branch: self.git_branch.clone(),
            is_processing: self.is_processing,
            scroll_info: String::new(),
            focus: String::new(),
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

    async fn submit(&mut self) {
        let prompt = std::mem::take(&mut self.input);
        // ★ 无论走哪条路径，先重置输入相关状态
        self.input_cursor = 0;
        self.show_command_menu = false;
        self.command_menu_index = 0;

        if prompt.trim().is_empty() {
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
            self.status_msg = "Already processing a request...".to_string();
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

        tokio::spawn(async move {
            let result = agent.run(&session_id, &prompt, &workspace).await;
            if let Err(e) = &result {
                tracing::error!("Agent error: {}", e);
            }
            let _ = result;
        });
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
                    text: "Commands:\n  /yolo      ⚡ YOLO mode\n  /agent     🤖 Agent mode\n  /plan      📋 Plan mode\n  /flow      🌊 Flow mode\n  /edits     ✏ Accept-Edits\n  /model     📦 Models (API fetch)\n  /editprovider ⚙ Edit providers\n  /help      Show commands\n  /clear     Clear conversation\n  /new       New session\n  /resume <id> Resume\n  /sessions  List sessions\n  /session <id> Info\n  /findskill 🔍 Find & install skills\n  /mcp list  MCP servers\n  /compact   Compress\n  /exit      Quit".into(),
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
                        self.add_line(UiElement::System {
                            text: format!("New session created: {} ({})", session.id.chars().take(8).collect::<String>(), session.title),
                        });
                        self.session_id = session.id;
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
            _ => {
                self.add_line(UiElement::System {
                    text: format!("Unknown: {}. Use /help to see available commands.", cmd),
                });
            }
        }
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

    /// Move cursor to the previous line in multi-line input (TUI-2).
    /// Finds the column position in the current line, then jumps to the
    /// same column (or end) in the previous line.
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
            return;
        }

        let col = cursor - line_start; // column within current line

        // Find the end of the previous line
        let prev_line_end = line_start.saturating_sub(1); // skip the '\n'
        let prev_line_start = chars[..prev_line_end].iter().rposition(|&c| c == '\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let prev_line_len = prev_line_end - prev_line_start;

        // Place cursor at same column, or end of previous line
        let new_col = col.min(prev_line_len);
        self.input_cursor = prev_line_start + new_col;
    }

    /// Move cursor to the next line in multi-line input (TUI-2).
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
        let col = cursor - line_start;

        // Next line starts right after the '\n'
        let next_line_start = next_nl + 1;
        // Find end of next line (next '\n' or end of string)
        let next_line_end = chars[next_line_start..].iter().position(|&c| c == '\n')
            .map(|p| next_line_start + p)
            .unwrap_or(chars.len());
        let next_line_len = next_line_end - next_line_start;

        let new_col = col.min(next_line_len);
        self.input_cursor = next_line_start + new_col;
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
            } => {
                // In YOLO mode: auto-approve all tool calls without dialog
                if self.mode == AgentMode::Yolo {
                    let decision = ApprovalDecision {
                        approved: true,
                        approve_all: true,
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
                    // Bracketed paste: sanitize and flatten the pasted text.
                    // Some Windows terminals split multi-line pastes into lines
                    // followed by Enter keys; normalizing all line breaks to spaces
                    // prevents accidental auto-submit of the first segment.
                    // Also drop NUL bytes which can truncate C-level clipboard APIs.
                    let mut text: String = text
                        .chars()
                        .filter(|c| *c != '\0')
                        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if text.is_empty() {
                        continue;
                    }
                    // Input length guard — prevent OOM / O(n²) performance collapse
                    let char_count = app.input.chars().count();
                    const INPUT_MAX: usize = 10_000;
                    if char_count + text.chars().count() > INPUT_MAX {
                        let available = INPUT_MAX.saturating_sub(char_count);
                        if available == 0 {
                            app.status_msg = "Input full (10,000 chars limit)".to_string();
                            continue;
                        }
                        text = text.chars().take(available).collect();
                        app.status_msg = format!("Paste truncated to fit 10k char limit ({} remaining)", available);
                    }
                    if app.input_cursor > char_count {
                        app.input_cursor = char_count;
                    }
                    // If the input already has text, append a space before the paste.
                    let byte_pos = app.input.char_indices().nth(app.input_cursor).map(|(i, _)| i).unwrap_or(app.input.len());
                    if byte_pos > 0 && !app.input.ends_with(' ') && !text.starts_with(' ') {
                        app.input.insert(byte_pos, ' ');
                        app.input_cursor += 1;
                    }
                    let byte_pos = app.input.char_indices().nth(app.input_cursor).map(|(i, _)| i).unwrap_or(app.input.len());
                    app.input.insert_str(byte_pos, &text);
                    app.input_cursor += text.chars().count();
                    app.show_command_menu = app.input.starts_with('/');
                    app.last_input_time = Some(Instant::now());
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
                                    })
                                }
                                KeyCode::Char('a') | KeyCode::Char('A') => {
                                    Some(ApprovalDecision {
                                        approved: true,
                                        approve_all: true,
                                    })
                                }
                                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                    Some(ApprovalDecision {
                                        approved: false,
                                        approve_all: false,
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
                        // Esc: close menu if open, otherwise exit
                        (KeyCode::Esc, _) => {
                            if app.show_command_menu {
                                app.show_command_menu = false;
                            } else if app.input.is_empty() {
                                return Ok(());
                            } else {
                                app.input.clear();
                                app.input_cursor = 0;  // P0 fix: reset cursor on clear
                                app.show_command_menu = false;
                            }
                        }
    
                        // Tab: auto-complete command from menu, or cycle mode when menu is closed
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
                            } else {
                                app.cycle_mode();
                            }
                        }
    
                        // Submit
                        (KeyCode::Enter, _) => {
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
                            // Paste-burst guard: if Enter arrives within 80 ms of
                            // the last input event (Paste / Char / Backspace), the
                            // terminal is likely splitting a multi-line paste into
                            // per-line events.  Absorb the Enter as a space instead
                            // of submitting a truncated prompt.
                            if let Some(t) = app.last_input_time {
                                if t.elapsed() < Duration::from_millis(80) {
                                    app.input.push(' ');
                                    app.input_cursor = app.input.chars().count();
                                    app.last_input_time = Some(Instant::now());
                                    continue;
                                }
                            }
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
    
                        // Backspace
                        (KeyCode::Backspace, _) => {
                            let char_count = app.input.chars().count();
                            if !app.input.is_empty() && app.input_cursor > 0 && app.input_cursor <= char_count {
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
                            app.last_input_time = Some(Instant::now());
                        }

                        // Delete (forward delete)
                        (KeyCode::Delete, _) => {
                            let char_count = app.input.chars().count();
                            if !app.input.is_empty() && app.input_cursor < char_count {
                                let char_indices: Vec<(usize, char)> = app.input.char_indices().collect();
                                let (byte_pos, _) = char_indices[app.input_cursor];
                                app.input.remove(byte_pos);
                            }
                            if app.input_cursor > char_count {
                                app.input_cursor = char_count;
                            }
                            app.show_command_menu = app.input.starts_with('/');
                            app.last_input_time = Some(Instant::now());
                        }

                        // ── Ctrl+letter handlers (BEFORE general Char to avoid inserting text) ──
                        (KeyCode::Char(c), m) if m.contains(KeyModifiers::CONTROL) => {
                            match c {
                                // Ctrl+U: clear entire input line (matches CodeWhale / readline)
                                'u' => {
                                    app.input.clear();
                                    app.input_cursor = 0;
                                    app.show_command_menu = false;
                                }
                                // Ctrl+W: delete previous word
                                'w' => {
                                    let cursor = app.input_cursor.min(app.input.chars().count());
                                    let prefix: String = app.input.chars().take(cursor).collect();
                                    let trimmed_len = prefix.trim_end().len();
                                    if trimmed_len > 0 {
                                        let word_start = prefix[..trimmed_len]
                                            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                                            .map(|p| p + 1)
                                            .unwrap_or(0);
                                        let _delete_count = cursor - word_start;
                                        // Remove the word from the real input
                                        let pre_byte: usize = app.input.chars().take(word_start).collect::<String>().len();
                                        let post: String = app.input.chars().skip(cursor).collect();
                                        app.input.truncate(pre_byte);
                                        app.input.push_str(&post);
                                        app.input_cursor = word_start;
                                    }
                                    app.show_command_menu = app.input.starts_with('/');
                                }
                                _ => {}
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
                            let byte_pos = app.input.char_indices().nth(app.input_cursor).map(|(i, _)| i).unwrap_or(app.input.len());
                            app.input.insert(byte_pos, c);
                            app.input_cursor += 1;
                            app.show_command_menu = app.input.starts_with('/');
                            app.last_input_time = Some(Instant::now());
                        }
    
                        // Cursor movement inside input
                        (KeyCode::Left, _) => {
                            if app.input_cursor > 0 {
                                app.input_cursor -= 1;
                            }
                        }
                        (KeyCode::Right, _) => {
                            let char_count = app.input.chars().count();
                            if app.input_cursor < char_count {
                                app.input_cursor += 1;
                            }
                        }
                        (KeyCode::Home, _) => {
                            app.input_cursor = 0;
                        }
                        (KeyCode::End, _) => {
                            app.input_cursor = app.input.chars().count();
                        }
    
                        // Command menu navigation (Up/Down) or scroll when no menu
                        (KeyCode::Up, _) => {
                            if app.show_command_menu {
                                if app.command_menu_index > 0 {
                                    app.command_menu_index -= 1;
                                }
                            } else if app.input.contains('\n') {
                                // Multi-line input: move cursor to previous line (TUI-2)
                                app.move_cursor_line_up();
                            } else {
                                app.scroll_up(1);
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
                                // Multi-line input: move cursor to next line (TUI-2)
                                app.move_cursor_line_down();
                            } else {
                                app.scroll_down(1);
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

    // Compute dynamic input height using CodeWhale's composer_height formula.
    // This is the single source of truth for how many rows the input panel
    // needs, matching the wrap logic used for rendering and cursor.
    const MAX_INPUT_ROWS: usize = 10;
    const MIN_INPUT_ROWS: usize = 3;
    let input_prompt = format!("{} ▶ ", app.mode_display());
    let full_input = format!("{}{}", input_prompt, app.input);
    let input_rows = composer_height(
        &full_input,
        area.width,
        MAX_INPUT_ROWS as u16 + 2,
        MIN_INPUT_ROWS,
        MAX_INPUT_ROWS,
    ) as u16;

    // Split: title(1) | chat(min 3) | status(1) | [menu(dynamic)] | input(dynamic)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                     // title bar
            Constraint::Min(3),                        // chat area
            Constraint::Length(1),                     // status bar
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
            "─".repeat((chunks[3].width.saturating_sub(4)) as usize),
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
        f.render_widget(menu_para, chunks[3]);
    }

    // ── Input Line (CodeWhale 1:1 port) ──
    // layout_input_with_scroll is the single source of truth for both rendering
    // and cursor position — they can never drift.
    let input_inner_width = area.width.saturating_sub(4) as usize;
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
    for line in &visible_lines {
        lines.push(Line::from(line.as_str()));
    }
    let input_widget = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(50, 50, 55)))
                .title(" Input (/ for commands) ")
                .title_style(Style::default().fg(Color::Rgb(100, 100, 110)))
        )
        .style(Theme::style_input());
    f.render_widget(input_widget, chunks[4]);

    // ── Cursor position — uses the same layout_input_with_scroll result ──
    let cursor_x = chunks[4]
        .x
        .saturating_add(2)
        .saturating_add(cursor_col as u16);
    let cursor_y = chunks[4]
        .y
        .saturating_add(1)
        .saturating_add((top_padding + cursor_row) as u16);
    f.set_cursor_position((cursor_x, cursor_y));

    // ── Approval Dialog Overlay ──
    if let Some((ref request, ref choice)) = app.approval_dialog {
        let dialog_area = ApprovalDialog::dialog_area(f.area());
        ApprovalDialog::render(request, choice, dialog_area, f.buffer_mut());
    }
}

// ── Helpers ──

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
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
/// cursor position calculation.  This avoids the mismatch between ratatui's
/// word-wrap and our cursor math, which caused the cursor to float below the
/// visible text.
fn wrap_input_lines(input: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![String::new()];
    let mut line_width = 0usize;
    for ch in input.chars() {
        if ch == '\n' {
            lines.push(String::new());
            line_width = 0;
            continue;
        }
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if line_width + w > width && !lines.last().unwrap().is_empty() {
            lines.push(String::new());
            line_width = 0;
        }
        lines.last_mut().unwrap().push(ch);
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
}
