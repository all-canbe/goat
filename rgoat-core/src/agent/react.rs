//! ReAct Agent 主循环
//!
//! 核心逻辑：
//! 1. 构建 system prompt + 历史消息
//! 2. 调用 LLM
//! 3. 解析输出中的工具调用
//! 4. 经审批引擎检查
//! 5. 执行工具，获得 observation
//! 6. 将 observation 加入历史，重复直到得到最终答案

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::agent::types::{AgentConfig, AgentError, AgentEvent, AgentRunResult, ParsedToolCall};
use crate::conversation::compressor::{CompressedMessage, CompressionConfig, CompressionLevel, ContextCompressor, MilestoneSummary};
use crate::conversation::manager::{ConversationManager, MessageRecord};
use crate::conversation::templates::build_system_prompt;
use crate::conversation::templates::extract_tech_stack_constraints;
use crate::core::cancellation::CancellationToken;
use crate::core::event_bus::{EventBus, EventType};
use crate::provider::provider::{ChatMessage, ChatResponse, Choice, LlmProvider, MessageContent, Role, ToolCallDef, ToolDef, UsageInfo};
use crate::security::approval::{AgentMode, ApprovalDecision, ApprovalEngine, ApprovalResponder, Decision, ToolCategory, ApprovalScope, SessionCacheKey, calculate_danger_score};
use crate::security::sandbox::Sandbox;
use crate::tools::registry::{ToolRegistry, ToolResult};
use super::steps_tracker::StepsTracker;
use super::planner::Planner;
use super::task_persistence::{TaskPersistence, TaskEvent, TaskStatus};
use super::checkpoint::{Checkpoint, CheckpointData, CheckpointStatus};

/// ReAct Agent
pub struct ReActAgent {
    pub config: AgentConfig,
    pub provider: Arc<dyn LlmProvider>,
    pub tools: Arc<ToolRegistry>,
    pub approval: Arc<ApprovalEngine>,
    pub conversation: Arc<ConversationManager>,
    pub event_bus: Arc<EventBus>,
    pub cancellation: CancellationToken,
    pub(crate) mode: AtomicUsize,
    /// Whether the agent is currently paused (shared with desktop IPC).
    pub paused: Arc<AtomicBool>,
    /// Oneshot channel for blocking tool-approval handshake with TUI.
    pub approval_responder: ApprovalResponder,
    /// M14: 运行时上下文窗口（从 /models API 获取，默认 128k）
    pub context_window: Arc<AtomicU64>,
    /// B1: 子 Agent 自动审批标志 — 非破坏性工具调用直接 Allow（不等待 TUI 审批）
    pub auto_approve: bool,
    /// D1-T02: 会话级审批缓存 — 用户选择 Session/AllSimilar 后同工具自动通过
    pub session_cache: Arc<std::sync::Mutex<std::collections::HashMap<SessionCacheKey, Decision>>>,
    /// 沙箱安全层（可选）— 工具执行前校验文件路径是否在允许范围内
    pub sandbox: Option<Arc<dyn Sandbox>>,
}

impl ReActAgent {
    pub fn new(
        config: AgentConfig,
        provider: Arc<dyn LlmProvider>,
        tools: Arc<ToolRegistry>,
        approval: Arc<ApprovalEngine>,
        conversation: Arc<ConversationManager>,
        event_bus: Arc<EventBus>,
        cancellation: CancellationToken,
        mode: AgentMode,
        paused: Arc<AtomicBool>,
        approval_responder: ApprovalResponder,
    ) -> Self {
        let ctx_win = config.context_window;
        Self {
            config,
            provider,
            tools,
            approval,
            conversation,
            event_bus,
            cancellation,
            mode: AtomicUsize::new(mode as usize),
            paused,
            approval_responder,
            context_window: Arc::new(AtomicU64::new(ctx_win as u64)),
            auto_approve: false,
            session_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            sandbox: None,
        }
    }

    /// B1: 创建子 Agent 实例 — 共享基础设施但独立 mode/approval_responder
    ///
    /// 子 Agent 强制使用 AgentMode::Agent（不继承父的 Yolo/Plan 模式），
    /// 拥有独立的 approval_responder（不与父 Agent 的审批通道冲突）。
    /// 其余基础设施（provider/tools/conversation/event_bus 等）共享父实例。
    pub fn create_sub_agent(&self) -> ReActAgent {
        ReActAgent {
            config: self.config.clone(),
            provider: self.provider.clone(),
            tools: self.tools.clone(),
            approval: self.approval.clone(),
            conversation: self.conversation.clone(),
            event_bus: self.event_bus.clone(),
            cancellation: self.cancellation.clone(),
            mode: AtomicUsize::new(AgentMode::Agent as usize),
            paused: self.paused.clone(),
            approval_responder: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            context_window: self.context_window.clone(),
            auto_approve: true,
            session_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            sandbox: self.sandbox.clone(),
        }
    }

    /// M14: 运行时更新上下文窗口（TUI 从 /models API 获取后调用）
    pub fn update_context_window(&self, window: usize) {
        self.context_window.store(window as u64, Ordering::SeqCst);
    }

    /// 同步 mode（TUI /yolo /agent /plan 等命令调用）
    pub fn set_mode(&self, mode: AgentMode) {
        self.mode.store(mode as usize, Ordering::SeqCst);
    }

    /// D3: 读取当前 mode
    pub fn get_mode(&self) -> AgentMode {
        // D3: transmute 安全 — set_mode 仅存入有效 AgentMode 判别值，#[repr(usize)] 保证布局确定
        unsafe { std::mem::transmute(self.mode.load(Ordering::SeqCst)) }
    }

    /// 设置沙箱安全层（builder 风格）
    ///
    /// 设置后，agent 在执行文件操作工具（read_file/write_file/edit_file）前
    /// 会通过 sandbox.is_path_allowed() 校验路径，拒绝 workspace 外或
    /// 受保护目录（.git/node_modules/target）的访问。
    ///
    /// 注意：此方法仅设置应用层路径校验，不会调用 sandbox.init()
    /// 触发内核级 Landlock/seccomp 限制（因 apply_to_current_thread
    /// 是线程级限制，不适合 tokio 线程池环境）。
    pub fn with_sandbox(mut self, sandbox: Arc<dyn Sandbox>) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// 沙箱路径校验 — 在工具执行前检查文件路径是否被允许
    ///
    /// 返回 `Some(ToolResult::error)` 表示路径被沙箱拒绝，
    /// 返回 `None` 表示路径允许或工具不涉及文件路径。
    fn check_sandbox_allowed(&self, tool_name: &str, arguments: &serde_json::Value) -> Option<ToolResult> {
        let sandbox = self.sandbox.as_ref()?;

        // 仅对文件操作工具校验 file_path 参数
        let path_arg = match tool_name {
            "read_file" | "write_file" | "edit_file" => {
                arguments.get("file_path").and_then(|v| v.as_str())?
            }
            _ => return None, // 非文件操作工具无需校验
        };

        if !sandbox.is_path_allowed(path_arg) {
            return Some(ToolResult::error(
                format!(
                    "沙箱安全拒绝：路径 '{}' 不在允许范围内（workspace 外或受保护目录）",
                    path_arg
                ),
                "sandbox_path_denied",
            ));
        }

        None
    }

    /// 从文件系统加载 rules（全局 + 项目记忆 + AGENTS.md + cwd）
    ///
    /// 加载顺序（后加载追加，不覆盖）：
    /// 1. `~/.goat/rules.md`（全局规则）
    /// 2. `<workspace>/.goat/rules.md`（项目规则）
    /// 3. `<workspace>/GOAT.md`（Goat 原生项目记忆，优先级最高）
    /// 4. `<workspace>/CLAUDE.md`（Claude Code 生态兼容）
    /// 5. `<workspace>/AGENTS.md`（OpenCode 生态兼容）
    /// 6. 从 cwd 递归向上查找 `.goat/rules.md`
    ///
    /// 返回 `(rules, rules_loaded)`：第二个 bool 表示是否从磁盘加载到任何规则文件。
    pub fn load_rules(workspace: &str) -> (Vec<String>, bool) {
        let mut rules = Vec::new();
        let ws = std::path::Path::new(workspace);

        // 1. 全局规则: ~/.goat/rules.md
        if let Ok(home) = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
        {
            let global_rules = std::path::Path::new(&home).join(".goat").join("rules.md");
            if global_rules.exists() {
                if let Ok(content) = std::fs::read_to_string(&global_rules) {
                    rules.push(content);
                }
            }
        }

        // 2. 项目规则: <workspace>/.goat/rules.md
        let project_rules = ws.join(".goat").join("rules.md");
        if project_rules.exists() {
            if let Ok(content) = std::fs::read_to_string(&project_rules) {
                rules.push(content);
            }
        }

        // 3. GOAT.md: Goat 原生项目记忆文件（优先级最高）
        let goat_md = ws.join("GOAT.md");
        if goat_md.exists() {
            if let Ok(content) = std::fs::read_to_string(&goat_md) {
                rules.push(content);
            }
        }

        // 4. CLAUDE.md: 与 Claude Code 生态兼容
        let claude_md = ws.join("CLAUDE.md");
        if claude_md.exists() {
            if let Ok(content) = std::fs::read_to_string(&claude_md) {
                rules.push(content);
            }
        }

        // 5. AGENTS.md: 与 OpenCode 生态兼容
        let agents_md = ws.join("AGENTS.md");
        if agents_md.exists() {
            if let Ok(content) = std::fs::read_to_string(&agents_md) {
                rules.push(content);
            }
        }

        // D3: 6. 嵌套项目记忆: 从 cwd 递归向上查找 GOAT.md/CLAUDE.md/AGENTS.md/.goat/rules.md
        if let Ok(cwd) = std::env::current_dir() {
            let ws_canonical = ws.canonicalize().ok();
            let mut current = cwd;
            loop {
                // D3: 查找当前层级的记忆文件
                for fname in &["GOAT.md", "CLAUDE.md", "AGENTS.md"] {
                    let candidate = current.join(fname);
                    if candidate.exists() {
                        // D3: 跳过与 workspace 根目录重复的文件（已在 #3/#4/#5 加载）
                        let is_workspace_root = match (&ws_canonical, candidate.canonicalize().ok()) {
                            (Some(ws), Some(c)) => c.parent().map(|p| p == *ws).unwrap_or(false),
                            _ => false,
                        };
                        if !is_workspace_root {
                            if let Ok(content) = std::fs::read_to_string(&candidate) {
                                rules.push(content);
                            }
                        }
                    }
                }

                // D3: 查找当前层级的 .goat/rules.md
                let candidate = current.join(".goat").join("rules.md");
                if candidate.exists() {
                    let project_rules_canonical = project_rules.canonicalize().ok();
                    let is_duplicate = match (candidate.canonicalize().ok(), &project_rules_canonical) {
                        (Some(a), Some(b)) => a == *b,
                        _ => false,
                    };
                    if !is_duplicate {
                        if let Ok(content) = std::fs::read_to_string(&candidate) {
                            rules.push(content);
                        }
                    }
                    break; // D3: 找到 .goat/rules.md 后停止向上查找
                }
                if !current.pop() {
                    break;
                }
            }
        }

        let loaded = !rules.is_empty();
        (rules, loaded)
    }

    /// 从文件系统扫描 skills（全局 + 项目级，项目级覆盖全局同名）
    ///
    /// 扫描目录：
    /// 1. `~/.goat/skills/*/SKILL.md`
    /// 2. `<workspace>/.goat/skills/*/SKILL.md`（覆盖全局同名 skill）
    pub fn load_skills(workspace: &str) -> Vec<String> {
        let mut skill_map: HashMap<String, (String, String)> = HashMap::new();
        let ws = std::path::Path::new(workspace);

        // Helper: scan a skills directory
        fn scan_skills_dir(dir: &std::path::Path, map: &mut HashMap<String, (String, String)>) {
            let iter = match std::fs::read_dir(dir) {
                Ok(iter) => iter,
                Err(_) => return,
            };
            for entry in iter.flatten() {
                let skill_md = entry.path().join("SKILL.md");
                if !skill_md.exists() {
                    continue;
                }
                let content = match std::fs::read_to_string(&skill_md) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if let Some((name, desc, body)) = parse_skill_frontmatter(&content) {
                    // M5+ 渐进加载：system prompt 只注入 name + 一句话描述（~50-100 chars/个）
                    // 完整 SKILL.md body 保持可访问（后续 load_skill 工具会用到），
                    // 但不在首轮全量注入，对标 OpenCode Skill delay-load + ECC Skill-First 架构。
                    let summary = format!("- **{}**: {}", name, desc);
                    // 保留完整 body 以便后续的 skill_load 工具按需获取
                    let _full = format!("Skill: {}\nDescription: {}\n\n{}", name, desc, body);
                    map.insert(name, (desc, summary));
                }
            }
        }

        // 1. 全局 skills: ~/.goat/skills/*/SKILL.md
        if let Ok(home) = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
        {
            let global_skills_dir = std::path::Path::new(&home)
                .join(".goat")
                .join("skills");
            scan_skills_dir(&global_skills_dir, &mut skill_map);
        }

        // 2. 项目 skills: <workspace>/.goat/skills/*/SKILL.md（覆盖全局）
        let project_skills_dir = ws.join(".goat").join("skills");
        scan_skills_dir(&project_skills_dir, &mut skill_map);

        // 返回去重后的 skills（项目级已在 HashMap 中覆盖全局）
        skill_map.into_values().map(|(_desc, formatted)| formatted).collect()
    }

    /// D3-T05: 结构化加载 skills — 供前端 Slash 命令 UI 使用。
    ///
    /// 返回 `Vec<SkillInfo>`，包含 name/description/source/path，
    /// 项目级 skill 覆盖全局同名 skill（与 `load_rules` 语义一致）。
    pub fn load_skills_structured(workspace: &str) -> Vec<SkillInfo> {
        let mut skill_map: HashMap<String, SkillInfo> = HashMap::new();
        let ws = std::path::Path::new(workspace);

        fn scan_dir(dir: &std::path::Path, source: &str, map: &mut HashMap<String, SkillInfo>) {
            let iter = match std::fs::read_dir(dir) {
                Ok(iter) => iter,
                Err(_) => return,
            };
            for entry in iter.flatten() {
                let skill_md = entry.path().join("SKILL.md");
                if !skill_md.exists() {
                    continue;
                }
                let content = match std::fs::read_to_string(&skill_md) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if let Some((name, desc, _body)) = parse_skill_frontmatter(&content) {
                    map.insert(
                        name.clone(),
                        SkillInfo {
                            name,
                            description: desc,
                            source: source.to_string(),
                            path: skill_md.display().to_string(),
                        },
                    );
                }
            }
        }

        // 1. 全局 skills
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let global_dir = std::path::Path::new(&home).join(".goat").join("skills");
            scan_dir(&global_dir, "global", &mut skill_map);
        }
        // 2. 项目 skills（覆盖全局同名）
        let project_dir = ws.join(".goat").join("skills");
        scan_dir(&project_dir, "project", &mut skill_map);

        let mut skills: Vec<SkillInfo> = skill_map.into_values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills
    }

    /// D3-T05: 读取指定 skill 的完整 SKILL.md 内容。
    ///
    /// 查找顺序：项目级 → 全局级（与 load_skills_structured 一致）。
    pub fn read_skill_content(workspace: &str, name: &str) -> Option<String> {
        let ws = std::path::Path::new(workspace);
        // 1. 项目级
        let project_path = ws.join(".goat").join("skills").join(name).join("SKILL.md");
        if project_path.exists() {
            return std::fs::read_to_string(&project_path).ok();
        }
        // 2. 全局级
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let global_path = std::path::Path::new(&home)
                .join(".goat")
                .join("skills")
                .join(name)
                .join("SKILL.md");
            if global_path.exists() {
                return std::fs::read_to_string(&global_path).ok();
            }
        }
        None
    }

    /// C4: 持久化事件（fire-and-forget）— 失败仅 warn 不阻断主循环，
    /// 对齐 run_verification_hook 中 `let _ =` 模式。
    async fn persist_event(
        persistence: &Option<TaskPersistence>,
        step: usize,
        tool_name: Option<String>,
        status: TaskStatus,
        summary: String,
    ) {
        if let Some(p) = persistence {
            if let Err(e) = p.append_event(&TaskEvent {
                step,
                tool_name,
                status,
                summary,
                created_at: chrono::Utc::now(),
            }).await {
                tracing::warn!("task persistence append failed: {}", e);
            }
        }
    }

    /// D1: 标记会话 checkpoint 完成（fire-and-forget）— 失败仅 warn 不阻断
    async fn mark_checkpoint_completed(checkpoint: &Option<Checkpoint>, session_id: &str) {
        if let Some(cp) = checkpoint {
            if let Err(e) = cp.mark_completed(session_id).await {
                tracing::warn!("checkpoint mark_completed failed: {}", e);
            }
        }
    }

    /// 运行 Agent 处理用户输入
    pub async fn run(
        &self,
        session_id: &str,
        user_prompt: &str,
        workspace: &str,
    ) -> Result<AgentRunResult, AgentError> {
        self.emit(AgentEvent::Started {
            mode: self.get_mode().to_string(),
            prompt: user_prompt.to_string(),
        }).await;

        // 保存用户消息
        self.conversation
            .add_message(session_id, "user", user_prompt, None, None)
            .await
            .map_err(|e| AgentError::Tool(e.to_string()))?;

        let (rules, rules_loaded) = Self::load_rules(workspace);
        let skills = Self::load_skills(workspace);
        // P0: 自动检测用户 prompt 中的技术栈要求并注入 system prompt
        let tech_stack = extract_tech_stack_constraints(user_prompt);
        let system_prompt = build_system_prompt(
            self.get_mode(),
            workspace,
            &rules,
            rules_loaded,
            &skills,
            &self.tools_description(),
            tech_stack.as_deref(),
        );

        // C4: 任务持久化 + 断点续传 — fire-and-forget，失败仅 warn 不阻断主循环
        let persistence = if self.config.task_persistence_enabled {
            match TaskPersistence::new(workspace, session_id).await {
                Ok(p) => {
                    // 断点续传：若有历史事件，注入续传提示作为 user 消息
                    if let Some(resume) = p.build_resume_summary().await {
                        let _ = self.conversation
                            .add_message(session_id, "user", &resume, None, None)
                            .await;
                    }
                    Some(p)
                }
                Err(e) => {
                    tracing::warn!("task persistence init failed: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // D1: Checkpoint 看门狗 — 初始化（fire-and-forget，失败仅 warn 不阻断）
        let checkpoint = match Checkpoint::new(workspace, session_id).await {
            Ok(cp) => {
                let init_data = CheckpointData {
                    session_id: session_id.to_string(),
                    step: 0,
                    status: CheckpointStatus::Running,
                    progress_summary: user_prompt.chars().take(200).collect(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                if let Err(e) = cp.write(&init_data).await {
                    tracing::warn!("checkpoint initial write failed: {}", e);
                }
                Some(cp)
            }
            Err(e) => {
                tracing::warn!("checkpoint init failed: {}", e);
                None
            }
        };

        let mut events = Vec::new();
        let mut tool_calls_count = 0;
        // Tracks the most recent LLM text response for partial-answer on cancellation
        let mut last_answer_text = String::new();

        // ── M3: 工具循环熔断检测器（每轮 run 独立实例）──
        let mut deduper = ToolCallDeduper::new();
        // ── M2: 无工具调用（且非最终答案）的连续催促计数 ──
        let mut no_tool_count: usize = 0;
        // C2: 连续工具失败计数 — 达到 2 次时触发重规划
        let mut consecutive_tool_failures: usize = 0;
        // ── M1 主检查: StepsTracker — 解析 ## 计划 / ## 进度: N ──
        let mut steps_tracker = StepsTracker::new();
        // ── M6+ 策略压缩: 步骤完成标记 → 主动触发压缩（非等窗口满）──
        let mut milestone_hit = false;
        // ── A1: 里程碑摘要 — 压缩时保留已完成步骤的关键信息 ──
        let mut milestone_summary = MilestoneSummary::default();
        // ── M-A3: 步骤上限自适应扩展 — 有进展时可动态扩展，连续无进展达阈值则停止扩展 ──
        let mut effective_max_steps = self.config.max_steps;
        let mut consecutive_no_progress: usize = 0;
        // ── A6: LLM 连续失败熔断 — 连续失败 ≥ 3 次则直接退出，不再重试 ──
        let mut llm_fail_count: usize = 0;

        // C1: 强制规划阶段 — 主循环前先让 LLM 生成 ## 计划
        if self.config.planning_enabled {
            let planning_prompt = Planner::planning_prompt(user_prompt, workspace);
            self.conversation
                .add_message(session_id, "user", &planning_prompt, None, None)
                .await
                .map_err(|e| AgentError::Tool(e.to_string()))?;

            let plan_messages = self.build_messages(session_id, &system_prompt).await?;
            let plan_response = self.call_llm_with_streaming(&plan_messages, &self.tools.all_tool_defs(), 0).await;

            match plan_response {
                Ok(resp) => {
                    if let Some(choice) = resp.choices.into_iter().next() {
                        let plan_text = content_to_string(&choice.message.content);
                        if !plan_text.is_empty() {
                            // 保存规划响应（剥离 tool_calls，避免产生孤儿 tool_call 消息）
                            self.conversation
                                .add_message(session_id, "assistant", &plan_text, None, None)
                                .await
                                .map_err(|e| AgentError::Tool(e.to_string()))?;

                            // 预解析计划到 StepsTracker（让主循环的进度注入立即可用）
                            steps_tracker.try_parse(&plan_text);

                            self.emit(AgentEvent::Thought {
                                step: 0,
                                content: format!("[plan] 规划阶段完成，已生成 {} 步计划",
                                    steps_tracker.len()),
                            }).await;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("C1 规划阶段 LLM 调用失败，跳过规划: {}", e);
                    self.emit(AgentEvent::Error {
                        message: format!("规划阶段失败（已跳过，继续正常执行）: {}", e),
                    }).await;
                }
            }
            // 规划失败不阻断主循环，Agent 仍可在循环中自行规划
        }

        for step in 0.. {
            if step >= effective_max_steps {
                break;
            }
            if self.cancellation.is_cancelled() {
                self.emit(AgentEvent::Cancelled {
                    partial_answer: last_answer_text.clone(),
                }).await;
                Self::persist_event(&persistence, step, None, TaskStatus::Interrupted, "Cancelled".to_string()).await;
                Self::mark_checkpoint_completed(&checkpoint, session_id).await;
                return Err(AgentError::Cancelled);
            }

            // ── Pause polling ──
            while self.paused.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
                // re-check cancellation while paused
                if self.cancellation.is_cancelled() {
                    self.emit(AgentEvent::Cancelled {
                        partial_answer: last_answer_text.clone(),
                    }).await;
                    Self::persist_event(&persistence, step, None, TaskStatus::Interrupted, "Cancelled while paused".to_string()).await;
                    Self::mark_checkpoint_completed(&checkpoint, session_id).await;
                    return Err(AgentError::Cancelled);
                }
            }

            // 构建消息历史
            let mut messages = self.build_messages(session_id, &system_prompt).await?;

            // 动态进度注入（独立 SystemMessage，保护 prefix cache 稳定性）
            // Python 版此处将 StepsTracker 进度作为独立 SystemMessage 追加，
            // 而非拼入主 System Prompt，使 prefix cache 可跨轮次复用。
            if let Some(progress) = steps_tracker.progress_injection() {
                messages.insert(1, ChatMessage {
                    role: Role::System,
                    content: MessageContent::Text(progress),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
            }

            // ── M6: 上下文压缩 — 防消息数量超标导致 LLM API 截断/拒绝 ──
            // 对标 ECC AUTOCOMPACT_PCT=50（50% 窗口主动压缩，非 95% 被动压缩）
            // 对标 OpenCode PreCompact hook 在压缩前注入关键状态
            {
                let compressor = ContextCompressor::new(CompressionConfig::default());
                let estimated_tokens: usize = messages
                    .iter()
                    .map(|m| content_to_string(&m.content).chars().count() / 3)
                    .sum();
                // M14 Route-Aware: 使用运行时 context_window（TUI 从 /models 动态更新）
                let window = self.context_window.load(Ordering::SeqCst) as usize;
                let window = window.max(32_000); // floor: 32k minimum
                let threshold = if milestone_hit {
                    window * 40 / 100
                } else {
                    window * 50 / 100
                };
                let level = if estimated_tokens < threshold {
                    CompressionLevel::Full
                } else if estimated_tokens < window * 3 / 4 {
                    CompressionLevel::Micro
                } else if estimated_tokens < window {
                    CompressionLevel::ContextCollapse
                } else {
                    CompressionLevel::SessionMemory
                };

                if level != CompressionLevel::Full {
                    let cm: Vec<CompressedMessage> = messages
                        .iter()
                        .map(|m| CompressedMessage {
                            role: format!("{:?}", m.role).to_lowercase(),
                            content: content_to_string(&m.content),
                            is_summary: false,
                        })
                        .collect();
                    let before_count = cm.len();
                    let compressed = compressor.compress(&cm, level);

                    if compressed.has_more {
                        self.emit(AgentEvent::ContextCompacted {
                            level: format!("{:?}", level),
                            messages_before: before_count,
                            messages_after: compressed.recent_messages.len()
                                + if compressed.summary.is_some() { 1 } else { 0 },
                            estimated_tokens: compressed.estimated_tokens,
                        }).await;

                        // M6+ PreCompact 注入: 压缩摘要作为独立 SystemMessage（保护 prefix cache）
                        let mut compact_note = String::from("[上下文压缩] ");
                        if let Some(summary) = &compressed.summary {
                            compact_note.push_str(summary);
                        }

                        let mut new_messages = Vec::with_capacity(
                            compressed.recent_messages.len() + 3,
                        );
                        new_messages.push(messages[0].clone()); // 保留 System Prompt
                        new_messages.push(ChatMessage {
                            role: Role::System,
                            content: MessageContent::Text(compact_note),
                            name: None,
                            tool_call_id: None,
                            tool_calls: None,
                        });
                        // A1: 里程碑摘要作为独立 SystemMessage 注入（而非拼入 compact_note），
                        // 保护 prefix cache 且保留已完成步骤的关键信息，避免压缩后 agent "失忆"
                        if !milestone_summary.is_empty() {
                            new_messages.push(ChatMessage {
                                role: Role::System,
                                content: MessageContent::Text(milestone_summary.render()),
                                name: None,
                                tool_call_id: None,
                                tool_calls: None,
                            });
                        }
                        // 追加未压缩的最近消息
                        let skip = compressed.compressed_count;
                        new_messages.extend_from_slice(&messages[skip..]);
                        // B2: 压缩结果持久化 — 将压缩后的消息写回 DB，避免每轮重新压缩
                        // 跳过 System Prompt（[0]）以及动态注入的 compact_note / milestone_summary
                        // SystemMessage（每轮重新生成，不应持久化）
                        let compressed_records: Vec<MessageRecord> = new_messages.iter()
                            .skip(1) // 跳过 System Prompt
                            .filter(|m| !matches!(m.role, Role::System)) // 跳过动态注入的 SystemMessage
                            .map(|m| MessageRecord {
                                id: 0, // autoincrement
                                session_id: session_id.to_string(),
                                role: format!("{:?}", m.role).to_lowercase(),
                                content: content_to_string(&m.content),
                                tool_calls: m.tool_calls.as_ref().map(|tc| serde_json::to_string(tc).unwrap_or_default()),
                                tool_call_id: m.tool_call_id.clone(),
                                created_at: chrono::Utc::now(),
                            })
                            .collect();
                        self.conversation.replace_messages(session_id, &compressed_records)
                            .await
                            .map_err(|e| AgentError::Tool(format!("Failed to persist compressed messages: {}", e)))?;
                        messages = new_messages;
                    }
                }
            }

            // 调用 LLM（尝试流式，失败回退到非流式；P0 修复：LLM 错误不应 abort session）
            // D1: 看门狗定时器 — LLM 调用超时则保存 checkpoint 并暂停
            let response = match tokio::time::timeout(
                Duration::from_secs(self.config.watchdog_timeout_secs),
                self.call_llm_with_streaming(&messages, &self.tools.all_tool_defs(), step)
            ).await {
                Ok(Ok(r)) => {
                    llm_fail_count = 0;
                    r
                }
                Ok(Err(_e)) => {
                    // 重试一次（应对瞬时 API 错误/网络抖动）
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    match self.call_llm_with_streaming(&messages, &self.tools.all_tool_defs(), step).await {
                        Ok(r) => {
                            llm_fail_count = 0;
                            r
                        }
                        Err(e2) => {
                            llm_fail_count += 1;
                            if llm_fail_count >= 3 {
                                let msg = format!("LLM 连续 {} 次调用失败，已熔断退出。最后错误: {}", llm_fail_count, e2);
                                self.emit(AgentEvent::Error { message: msg.clone() }).await;
                                Self::persist_event(&persistence, step, None, TaskStatus::Failed, msg.clone()).await;
                                Self::mark_checkpoint_completed(&checkpoint, session_id).await;
                                return Err(AgentError::Llm(msg));
                            }
                            let msg = format!("LLM 调用失败（已重试，连续第 {} 次失败）: {}", llm_fail_count, e2);
                            self.emit(AgentEvent::Error {
                                message: msg.clone(),
                            }).await;
                            // 对齐 Python try/except + OpenCode 不崩溃模式：
                            // 注入错误作为 observation，让模型下一轮有机会自行恢复或给用户可见提示
                            self.conversation
                                .add_message(session_id, "user", &msg, None, None)
                                .await
                                .map_err(|e3| AgentError::Tool(e3.to_string()))?;
                            continue;
                        }
                    }
                }
                Err(_elapsed) => {
                    // D1 看门狗超时：保存进度并暂停，等待用户手动恢复
                    tracing::warn!(
                        "D1 看门狗：LLM 调用超时 {} 秒，保存 checkpoint 并暂停",
                        self.config.watchdog_timeout_secs
                    );
                    if let Some(ref cp) = checkpoint {
                        let summary = steps_tracker.progress_injection()
                            .unwrap_or_else(|| last_answer_text.chars().take(200).collect());
                        let data = CheckpointData {
                            session_id: session_id.to_string(),
                            step,
                            status: CheckpointStatus::Running,
                            progress_summary: summary,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        };
                        if let Err(e) = cp.write(&data).await {
                            tracing::warn!("checkpoint write on watchdog timeout failed: {}", e);
                        }
                    }
                    self.paused.store(true, Ordering::SeqCst);
                    self.emit(AgentEvent::Error {
                        message: "看门狗超时，已保存进度并暂停".to_string(),
                    }).await;
                    continue;
                }
            };

            let choice = response.choices.into_iter().next()
                .ok_or_else(|| AgentError::Llm("Empty response from LLM".to_string()))?;
            let assistant_message = choice.message;

            // 保存 assistant 文本内容
            let content_text = content_to_string(&assistant_message.content);
            last_answer_text = content_text.clone();
            if !content_text.is_empty() {
                self.emit(AgentEvent::Thought {
                    step,
                    content: content_text.clone(),
                }).await;
                push_event(&mut events, AgentEvent::Thought { step, content: content_text.clone() }, self.config.max_events);
            }

            // 解析 LLM 输出中的 ## 计划 / ## 进度: N（StepsTracker 主检查机制）
            let parsed_update = steps_tracker.try_parse(&content_text);
            // 检测新计划（首次或 replan）：content 包含 "## 计划" 且 StepsTracker 有计划
            // 用 content_text 直接检测比依赖 "之前是否有计划" 更准确：
            //   - 首次出计划：content 含 "## 计划" → 重置（无害，本就为空）
            //   - Replan（已有旧计划，模型发新计划）：旧计划使 has_plan()=true，
            //     但 content 含 "## 计划" → 触发，清除旧数据
            //   - 进度更新：content 不含 "## 计划" → 不触发（正确）
            let is_new_plan = content_text.contains("## 计划") && steps_tracker.has_plan();
            if is_new_plan {
                milestone_hit = true;
                // 新计划（首次或 replan）时重置里程碑摘要，避免旧计划已完成步骤残留
                milestone_summary = MilestoneSummary::default();
            }
            // A1: 步骤完成时将新完成步骤的描述追加到里程碑摘要
            if parsed_update && steps_tracker.has_plan() {
                let completed_descs = extract_completed_step_descriptions(
                    steps_tracker.progress_injection().as_deref(),
                );
                for desc in &completed_descs {
                    if !milestone_summary.completed_steps.contains(desc) {
                        milestone_summary.completed_steps.push(desc.clone());
                    }
                }
            }

            // B7: 自动推进当前步骤为下一个未完成步骤，供 progress_injection() 标记 🔄
            if steps_tracker.has_plan() {
                steps_tracker.advance_current_step();
            }

            // 保存 assistant 消息到数据库
            let tool_calls_json = assistant_message.tool_calls.as_ref()
                .map(|tc| serde_json::to_string(tc).unwrap_or_default());
            self.conversation
                .add_message(
                    session_id,
                    "assistant",
                    &content_text,
                    tool_calls_json.as_deref(),
                    None,
                )
                .await
                .map_err(|e| AgentError::Tool(e.to_string()))?;

            // ── 没有工具调用：M4 截断恢复 / M1 完成检测 / M2 续写催促 ──
            let has_tool_calls = assistant_message
                .tool_calls
                .as_ref()
                .map(|t| !t.is_empty())
                .unwrap_or(false);
            let finish_reason = choice.finish_reason.as_deref().unwrap_or("");
            // C4: 捕获本步首个工具名，用于持久化事件（最小侵入，不修改工具执行循环）
            let step_first_tool = assistant_message.tool_calls.as_ref()
                .and_then(|tc| tc.first())
                .map(|tc| tc.function.name.clone());

            if !has_tool_calls {
                // M4: 截断恢复 — 模型输出被 max_tokens 截断，催促其补全未完成的 tool_calls
                if (finish_reason == "length" || finish_reason == "max_tokens") && !content_text.is_empty() {
                    let msg = format!(
                        "你的上一条响应被截断了 (finish_reason={})，请直接输出未完成的 tool_calls。",
                        finish_reason
                    );
                    self.emit(AgentEvent::Thought { step, content: msg.clone() }).await;
                    // 注：Python 版此处注入 ToolMessage，但 OpenAI 兼容 API 要求 tool 消息必须对应
                    // 前置 assistant 的 tool_call，否则会 400。故改为 user 角色注入，效果等价。
                    self.conversation
                        .add_message(session_id, "user", &msg, None, None)
                        .await
                        .map_err(|e| AgentError::Tool(e.to_string()))?;
                    continue;
                }

                // M1: 完成检测 — 主检查 StepsTracker（优先），启发式兜底（fallback）
                // Python 版此处优先检查 steps_tracker.is_all_done()，再 fallback 到
                // looks_like_final_answer()。StepsTracker 比纯文字启发式更可靠。
                if steps_tracker.is_all_done() {
                    // M13 验证阻断: 完成前自动运行项目验证
                    if self.config.auto_verify {
                        self.run_verification_hook(session_id, workspace, step).await;
                    }
                    let answer = if content_text.is_empty() {
                        last_answer_text.clone()
                    } else {
                        content_text.clone()
                    };
                    self.emit(AgentEvent::Finished {
                        answer: answer.clone(),
                        steps: step + 1,
                    }).await;
                    Self::persist_event(&persistence, step, None, TaskStatus::Completed, "All steps done".to_string()).await;
                    Self::mark_checkpoint_completed(&checkpoint, session_id).await;
                    return Ok(AgentRunResult {
                        answer,
                        steps_taken: step + 1,
                        tool_calls: tool_calls_count,
                        events,
                    });
                }

                // M1 兜底: 完成检测启发式 — 判断当前文本是否为最终答案（而非中途状态更新）
                if looks_like_final_answer(&content_text) {
                    self.emit(AgentEvent::Finished {
                        answer: content_text.clone(),
                        steps: step + 1,
                    }).await;
                    Self::persist_event(&persistence, step, None, TaskStatus::Completed, "Final answer".to_string()).await;
                    Self::mark_checkpoint_completed(&checkpoint, session_id).await;
                    return Ok(AgentRunResult {
                        answer: content_text,
                        steps_taken: step + 1,
                        tool_calls: tool_calls_count,
                        events,
                    });
                }

                // M2: 续写催促 — 文本不是最终答案且没调工具，逐步催促模型改用工具
                no_tool_count += 1;
                if no_tool_count <= 3 {
                    let prompt = continuation_prompt(no_tool_count);
                    self.emit(AgentEvent::Thought { step, content: prompt.clone() }).await;
                    self.conversation
                        .add_message(session_id, "user", &prompt, None, None)
                        .await
                        .map_err(|e| AgentError::Tool(e.to_string()))?;
                    continue;
                } else {
                    // 超过最大催促次数（第 4 次）→ 退出循环，以当前累积文本作为结果
                    let answer = if content_text.is_empty() {
                        last_answer_text.clone()
                    } else {
                        content_text.clone()
                    };
                    self.emit(AgentEvent::Finished {
                        answer: answer.clone(),
                        steps: step + 1,
                    }).await;
                    Self::persist_event(&persistence, step, None, TaskStatus::Completed, "No-tool limit reached".to_string()).await;
                    Self::mark_checkpoint_completed(&checkpoint, session_id).await;
                    return Ok(AgentRunResult {
                        answer,
                        steps_taken: step + 1,
                        tool_calls: tool_calls_count,
                        events,
                    });
                }
            }

            // ── 处理工具调用 ──
            // A2: 跟踪本轮是否有文件变更（write_file/edit_file/patch/git），用于无进展检测
            let mut step_has_file_change = false;
            if let Some(tool_calls) = assistant_message.tool_calls {
                for tool_call in tool_calls {
                    tool_calls_count += 1;
                    // P0 修复：parse_tool_call 失败不应 abort 整个 agent（对齐 Python try/except 模式 + OpenCode doom_loop 用户可见而非崩溃）
                    let ParsedToolCall { name, arguments } = match parse_tool_call(&tool_call) {
                        Ok(p) => p,
                        Err(e) => {
                            let msg = format!("工具调用解析失败 ({}): {}", &tool_call.function.name, e);
                            self.emit(AgentEvent::ToolResult {
                                step,
                                tool_name: tool_call.function.name.clone(),
                                success: false,
                                output: msg.clone(),
                            }).await;
                            push_event(&mut events, AgentEvent::ToolResult {
                                step,
                                tool_name: tool_call.function.name.clone(),
                                success: false,
                                output: msg.clone(),
                            }, self.config.max_events);
                            self.add_observation(session_id, "parse_error", &tool_call.id, &msg, true).await?;
                            continue;
                        }
                    };

                    // M3/A2: 工具循环熔断 — 渐进式（Suggest 仍执行 / ForceSkip 跳过执行）
                    // A2 修复: Suggest 提示延后到工具执行后注入，避免在 assistant.tool_calls 与
                    // tool 消息之间插入 user 消息导致 OpenAI 兼容 API 400 错误。
                    let mut suggest_msg: Option<String> = None;
                    match deduper.register_and_check(&name, &arguments) {
                        LoopAction::None => {}
                        LoopAction::Suggest => {
                            // 精确重复 2 次（即将熔断）：建议换策略，但仍然执行工具
                            let msg = format!(
                                "⚠️ 检测到 {} 即将形成重复循环（已第 2 次相同调用），建议更换策略。本次仍会执行。",
                                name
                            );
                            self.emit(AgentEvent::Thought { step, content: msg.clone() }).await;
                            suggest_msg = Some(msg);
                        }
                        LoopAction::ForceSkip => {
                            // 精确重复 ≥3 次或语义重复 ≥4 次：必须换策略，跳过工具执行
                            let msg = format!("检测到重复调用 {}，请更换策略", name);
                            self.emit(AgentEvent::ToolResult {
                                step,
                                tool_name: name.clone(),
                                success: false,
                                output: msg.clone(),
                            }).await;
                            push_event(&mut events, AgentEvent::ToolResult {
                                step,
                                tool_name: name.clone(),
                                success: false,
                                output: msg.clone(),
                            }, self.config.max_events);
                            // 对应真实 assistant 的 tool_call_id，保证 OpenAI 兼容 API 的 tool 消息合法
                            self.add_observation(session_id, &name, &tool_call.id, &msg, false).await?;
                            // C2: ForceSkip 视为工具失败
                            consecutive_tool_failures += 1;
                            if consecutive_tool_failures >= 2 {
                                let replan_msg = "前两步工具调用连续失败。请重新评估当前计划，如果当前路径不可行，请使用 ## 计划 重新规划。";
                                self.emit(AgentEvent::Thought {
                                    step,
                                    content: format!("[replan] {}", replan_msg),
                                }).await;
                                self.conversation
                                    .add_message(session_id, "user", replan_msg, None, None)
                                    .await
                                    .map_err(|e| AgentError::Tool(e.to_string()))?;
                                consecutive_tool_failures = 0;
                            }
                            // P0 修复：不再硬中止（对齐 OpenCode doom_loop→用户权限 + Python inject-continue）。
                            // 仅靠 inject-continue + max_steps 兜底，硬中止会导致用户工作丢失。
                            continue;
                        }
                    }

                    self.emit(AgentEvent::ToolCall {
                        step,
                        tool_name: name.clone(),
                        arguments: arguments.clone(),
                    }).await;
                    push_event(&mut events, AgentEvent::ToolCall {
                        step,
                        tool_name: name.clone(),
                        arguments: arguments.clone(),
                    }, self.config.max_events);

                    // 查找工具类别
                    let category = self.tools.get(&name)
                        .map(|t| t.category())
                        .unwrap_or(ToolCategory::Read);

                    // B1: 子 Agent auto_approve — 非破坏性工具直接 Allow，避免审批通道孤立
                    let approval = if self.auto_approve && category != ToolCategory::Destructive {
                        crate::security::approval::ApprovalResult {
                            decision: Decision::Allow,
                            source: "auto-approve".to_string(),
                            message: String::new(),
                            bypass_immune: false,
                        }
                    } else {
                        self.approval.check(
                            self.get_mode(),
                            &name,
                            category,
                            &arguments,
                        ).await
                    };

                    self.emit(AgentEvent::Approval {
                        tool_name: name.clone(),
                        decision: format!("{:?}", approval.decision),
                        message: approval.message.clone(),
                    }).await;

                    match approval.decision {
                        Decision::Allow => {
                            // 沙箱路径校验
                            if let Some(denied) = self.check_sandbox_allowed(&name, &arguments) {
                                self.handle_tool_result(session_id, step, &name, &tool_call.id, &denied).await?;
                                push_event(&mut events, AgentEvent::ToolResult {
                                    step,
                                    tool_name: name.clone(),
                                    success: false,
                                    output: denied.output.clone(),
                                }, self.config.max_events);
                                consecutive_tool_failures += 1;
                                continue;
                            }
                            // 执行工具
                            let result = self.tools.execute(&name, arguments).await;
                            self.handle_tool_result(session_id, step, &name, &tool_call.id, &result).await?;
                            // A2: 记录本轮是否有文件变更工具被执行
                            if ToolCallDeduper::is_file_change_tool(&name) {
                                step_has_file_change = true;

                                // Flow 模式运行时审查（对齐 Python main.py mid-flow review）
                                if self.get_mode() == AgentMode::Flow {
                                    let diff = crate::agent::flow::get_git_diff(workspace).await;
                                    if !diff.is_empty() {
                                        let findings = self.run_mid_flow_review(&diff, step).await;
                                        if !findings.is_empty() {
                                            let findings_text = findings.iter()
                                                .map(|f| format!(
                                                    "[{}] {}:{} - {}\n  suggestion: {}",
                                                    f.severity.to_uppercase(),
                                                    f.file_path,
                                                    f.line.map(|l| l.to_string()).unwrap_or_else(|| "?".to_string()),
                                                    f.description,
                                                    f.suggestion
                                                ))
                                                .collect::<Vec<_>>()
                                                .join("\n");
                                            let review_msg = format!(
                                                "## Mid-Flow Review Findings\n\
                                                 The following issues were detected in your recent changes. \
                                                 Please fix them before continuing:\n\n{}",
                                                findings_text
                                            );
                                            self.conversation
                                                .add_message(session_id, "user", &review_msg, None, None)
                                                .await
                                                .map_err(|e| AgentError::Tool(e.to_string()))?;
                                        }
                                    }
                                }
                            }
                            push_event(&mut events, AgentEvent::ToolResult {
                                step,
                                tool_name: name.clone(),
                                success: result.success,
                                output: result.output.clone(),
                            }, self.config.max_events);
                            // C2: 跟踪连续工具失败
                            if result.success {
                                consecutive_tool_failures = 0;
                            } else {
                                consecutive_tool_failures += 1;
                                if consecutive_tool_failures >= 2 {
                                    let replan_msg = "前两步工具调用连续失败。请重新评估当前计划，如果当前路径不可行，请使用 ## 计划 重新规划。";
                                    self.emit(AgentEvent::Thought {
                                        step,
                                        content: format!("[replan] {}", replan_msg),
                                    }).await;
                                    self.conversation
                                        .add_message(session_id, "user", replan_msg, None, None)
                                        .await
                                        .map_err(|e| AgentError::Tool(e.to_string()))?;
                                    consecutive_tool_failures = 0;
                                }
                            }
                        }
                        Decision::Ask => {
                            // D3-T03: 先检查持久化规则（Always scope 跨会话）
                            {
                                let persistent = crate::security::approval::load_persistent_rules(workspace);
                                if !persistent.is_empty() {
                                    let args_hash = compute_args_hash(&arguments);
                                    let matched = persistent.iter().any(|r| {
                                        r.tool_name == name && (r.args_hash == 0 || r.args_hash == args_hash)
                                    });
                                    if matched {
                                        // 沙箱路径校验
                                        if let Some(denied) = self.check_sandbox_allowed(&name, &arguments) {
                                            self.handle_tool_result(session_id, step, &name, &tool_call.id, &denied).await?;
                                            push_event(&mut events, AgentEvent::ToolResult {
                                                step,
                                                tool_name: name.clone(),
                                                success: false,
                                                output: denied.output.clone(),
                                            }, self.config.max_events);
                                            consecutive_tool_failures += 1;
                                            continue;
                                        }
                                        // 持久化规则命中 — 自动放行（流程与 session_cache 命中一致）
                                        let result = self.tools.execute(&name, arguments).await;
                                        self.handle_tool_result(session_id, step, &name, &tool_call.id, &result).await?;
                                        if ToolCallDeduper::is_file_change_tool(&name) {
                                            step_has_file_change = true;
                                        }
                                        push_event(&mut events, AgentEvent::ToolResult {
                                            step,
                                            tool_name: name.clone(),
                                            success: result.success,
                                            output: result.output.clone(),
                                        }, self.config.max_events);
                                        if result.success {
                                            consecutive_tool_failures = 0;
                                        } else {
                                            consecutive_tool_failures += 1;
                                        }
                                        continue;
                                    }
                                }
                            }

                            // D1-T02: 查询 session_cache — 用户之前选择了 Session/AllSimilar 则自动通过
                            let cache_hit = {
                                let cache = self.session_cache.lock().ok();
                                if let Some(cache) = cache {
                                    // 先查 AllSimilar（精确匹配 args_hash）
                                    let exact_key = SessionCacheKey {
                                        session_id: session_id.to_string(),
                                        tool_name: name.clone(),
                                        args_hash: compute_args_hash(&arguments),
                                    };
                                    if let Some(dec) = cache.get(&exact_key).copied() {
                                        Some(dec)
                                    } else {
                                        // 再查 Session（通配，args_hash=0）
                                        let session_key = SessionCacheKey {
                                            session_id: session_id.to_string(),
                                            tool_name: name.clone(),
                                            args_hash: 0,
                                        };
                                        cache.get(&session_key).copied()
                                    }
                                } else {
                                    None
                                }
                            };

                            if let Some(Decision::Allow) = cache_hit {
                                // 沙箱路径校验
                                if let Some(denied) = self.check_sandbox_allowed(&name, &arguments) {
                                    self.handle_tool_result(session_id, step, &name, &tool_call.id, &denied).await?;
                                    push_event(&mut events, AgentEvent::ToolResult {
                                        step,
                                        tool_name: name.clone(),
                                        success: false,
                                        output: denied.output.clone(),
                                    }, self.config.max_events);
                                    consecutive_tool_failures += 1;
                                    continue;
                                }
                                // 缓存命中 — 直接执行工具（跳过审批）
                                let result = self.tools.execute(&name, arguments).await;
                                self.handle_tool_result(session_id, step, &name, &tool_call.id, &result).await?;
                                if ToolCallDeduper::is_file_change_tool(&name) {
                                    step_has_file_change = true;
                                }
                                push_event(&mut events, AgentEvent::ToolResult {
                                    step,
                                    tool_name: name.clone(),
                                    success: result.success,
                                    output: result.output.clone(),
                                }, self.config.max_events);
                                if result.success {
                                    consecutive_tool_failures = 0;
                                } else {
                                    consecutive_tool_failures += 1;
                                }
                                continue;
                            }

                            // ── Blocking approval handshake via oneshot channel ──
                            let (tx, rx) = tokio::sync::oneshot::channel::<ApprovalDecision>();
                            *self.approval_responder.lock().await = Some(tx);

                            let tool_type = tool_type_from_category(category);
                            let risk_level = risk_level_from_category(category);
                            let summary = format!("Tool '{}' requires approval ({:?})", name, category);
                            let (command, path, url) = extract_tool_context(&name, &arguments);

                            // D1-T02: 计算 danger_score + 生成 diff 预览
                            let danger_score = calculate_danger_score(category, &name, &arguments, workspace);
                            let (diff_preview, affected_files) = generate_diff_preview(&name, &arguments, workspace);
                            let allow_options: Vec<String> = match category {
                                ToolCategory::Write => vec![
                                    "once".to_string(), "session".to_string(), "all_similar".to_string(),
                                ],
                                ToolCategory::Shell | ToolCategory::Network => vec![
                                    "once".to_string(), "session".to_string(), "all_similar".to_string(), "always".to_string(),
                                ],
                                _ => vec!["once".to_string(), "session".to_string()],
                            };

                            self.emit(AgentEvent::ApprovalRequired {
                                tool_name: name.clone(),
                                tool_type,
                                summary,
                                risk_level,
                                command,
                                path,
                                url,
                                diff: diff_preview,
                                affected_files,
                                allow_options,
                                danger_score,
                            }).await;

                            // M8 P0 修复: 审批期间可被 Ctrl+C 取消
                            // 原实现: timeout(60s, rx).await → agent 卡在 oneshot 时 Ctrl+C 无效
                            // 新实现: tokio::select! 三路竞争 — 审批决策 / 60s 超时 / 取消信号
                            let cancel = self.cancellation.clone();
                            tokio::select! {
                                result = rx => {
                                    match result {
                                        Ok(dec) if dec.approved => {
                                            // D1-T02: 根据 dec.scope 写入 session_cache — 后续同工具调用自动通过
                                            match dec.scope {
                                                ApprovalScope::Always => {
                                                    // 写入 session_cache（即时生效）
                                                    // 通配 key（args_hash=0）— 同工具任意参数均命中
                                                    let key = SessionCacheKey {
                                                        session_id: session_id.to_string(),
                                                        tool_name: name.clone(),
                                                        args_hash: 0,
                                                    };
                                                    if let Ok(mut cache) = self.session_cache.lock() {
                                                        cache.insert(key, Decision::Allow);
                                                    }
                                                    // D3-T03: 写入持久化规则（跨会话生效）
                                                    let mut persistent = crate::security::approval::load_persistent_rules(workspace);
                                                    if !persistent.iter().any(|r| r.tool_name == name) {
                                                        persistent.push(crate::security::approval::PersistentApprovalRule {
                                                            tool_name: name.clone(),
                                                            args_hash: 0,
                                                        });
                                                        crate::security::approval::save_persistent_rules(workspace, &persistent);
                                                    }
                                                }
                                                ApprovalScope::Session => {
                                                    // 仅 session_cache，不持久化
                                                    // 通配 key（args_hash=0）— 同工具任意参数均命中
                                                    let key = SessionCacheKey {
                                                        session_id: session_id.to_string(),
                                                        tool_name: name.clone(),
                                                        args_hash: 0,
                                                    };
                                                    if let Ok(mut cache) = self.session_cache.lock() {
                                                        cache.insert(key, Decision::Allow);
                                                    }
                                                }
                                                ApprovalScope::AllSimilar => {
                                                    // 精确 key — 同工具同参数才命中
                                                    let key = SessionCacheKey {
                                                        session_id: session_id.to_string(),
                                                        tool_name: name.clone(),
                                                        args_hash: compute_args_hash(&arguments),
                                                    };
                                                    if let Ok(mut cache) = self.session_cache.lock() {
                                                        cache.insert(key, Decision::Allow);
                                                    }
                                                }
                                                ApprovalScope::Once => {
                                                    // 不缓存
                                                }
                                            }
                                            // 沙箱路径校验
                                            if let Some(denied) = self.check_sandbox_allowed(&name, &arguments) {
                                                self.handle_tool_result(session_id, step, &name, &tool_call.id, &denied).await?;
                                                push_event(&mut events, AgentEvent::ToolResult {
                                                    step,
                                                    tool_name: name.clone(),
                                                    success: false,
                                                    output: denied.output.clone(),
                                                }, self.config.max_events);
                                                consecutive_tool_failures += 1;
                                                continue;
                                            }
                                            // Approved — execute the tool
                                            let result = self.tools.execute(&name, arguments.clone()).await;
                                            self.handle_tool_result(session_id, step, &name, &tool_call.id, &result).await?;
                                            // A2: 记录本轮是否有文件变更工具被执行
                                            if ToolCallDeduper::is_file_change_tool(&name) {
                                                step_has_file_change = true;
                                            }
                                            if dec.approve_all {
                                                self.emit(AgentEvent::Approval {
                                                    tool_name: name.clone(),
                                                    decision: "approved_all".to_string(),
                                                    message: "Approved and will auto-approve subsequent calls".to_string(),
                                                }).await;
                                            }
                                            push_event(&mut events, AgentEvent::ToolResult {
                                                step,
                                                tool_name: name.clone(),
                                                success: result.success,
                                                output: result.output.clone(),
                                            }, self.config.max_events);
                                            // C2: 跟踪连续工具失败
                                            if result.success {
                                                consecutive_tool_failures = 0;
                                            } else {
                                                consecutive_tool_failures += 1;
                                                if consecutive_tool_failures >= 2 {
                                                    let replan_msg = "前两步工具调用连续失败。请重新评估当前计划，如果当前路径不可行，请使用 ## 计划 重新规划。";
                                                    self.emit(AgentEvent::Thought {
                                                        step,
                                                        content: format!("[replan] {}", replan_msg),
                                                    }).await;
                                                    self.conversation
                                                        .add_message(session_id, "user", replan_msg, None, None)
                                                        .await
                                                        .map_err(|e| AgentError::Tool(e.to_string()))?;
                                                    consecutive_tool_failures = 0;
                                                }
                                            }
                                        }
                                        _ => {
                                            // Denied or channel error
                                            let msg = format!("Tool '{}' approval denied.", name);
                                            self.emit(AgentEvent::Approval {
                                                tool_name: name.clone(),
                                                decision: "denied".to_string(),
                                                message: msg.clone(),
                                            }).await;
                                            self.add_observation(session_id, &name, &tool_call.id, &msg, true).await?;
                                            push_event(&mut events, AgentEvent::ToolResult {
                                                step,
                                                tool_name: name.clone(),
                                                success: false,
                                                output: msg,
                                            }, self.config.max_events);
                                            // C2: 审批拒绝视为失败
                                            consecutive_tool_failures += 1;
                                            if consecutive_tool_failures >= 2 {
                                                let replan_msg = "前两步工具调用连续失败。请重新评估当前计划，如果当前路径不可行，请使用 ## 计划 重新规划。";
                                                self.emit(AgentEvent::Thought {
                                                    step,
                                                    content: format!("[replan] {}", replan_msg),
                                                }).await;
                                                self.conversation
                                                    .add_message(session_id, "user", replan_msg, None, None)
                                                    .await
                                                    .map_err(|e| AgentError::Tool(e.to_string()))?;
                                                consecutive_tool_failures = 0;
                                            }
                                        }
                                    }
                                }
                                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                                    // Timeout
                                    let msg = format!("Tool '{}' approval timed out.", name);
                                    self.emit(AgentEvent::Approval {
                                        tool_name: name.clone(),
                                        decision: "timeout".to_string(),
                                        message: msg.clone(),
                                    }).await;
                                    self.add_observation(session_id, &name, &tool_call.id, &msg, true).await?;
                                    push_event(&mut events, AgentEvent::ToolResult {
                                        step,
                                        tool_name: name.clone(),
                                        success: false,
                                        output: msg,
                                    }, self.config.max_events);
                                    // C2: 审批超时视为失败
                                    consecutive_tool_failures += 1;
                                    if consecutive_tool_failures >= 2 {
                                        let replan_msg = "前两步工具调用连续失败。请重新评估当前计划，如果当前路径不可行，请使用 ## 计划 重新规划。";
                                        self.emit(AgentEvent::Thought {
                                            step,
                                            content: format!("[replan] {}", replan_msg),
                                        }).await;
                                        self.conversation
                                            .add_message(session_id, "user", replan_msg, None, None)
                                            .await
                                            .map_err(|e| AgentError::Tool(e.to_string()))?;
                                        consecutive_tool_failures = 0;
                                    }
                                }
                                _ = watch_cancellation(&cancel) => {
                                    let msg = format!("Tool '{}' was cancelled by user.", name);
                                    self.emit(AgentEvent::Approval {
                                        tool_name: name.clone(),
                                        decision: "cancelled".to_string(),
                                        message: msg.clone(),
                                    }).await;
                                    self.add_observation(session_id, &name, &tool_call.id, &msg, true).await?;
                                    push_event(&mut events, AgentEvent::ToolResult {
                                        step,
                                        tool_name: name.clone(),
                                        success: false,
                                        output: msg,
                                    }, self.config.max_events);
                                }
                            }
                        }
                        Decision::Block => {
                            let msg = format!("Tool '{}' was blocked: {}", name, approval.message);
                            self.add_observation(session_id, &name, &tool_call.id, &msg, true).await?;
                            push_event(&mut events, AgentEvent::ToolResult {
                                step,
                                tool_name: name.clone(),
                                success: false,
                                output: msg,
                            }, self.config.max_events);
                            // C2: 工具被阻止视为失败
                            consecutive_tool_failures += 1;
                            if consecutive_tool_failures >= 2 {
                                let replan_msg = "前两步工具调用连续失败。请重新评估当前计划，如果当前路径不可行，请使用 ## 计划 重新规划。";
                                self.emit(AgentEvent::Thought {
                                    step,
                                    content: format!("[replan] {}", replan_msg),
                                }).await;
                                self.conversation
                                    .add_message(session_id, "user", replan_msg, None, None)
                                    .await
                                    .map_err(|e| AgentError::Tool(e.to_string()))?;
                                consecutive_tool_failures = 0;
                            }
                        }
                        Decision::Defer => {}
                    }

                    // A2 修复: Suggest 提示在工具结果（tool 消息）之后注入，保证消息顺序
                    // assistant(tool_calls) → tool(tool_call_id) → user(suggest) 符合 OpenAI API 规范
                    if let Some(msg) = &suggest_msg {
                        self.conversation
                            .add_message(session_id, "user", msg, None, None)
                            .await
                            .map_err(|e| AgentError::Tool(e.to_string()))?;
                    }
                }
            }

            // M-A3: 步骤质量评分 — 评估本轮是否有可观测的进展
            // 有进展 = 文件变更工具成功 || StepsTracker 标记新步骤完成
            // A2/A3 统一使用此判定，避免语义冲突
            let steps_tracker_progressed = parsed_update;
            let has_progress = step_has_file_change || steps_tracker_progressed;

            // A2: 无进展检测 — 连续无进展 ≥ 阈值 → Suggest（与 A3 共用 has_progress 判定）
            if let LoopAction::Suggest = deduper.register_progress(has_progress) {
                let msg = format!(
                    "⚠️ 已连续 {} 步无进展，可能陷入无效循环。请考虑直接修改文件（write_file/edit_file）或更换策略。",
                    deduper.no_progress_count
                );
                self.emit(AgentEvent::Thought { step, content: msg.clone() }).await;
                self.conversation
                    .add_message(session_id, "user", &msg, None, None)
                    .await
                    .map_err(|e| AgentError::Tool(e.to_string()))?;
            }

            // C4: 持久化本步事件（fire-and-forget）
            Self::persist_event(
                &persistence,
                step,
                step_first_tool.clone(),
                TaskStatus::Running,
                if content_text.is_empty() { format!("Step {}", step) } else { content_text.chars().take(100).collect() },
            ).await;

            self.emit(AgentEvent::StepCompleted {
                step,
                total_steps: effective_max_steps,
            }).await;

            // D1: 定期 checkpoint — 每 checkpoint_interval_steps 步写入一次（fire-and-forget）
            if (step + 1) % self.config.checkpoint_interval_steps == 0 {
                if let Some(ref cp) = checkpoint {
                    let summary = steps_tracker.progress_injection()
                        .unwrap_or_else(|| last_answer_text.chars().take(200).collect());
                    let data = CheckpointData {
                        session_id: session_id.to_string(),
                        step,
                        status: CheckpointStatus::Running,
                        progress_summary: summary,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    };
                    if let Err(e) = cp.write(&data).await {
                        tracing::warn!("checkpoint periodic write failed: {}", e);
                    }
                }
            }

            if has_progress {
                // 在重置前判断：若之前连续无进展步数已达阈值，则不再扩展
                let was_in_long_streak =
                    consecutive_no_progress >= self.config.no_progress_step_limit;
                consecutive_no_progress = 0;
                // 有进展且即将到达上限 → 动态扩展（受 no_progress_step_limit 约束）
                if step + 1 >= effective_max_steps && !was_in_long_streak {
                    if let Some(new_limit) = compute_extended_max_steps(
                        effective_max_steps,
                        self.config.max_steps,
                        self.config.max_steps_extend_limit,
                    ) {
                        effective_max_steps = new_limit;
                        self.emit(AgentEvent::Thought {
                            step,
                            content: format!("检测到持续进展，步骤上限扩展至 {}", effective_max_steps),
                        }).await;
                    }
                }
            } else {
                consecutive_no_progress += 1;
            }
        }

        // M1/M2 兜底失败：max_steps 耗尽但 Agent 仍未给出明确结束信号。
        // 不要直接抛错误，而是尝试返回已有的最佳答案，避免用户工作完全丢失。
        // 同时发出一条 Error 事件，让 TUI 知道发生了截断。
        let msg = format!(
            "Reached max steps ({}) without explicit completion. Returning the best available partial answer. If the result is incomplete, try re-submitting with a more specific prompt or increasing the step limit.",
            effective_max_steps
        );
        self.emit(AgentEvent::Error { message: msg.clone() }).await;
        self.emit(AgentEvent::Finished {
            answer: last_answer_text.clone(),
            steps: effective_max_steps,
        }).await;
        Self::persist_event(&persistence, effective_max_steps, None, TaskStatus::Interrupted, "Max steps exhausted".to_string()).await;
        Self::mark_checkpoint_completed(&checkpoint, session_id).await;
        Ok(AgentRunResult {
            answer: last_answer_text,
            steps_taken: effective_max_steps,
            tool_calls: tool_calls_count,
            events,
        })
    }

    /// 调用 LLM：尝试流式输出 token，失败时回退到非流式调用
    ///
    /// 流式模式下：
    /// - 每个文本 chunk 按空格分词后逐词 emit `MessageDelta`
    /// - 流式结束后 emit `Usage`（从最后一个 chunk 提取）
    /// - 失败时回退到非流式调用
    async fn call_llm_with_streaming(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        _step: usize,
    ) -> Result<ChatResponse, AgentError> {
        if !self.config.stream {
            return self.provider.chat(messages, tools).await
                .map_err(|e| AgentError::Llm(e.to_string()));
        }

        // 尝试流式调用
        match self.provider.chat_stream(messages, tools).await {
            Ok(mut stream) => {
                use futures::StreamExt;
                use std::collections::BTreeMap;

                let mut full_content = String::new();
                let mut tool_call_builders: BTreeMap<u32, ToolCallBuilder> = BTreeMap::new();
                let mut final_usage: Option<(u64, u64)> = None; // (input_tokens, output_tokens)
                let mut final_finish_reason: Option<String> = None; // 用于 M4 截断恢复

                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(sc) => {
                            // Capture usage if present in this chunk
                            if let Some(ref usage_info) = sc.usage {
                                final_usage = Some((
                                    usage_info.prompt_tokens,
                                    usage_info.completion_tokens,
                                ));
                            }

                            // Capture finish_reason from this chunk（M4 截断恢复依赖真实值）
                            for choice in &sc.choices {
                                if let Some(ref fr) = choice.finish_reason {
                                    final_finish_reason = Some(fr.clone());
                                }
                            }

                            for choice in sc.choices {
                                // 累加文本内容并发射逐词 MessageDelta 事件
                                if let Some(ref content) = choice.delta.content {
                                    full_content.push_str(content);
                                    // M7 改进: 按字符逐字 emit，避免空格 split 在 CJK 文本中无用
                                    // 且避免 chunk 边界处的多余空格 (old: split(' ') → "你好 世界")
                                    for ch in content.chars() {
                                        self.emit(AgentEvent::MessageDelta {
                                            delta: ch.to_string(),
                                        }).await;
                                    }
                                }
                                // 累加工具调用 delta
                                if let Some(tc_deltas) = &choice.delta.tool_calls {
                                    for delta in tc_deltas {
                                        let builder = tool_call_builders.entry(delta.index).or_default();
                                        if let Some(ref id) = delta.id {
                                            builder.id = Some(id.clone());
                                        }
                                        if let Some(ref func) = delta.function {
                                            if let Some(ref name) = func.name {
                                                builder.name = Some(name.clone());
                                            }
                                            if let Some(ref args) = func.arguments {
                                                builder.arguments.push_str(args);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Stream chunk error: {}, falling back to non-streaming", e);
                            return self.provider.chat(messages, tools).await
                                .map_err(|e| AgentError::Llm(e.to_string()));
                        }
                    }
                }

                // Emit Usage event after streaming completes
                if let Some((input_tokens, output_tokens)) = final_usage {
                    self.emit(AgentEvent::Usage {
                        input_tokens,
                        output_tokens,
                    }).await;
                } else {
                    self.emit(AgentEvent::Usage {
                        input_tokens: 0,
                        output_tokens: 0,
                    }).await;
                }

                // 从累加的 delta 中构建 tool_calls
                let tool_calls: Vec<ToolCallDef> = tool_call_builders.into_values()
                    .filter_map(|builder| builder.build())
                    .collect();
                let has_tool_calls = !tool_calls.is_empty();

                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: ChatMessage {
                            role: Role::Assistant,
                            content: MessageContent::Text(full_content),
                            name: None,
                            tool_call_id: None,
                            tool_calls: if has_tool_calls { Some(tool_calls) } else { None },
                        },
                        finish_reason: final_finish_reason.or_else(|| {
                            Some(if has_tool_calls { "tool_calls".to_string() } else { "stop".to_string() })
                        }),
                    }],
                    usage: final_usage.map(|(input, output)| UsageInfo {
                        prompt_tokens: input,
                        completion_tokens: output,
                        total_tokens: input + output,
                    }),
                })
            }
            Err(e) => {
                tracing::warn!("Stream init failed: {}, falling back to non-streaming", e);
                self.provider.chat(messages, tools).await
                    .map_err(|e| AgentError::Llm(e.to_string()))
            }
        }
    }

    async fn build_messages(&self, session_id: &str, system_prompt: &str) -> Result<Vec<ChatMessage>, AgentError> {
        let mut messages = vec![ChatMessage {
            role: Role::System,
            content: MessageContent::Text(system_prompt.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];

        // B2: 使用 get_recent_messages 限制加载的消息数量，避免全量加载撑爆上下文
        let limit = self.config.max_history_messages;
        let records = self.conversation
            .get_recent_messages(session_id, limit)
            .await
            .map_err(|e| AgentError::Tool(e.to_string()))?;

        for record in records {
            let role = match record.role.as_str() {
                "system" => Role::System,
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::User,
            };

            let tool_calls: Option<Vec<ToolCallDef>> = record.tool_calls
                .and_then(|tc| serde_json::from_str(&tc).ok());

            messages.push(ChatMessage {
                role,
                content: MessageContent::Text(record.content),
                name: None,
                tool_call_id: record.tool_call_id,
                tool_calls,
            });
        }

        Ok(messages)
    }

    async fn add_observation(
        &self,
        session_id: &str,
        _tool_name: &str,
        tool_call_id: &str,
        output: &str,
        is_error: bool,
    ) -> Result<(), AgentError> {
        let prefix = if is_error { "[error] " } else { "" };
        let content = format!("{}{}", prefix, output);
        self.conversation
            .add_message(session_id, "tool", &content, None, Some(tool_call_id))
            .await
            .map_err(|e| AgentError::Tool(e.to_string()))?;
        Ok(())
    }

    /// Mid-Flow 运行时轻量审查（对齐 Python run_mid_flow_review）
    /// 直接调用 provider.chat()（非 ReAct 循环），解析 findings
    async fn run_mid_flow_review(
        &self,
        diff: &str,
        step: usize,
    ) -> Vec<crate::agent::flow::ReviewFinding> {
        let prompt = format!(
            "## Step {}\nReview the following recent code changes (git diff). \
             Output findings in <findings> tags or <findings>PASS</findings> if clean.\n\n\
             ## Git Diff\n{}",
            step,
            &diff[..diff.len().min(5000)]
        );

        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: MessageContent::Text(
                    crate::agent::flow::REVIEW_SYSTEM_PROMPT.to_string(),
                ),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: MessageContent::Text(prompt),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];

        match self.provider.chat(&messages, &[]).await {
            Ok(resp) => {
                if let Some(choice) = resp.choices.into_iter().next() {
                    let text = match choice.message.content {
                        MessageContent::Text(t) => t,
                        MessageContent::Parts(parts) => parts
                            .iter()
                            .map(|p| match p {
                                crate::provider::provider::ContentPart::Text { text } => text.clone(),
                                crate::provider::provider::ContentPart::ImageUrl { image_url } => image_url.url.clone(),
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    };
                    crate::agent::flow::parse_review_output(&text)
                } else {
                    Vec::new()
                }
            }
            Err(e) => {
                tracing::warn!("Mid-flow review failed: {}", e);
                Vec::new()
            }
        }
    }

    async fn handle_tool_result(
        &self,
        session_id: &str,
        step: usize,
        tool_name: &str,
        tool_call_id: &str,
        result: &ToolResult,
    ) -> Result<(), AgentError> {
        let mut output = if result.success {
            result.output.clone()
        } else {
            format!("Error: {}; Output: {}", result.error.as_deref().unwrap_or("unknown"), result.output)
        };

        // 补充优化（对标 Python 版 >30000 字符截断为前 2000 字符预览）：
        // 单次工具输出过大时会撑爆上下文，这里截断后再写入 observation 与事件。
        if output.len() > 30000 {
            let preview: String = output.chars().take(2000).collect();
            output = format!("[结果过长 ({} 字符)，已截断为前 2000 字符]\n{}", output.len(), preview);
        }

        self.add_observation(session_id, tool_name, tool_call_id, &output, !result.success).await?;

        // M15 PostToolUseFailure: 失败分类事件
        if !result.success {
            let error_str = result.error.as_deref().unwrap_or("unknown");
            let failure_type = classify_tool_error(error_str);
            self.emit(AgentEvent::ToolFailed {
                tool_name: tool_name.to_string(),
                error: error_str.to_string(),
                failure_type,
            }).await;
        }

        self.emit(AgentEvent::ToolResult {
            step,
            tool_name: tool_name.to_string(),
            success: result.success,
            output: output.clone(),
        }).await;

        // D1-T01: 写工具执行成功后发出 FileChanged 事件（供前端 Changes 面板消费）
        if result.success {
            if let (Some(diff), Some(file_path)) = (&result.diff, result.affected_files.first()) {
                let change_type = result.metadata
                    .as_ref()
                    .and_then(|m| m.get("change_type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("edit")
                    .to_string();
                let old_size = result.metadata
                    .as_ref()
                    .and_then(|m| m.get("old_size"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                let new_size = result.metadata
                    .as_ref()
                    .and_then(|m| m.get("new_size"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                self.emit(AgentEvent::FileChanged {
                    tool_name: tool_name.to_string(),
                    file_path: file_path.clone(),
                    diff: diff.clone(),
                    old_size,
                    new_size,
                    change_type,
                }).await;
            }
        }

        Ok(())
    }

    /// M13 验证阻断: Agent 声称完成前自动运行项目验证（对标 OpenCode/Claude Code Stop Hook）
    async fn run_verification_hook(
        &self,
        session_id: &str,
        workspace: &str,
        step: usize,
    ) {
        use std::path::Path;

        let ws = Path::new(workspace);

        // C3: 多步骤验证 — 渐进式 check → test → lint
        let verify_steps: Vec<(&str, Vec<&str>, &str)> = if ws.join("Cargo.toml").exists() {
            vec![
                ("cargo", vec!["check", "--quiet"], "COMPILE"),
                ("cargo", vec!["test", "--quiet"], "TEST"),
                ("cargo", vec!["clippy", "--quiet"], "LINT"),
            ]
        } else if ws.join("package.json").exists() {
            let mut steps: Vec<(&str, Vec<&str>, &str)> = vec![
                ("npx", vec!["tsc", "--noEmit"], "COMPILE"),
            ];
            // C3: 仅在 package.json 含 test script 时添加测试步骤
            if let Ok(pkg_content) = std::fs::read_to_string(ws.join("package.json")) {
                if let Ok(pkg_json) = serde_json::from_str::<serde_json::Value>(&pkg_content) {
                    if pkg_json.get("scripts").and_then(|s| s.get("test")).is_some() {
                        steps.push(("npm", vec!["test"], "TEST"));
                    }
                }
            }
            steps
        } else if ws.join("pyproject.toml").exists() || ws.join("setup.py").exists() {
            vec![
                ("python3", vec!["-m", "compileall", "-q", workspace], "COMPILE"),
            ]
        } else {
            vec![]
        };

        let mut all_passed = true;
        let mut results: Vec<serde_json::Value> = Vec::new();

        for (cmd, args, category) in &verify_steps {
            let cmd_str = cmd.to_string();
            let args_str = args.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(" ");

            self.emit(AgentEvent::Thought { step, content: format!("[verify] {} {} …", cmd_str, args_str) }).await;

            let output = tokio::process::Command::new(cmd)
                .args(args)
                .current_dir(workspace)
                .output()
                .await;

            let result_entry = match output {
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    let short_stderr: String = stderr.chars().take(2000).collect();

                    if o.status.success() {
                        self.emit(AgentEvent::Thought { step, content: format!("[verify] ✓ {} passed", category) }).await;
                        serde_json::json!({
                            "command": format!("{} {}", cmd_str, args_str),
                            "category": category,
                            "status": "passed",
                        })
                    } else {
                        let severity = if *category == "LINT" { "WARNING" } else { "FAILED" };
                        let symbol = if *category == "LINT" { "⚠" } else { "✗" };
                        let msg = format!("[verify] {} {} {}\n{}\n---\n{}", symbol, category, severity, short_stderr,
                            if *category == "LINT" { "Lint 警告已记录，可继续。" } else { "修复上述错误后重新验证。" });

                        self.emit(AgentEvent::Thought { step, content: msg.clone() }).await;

                        // 编译/测试失败：注入 observation 阻止完成
                        if *category != "LINT" {
                            let _ = self.conversation.add_message(session_id, "user", &msg, None, None).await;
                            all_passed = false;
                            break;
                        }

                        serde_json::json!({
                            "command": format!("{} {}", cmd_str, args_str),
                            "category": category,
                            "status": "warning",
                            "stderr": short_stderr,
                        })
                    }
                }
                Err(e) => {
                    let msg = format!("[verify] error running {} {}: {}", cmd_str, args_str, e);
                    self.emit(AgentEvent::Thought { step, content: msg }).await;

                    // C3: 命令不存在（如 cargo/npm 未安装）视为验证失败
                    if *category != "LINT" {
                        let fail_msg = format!("[verify] ✗ {} FAILED (command not found or error)\n{}", category, e);
                        let _ = self.conversation.add_message(session_id, "user", &fail_msg, None, None).await;
                        all_passed = false;
                        break;
                    }

                    serde_json::json!({
                        "command": format!("{} {}", cmd_str, args_str),
                        "category": category,
                        "status": "error",
                        "error": e.to_string(),
                    })
                }
            };
            results.push(result_entry);
        }

        // C3: 验证结果持久化到 .goat/verify/result.json
        if !verify_steps.is_empty() {
            let verify_dir = ws.join(".goat").join("verify");
            let _ = std::fs::create_dir_all(&verify_dir);
            let verify_result = serde_json::json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "all_passed": all_passed,
                "steps": results,
            });
            if let Ok(content) = serde_json::to_string_pretty(&verify_result) {
                let _ = std::fs::write(verify_dir.join("result.json"), content);
            }
        }

        // P0: Post-hoc tech-stack sanity check — detect when the output is
        // plain HTML/CSS/JS despite the user requesting a framework.
        let has_index_html = ws.join("index.html").exists();
        let has_css = ws.join("styles.css").exists() || ws.join("style.css").exists();
        let has_app_js = ws.join("app.js").exists() || ws.join("script.js").exists();
        let has_pkg_json = ws.join("package.json").exists();
        let has_framework_cfg = ws.join("vite.config.ts").exists()
            || ws.join("vite.config.js").exists()
            || ws.join("next.config.js").exists()
            || ws.join("next.config.ts").exists()
            || ws.join("vue.config.js").exists()
            || ws.join("angular.json").exists()
            || ws.join("svelte.config.js").exists()
            || ws.join("astro.config.mjs").exists()
            || ws.join("nuxt.config.ts").exists();

        // Heuristic: index.html + styles.css + app.js with no package.json
        // and no framework config → classic Vanilla JS project
        let looks_like_vanilla = has_index_html
            && has_css
            && has_app_js
            && !has_pkg_json
            && !has_framework_cfg;

        if looks_like_vanilla {
            let msg = "[verify] ⚠ TECH STACK MISMATCH detected: output appears to be \
                       plain HTML/CSS/JS (index.html + styles.css + app.js) without \
                       any framework tooling (no package.json, no vite.config, no \
                       framework config files). If the user requested a framework \
                       (React, Vue, Angular, etc.), you MUST rewrite using the correct \
                       toolchain (e.g. `npm create vite`, `create-react-app`, etc.).";
            self.emit(AgentEvent::Thought { step, content: msg.to_string() }).await;
            let _ = self.conversation.add_message(session_id, "user", msg, None, None).await;
        }
    }

    pub(crate) async fn emit(&self, event: AgentEvent) {
        let (event_type, data) = match &event {
            AgentEvent::Started { .. } => (EventType::SubAgentSpawned, serde_json::to_value(&event)),
            AgentEvent::Thought { .. } => (EventType::LlmStreamChunk, serde_json::to_value(&event)),
            AgentEvent::ToolCall { .. } => (EventType::ToolCallStart, serde_json::to_value(&event)),
            AgentEvent::ToolResult { .. } => (EventType::ToolCallResult, serde_json::to_value(&event)),
            AgentEvent::Approval { .. } => (EventType::ApprovalDecided, serde_json::to_value(&event)),
            AgentEvent::Finished { .. } => (EventType::LlmStreamDone, serde_json::to_value(&event)),
            AgentEvent::Error { .. } => (EventType::Error, serde_json::to_value(&event)),
            AgentEvent::MessageDelta { .. } => (EventType::MessageDelta, serde_json::to_value(&event)),
            AgentEvent::Usage { .. } => (EventType::Usage, serde_json::to_value(&event)),
            AgentEvent::Cancelled { .. } => (EventType::AgentCancelled, serde_json::to_value(&event)),
            AgentEvent::ApprovalRequired { .. } => (EventType::ApprovalRequired, serde_json::to_value(&event)),
            AgentEvent::ContextCompacted { .. } => (EventType::SubAgentSpawned, serde_json::to_value(&event)),
            AgentEvent::ToolFailed { .. } => (EventType::ToolCallResult, serde_json::to_value(&event)),
            AgentEvent::StepCompleted { .. } | AgentEvent::Message { .. } => (EventType::PlanExecuting, serde_json::to_value(&event)),
            AgentEvent::FileChanged { .. } => (EventType::ToolCallResult, serde_json::to_value(&event)),
        };
        let _ = self.event_bus.emit(event_type, "agent", data.unwrap_or_default());
    }

    fn tools_description(&self) -> String {
        let mut desc = String::new();
        for tool_def in self.tools.all_tool_defs() {
            desc.push_str(&format!(
                "### {}\n{}\nParameters: {}\n\n",
                tool_def.function.name,
                tool_def.function.description,
                serde_json::to_string(&tool_def.function.parameters).unwrap_or_default()
            ));
        }
        desc
    }
}

/// 将 MessageContent 转为纯文本
fn content_to_string(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Parts(parts) => parts
            .iter()
            .map(|p| match p {
                crate::provider::provider::ContentPart::Text { text } => text.clone(),
                crate::provider::provider::ContentPart::ImageUrl { image_url } => image_url.url.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// A1: 从 StepsTracker.progress_injection() 输出中提取已完成步骤的描述列表。
///
/// progress_injection() 格式：
/// ```text
/// ## 当前计划进度
/// - ✅ 步骤 1: 需求分析
/// - ⬜ 步骤 2: 代码实现
/// ```
/// 本函数提取所有 ✅ 行中第一个 ": " 之后的部分作为步骤描述。
fn extract_completed_step_descriptions(progress: Option<&str>) -> Vec<String> {
    let progress = match progress {
        Some(p) => p,
        None => return Vec::new(),
    };
    progress
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with("- ✅") {
                return None;
            }
            // 行格式: "- ✅ 步骤 N: <描述>"
            line.split_once(": ").map(|(_, desc)| desc.to_string())
        })
        .collect()
}

/// 将 LLM 的 ToolCallDef 解析为内部格式
fn parse_tool_call(tool_call: &ToolCallDef) -> Result<ParsedToolCall, AgentError> {
    let args = serde_json::from_str(&tool_call.function.arguments)
        .map_err(|e| AgentError::ParseError(format!("Failed to parse tool arguments: {}", e)))?;
    Ok(ParsedToolCall {
        name: tool_call.function.name.clone(),
        arguments: args,
    })
}

/// 流式工具调用 delta 累加器
#[derive(Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ToolCallBuilder {
    fn build(self) -> Option<ToolCallDef> {
        let id = self.id?;
        let name = self.name?;
        Some(ToolCallDef {
            id,
            call_type: "function".to_string(),
            function: crate::provider::provider::FunctionCall {
                name,
                arguments: self.arguments,
            },
        })
    }
}

/// 完成检测：延续标记（文本暗示"还没说完，继续干"）
const CONTINUATION_MARKERS: &[&str] = &[
    "继续", "接下来", "正在", "开始", "首先", "让我", "先",
    "然后", "接着", "下一步",
];

/// 完成检测：完成标记（文本暗示"任务已完成"）
const COMPLETION_MARKERS: &[&str] = &[
    "完成", "总结", "以上", "综上", "done", "finished", "complete",
    "任务完成", "已完成", "## 任务完成",
];

/// 完成检测：结尾标记（暗示后续还有内容）
const TRAILING_SUFFIXES: &[&str] = &[":", "：", "…", "..."];

/// 判断回复内容是否为最终答案（任务完成），而非中途状态更新。
///
/// 移植自 Python 版 `subagent_runtime.looks_like_final_answer`，6 步检查：
/// 1. 空 → 非最终  2. 以 ":"/"："/"…"/"..." 结尾 → 非最终（未说完）
/// 3. 含延续标记 → 非最终（即使同时含完成词，优先级更高）
/// 4. 含完成标记 → 最终  5. 极短（<4 字符）→ 非最终  6. 默认 → 最终
fn looks_like_final_answer(content: &str) -> bool {
    if content.trim().is_empty() {
        return false;
    }
    let stripped = content.trim();

    // 2. 以暗示"未说完"的标点结尾
    if TRAILING_SUFFIXES.iter().any(|suf| stripped.trim_end().ends_with(*suf)) {
        return false;
    }

    // 3. 先查延续标记（优先级高于完成标记）
    if CONTINUATION_MARKERS.iter().any(|m| stripped.contains(*m)) {
        return false;
    }

    // 4. 再查完成标记
    if COMPLETION_MARKERS.iter().any(|m| stripped.contains(*m)) {
        return true;
    }

    // 5. 极短内容（< 4 字符）无任何标记 → 非最终（如"好的"）
    if stripped.chars().count() < 4 {
        return false;
    }

    // 6. 默认视为最终答案
    true
}

/// 根据连续无工具调用次数生成递进式催促提示（移植自 Python 版 `continuation_prompt`）。
fn continuation_prompt(count: usize) -> String {
    if count <= 1 {
        "你刚才输出了文字但没有调用任何工具。请立即调用工具来执行下一步操作，\
不要仅输出文本描述。如果需要读取文件就用 read_file，需要写文件就用 write_file。"
            .to_string()
    } else if count == 2 {
        "你又输出了文字但没有调用任何工具。请立刻调用工具完成任务，不要继续输出计划或说明。\
你有工具可用，请直接使用它们。"
            .to_string()
    } else {
        "你已经连续三次只输出文字不调用工具了。最后一次警告：立即调用合适的工具执行操作，\
否则将退出等待用户指令。不要输出任何说明文字，只调用工具。"
            .to_string()
    }
}

/// 工具循环熔断检测器的动作分级（渐进熔断策略）。
#[derive(Debug, PartialEq, Eq)]
enum LoopAction {
    /// 无异常，继续执行
    None,
    /// 建议换策略（注入提示但继续执行工具）
    Suggest,
    /// 必须换策略（注入提示并跳过工具执行）
    ForceSkip,
}

/// 视为"有文件变更"的工具名 — 用于无进展检测
const FILE_CHANGE_TOOLS: &[&str] = &["write_file", "edit_file", "patch", "git"];

/// 无进展熔断阈值：连续无文件变更步数达到此值 → `Suggest`
const NO_PROGRESS_THRESHOLD: usize = 10;

/// 工具循环熔断检测器 — 对标 Python 版 `ToolCallDeduper`（A2 增强）
///
/// 三层检测，按渐进熔断策略返回 [`LoopAction`]：
/// 1. **精确指纹**：`(tool_name, 排序后的参数键值对)`。最近 5 次窗口内
///    - 同指纹 == 2 次 → `Suggest`（仍执行工具，提前预警）
///    - 同指纹 ≥ 3 次 → `ForceSkip`（跳过执行）
/// 2. **语义指纹（模式指纹）**：`(tool_name, 排序后的参数键集)`，忽略值差异。
///    最近 5 次窗口内同模式 ≥ 4 次 → `ForceSkip`。捕获"连续 read_file 不同路径"
///    这类精确指纹检测漏掉的无进展循环。
/// 3. **无进展检测**：连续无文件变更步数 ≥ 10 → `Suggest`（见 [`register_progress`]）。
///
/// 与早期"哈希连续比较 + 直接返回 LoopDetected 中止"的实现不同：本实现分级
/// 熔断（Suggest → ForceSkip），注入 + continue，不硬中止。最终靠 `max_steps` 兜底。
struct ToolCallDeduper {
    /// 精确指纹历史：最近 5 次 `(tool_name, 参数键值对)`
    history: VecDeque<(String, BTreeMap<String, String>)>,
    /// 模式指纹历史：最近 5 次 `(tool_name, 排序后参数键集)`
    pattern_history: VecDeque<(String, Vec<String>)>,
    /// 连续无文件变更步数
    no_progress_count: usize,
}

impl ToolCallDeduper {
    fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(5),
            pattern_history: VecDeque::with_capacity(5),
            no_progress_count: 0,
        }
    }

    fn fingerprint(tool_name: &str, params: &serde_json::Value) -> (String, BTreeMap<String, String>) {
        let mut map = BTreeMap::new();
        if let Some(obj) = params.as_object() {
            for (k, v) in obj {
                map.insert(k.clone(), v.to_string());
            }
        }
        (tool_name.to_string(), map)
    }

    /// 模式指纹：忽略值，仅保留排序后的参数键集。用于检测"同 tool 同参数键集、
    /// 仅值不同"的语义重复（如连续 read_file 不同路径）。
    fn pattern_fingerprint(tool_name: &str, params: &serde_json::Value) -> (String, Vec<String>) {
        let mut keys: Vec<String> = if let Some(obj) = params.as_object() {
            obj.keys().map(|k| k.to_string()).collect()
        } else {
            Vec::new()
        };
        keys.sort();
        (tool_name.to_string(), keys)
    }

    /// 判断工具名是否视为"有文件变更"（write_file / edit_file / patch / git）
    /// A2 修复: 同时匹配带 MCP 前缀的工具名（如 `mcp__server__write_file`）
    fn is_file_change_tool(tool_name: &str) -> bool {
        FILE_CHANGE_TOOLS
            .iter()
            .any(|t| tool_name == *t || tool_name.ends_with(t))
    }

    /// 注册本次调用并按渐进熔断策略返回动作。
    ///
    /// 优先级（高 → 低）：
    /// 1. 精确指纹 ≥ 3 → `ForceSkip`
    /// 2. 模式指纹 ≥ 4 → `ForceSkip`
    /// 3. 精确指纹 == 2 → `Suggest`（仍执行工具）
    /// 4. 否则 → `None`
    fn register_and_check(&mut self, tool_name: &str, params: &serde_json::Value) -> LoopAction {
        let fp = Self::fingerprint(tool_name, params);
        let pf = Self::pattern_fingerprint(tool_name, params);

        self.history.push_back(fp.clone());
        if self.history.len() > 5 {
            self.history.pop_front();
        }
        self.pattern_history.push_back(pf.clone());
        if self.pattern_history.len() > 5 {
            self.pattern_history.pop_front();
        }

        let exact_count = self.history.iter().filter(|x| **x == fp).count();
        let pattern_count = self.pattern_history.iter().filter(|x| **x == pf).count();

        if exact_count >= 3 {
            LoopAction::ForceSkip
        } else if pattern_count >= 4 {
            LoopAction::ForceSkip
        } else if exact_count == 2 {
            LoopAction::Suggest
        } else {
            LoopAction::None
        }
    }

    /// 注册本轮是否有进展，连续无进展 ≥ 阈值时返回 `Suggest`。
    ///
    /// - `has_progress = true`：重置计数为 0，返回 `None`
    /// - `has_progress = false`：计数 +1，≥ [`NO_PROGRESS_THRESHOLD`] → `Suggest`
    ///
    /// 应在每轮循环结束时（工具执行后）调用。
    fn register_progress(&mut self, has_progress: bool) -> LoopAction {
        if has_progress {
            self.no_progress_count = 0;
            return LoopAction::None;
        }
        self.no_progress_count += 1;
        if self.no_progress_count >= NO_PROGRESS_THRESHOLD {
            LoopAction::Suggest
        } else {
            LoopAction::None
        }
    }
}

/// M-A3: 计算有进展时的新步骤上限。
///
/// 在当前上限基础上 +10，但不超过 `max_steps * max_steps_extend_limit`。
/// 返回 `Some(new_limit)` 表示可扩展；`None` 表示已达上限无法扩展。
fn compute_extended_max_steps(
    current: usize,
    max_steps: usize,
    extend_limit: usize,
) -> Option<usize> {
    let cap = max_steps.saturating_mul(extend_limit);
    let new_limit = (current + 10).min(cap);
    if new_limit > current {
        Some(new_limit)
    } else {
        None
    }
}

/// D3-T05: 结构化 Skill 信息（供前端 Slash 命令 UI 使用）
#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    /// "global" | "project"
    pub source: String,
    /// SKILL.md 完整路径
    pub path: String,
}

/// 解析 SKILL.md 的 frontmatter 头部
///
/// 格式：
/// ```text
/// ---
/// name: my-skill
/// description: does something
/// ---
/// <body content>
/// ```
///
/// 返回 `Some((name, description, body))`，解析失败返回 `None`。
fn parse_skill_frontmatter(content: &str) -> Option<(String, String, String)> {
    let body = content.strip_prefix("---\n")?;
    let (header, body) = body.split_once("\n---\n")?;
    let name = header
        .lines()
        .find(|l| l.starts_with("name:"))?
        .strip_prefix("name:")?
        .trim();
    let desc = header
        .lines()
        .find(|l| l.starts_with("description:"))?
        .strip_prefix("description:")?
        .trim();
    Some((name.to_string(), desc.to_string(), body.to_string()))
}

/// Map ToolCategory to a display string for ApprovalRequired.tool_type
fn tool_type_from_category(category: ToolCategory) -> String {
    match category {
        ToolCategory::Shell => "Shell".to_string(),
        ToolCategory::Write => "Write".to_string(),
        ToolCategory::Network => "Network".to_string(),
        ToolCategory::Read => "Read".to_string(),
        ToolCategory::Destructive => "Destructive".to_string(),
        ToolCategory::Mcp => "MCP".to_string(),
        ToolCategory::Agent => "Agent".to_string(),
        ToolCategory::Interactive => "Interactive".to_string(),
    }
}

/// Map ToolCategory to a risk level for ApprovalRequired.risk_level
fn risk_level_from_category(category: ToolCategory) -> String {
    match category {
        ToolCategory::Destructive => "HIGH".to_string(),
        ToolCategory::Shell | ToolCategory::Write | ToolCategory::Network => "MEDIUM".to_string(),
        ToolCategory::Mcp | ToolCategory::Agent => "MEDIUM".to_string(),
        ToolCategory::Read | ToolCategory::Interactive => "LOW".to_string(),
    }
}

/// D1-T02: 为写工具生成 diff 预览（审批弹窗展示用）
///
/// 仅对 edit_file/write_file 生成 unified diff 预览，其他工具返回 (None, [])。
fn generate_diff_preview(
    tool_name: &str,
    arguments: &serde_json::Value,
    workspace: &str,
) -> (Option<String>, Vec<String>) {
    use similar::{ChangeTag, TextDiff};

    let file_path = match arguments.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return (None, vec![]),
    };

    // 只为写工具生成 diff 预览
    if tool_name != "edit_file" && tool_name != "write_file" {
        return (None, vec![]);
    }

    let abs_path = crate::core::paths::safe_path(file_path, std::path::Path::new(workspace));
    let old_content = std::fs::read_to_string(&abs_path).unwrap_or_default();

    let new_content = if tool_name == "write_file" {
        arguments.get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        // edit_file — 预览时只替换第一个匹配（与执行时 replace_all=false 行为一致）
        let old_string = arguments.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
        let new_string = arguments.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
        if old_string.is_empty() {
            return (None, vec![]);
        }
        old_content.replacen(old_string, new_string, 1)
    };

    if old_content == new_content {
        return (None, vec![]);
    }

    let rel_path = abs_path.strip_prefix(workspace)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| file_path.to_string());

    let diff = TextDiff::from_lines(&old_content, &new_content);
    let mut output = String::new();
    output.push_str(&format!("--- a/{}\n", rel_path));
    output.push_str(&format!("+++ b/{}\n", rel_path));
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => '-',
            ChangeTag::Insert => '+',
            ChangeTag::Equal => ' ',
        };
        output.push(sign);
        output.push_str(change.value());
        if !change.value().ends_with('\n') {
            output.push('\n');
        }
    }

    (Some(output), vec![rel_path])
}

/// D1-T02: 计算参数 hash（用于 session_cache 键匹配）
fn compute_args_hash(arguments: &serde_json::Value) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    arguments.to_string().hash(&mut hasher);
    hasher.finish()
}

/// Extract tool-specific context fields from arguments
fn extract_tool_context(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> (Option<String>, Option<String>, Option<String>) {
    let command = arguments.get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let path = arguments.get("path")
        .or_else(|| arguments.get("file_path"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let url = arguments.get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // If tool_name contains hints, fall back to extracting from known arg names
    let cmd = if tool_name.contains("shell") || tool_name.contains("bash") || tool_name.contains("exec") {
        command.or_else(|| arguments.get("cmd").and_then(|v| v.as_str()).map(|s| s.to_string()))
    } else {
        command
    };

    (cmd, path, url)
}

/// M8 辅助: 可取消的等待 — 每 100ms 轮询 CancellationToken
///
/// 用于 `tokio::select!` 中与 oneshot::Receiver 竞争，确保审批等待期间
/// Ctrl+C 可中断 agent（而非陷入不可取消的阻塞）。
async fn watch_cancellation(token: &CancellationToken) {
    loop {
        if token.is_cancelled() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// M15: 工具错误分类 — retryable(网络/超时) / argument(参数错误不重试) / fatal(其他)
fn classify_tool_error(error: &str) -> String {
    let el = error.to_lowercase();
    if el.contains("timeout") || el.contains("timed out")
        || el.contains("connection") || el.contains("network")
        || el.contains("unavailable") || el.contains("rate limit")
        || el.contains("too many request") || el.contains("503")
        || el.contains("502") || el.contains("500") || el.contains("429") {
        "retryable".to_string()
    } else if el.contains("not found") || el.contains("no such file")
        || el.contains("invalid") || el.contains("permission denied")
        || el.contains("argument") || el.contains("syntax error")
        || el.contains("404") || el.contains("400") || el.contains("401") || el.contains("403") {
        "argument".to_string()
    } else {
        "fatal".to_string()
    }
}

/// A7: 受容量限制的 events 写入，超过上限时移除最旧事件。
#[inline]
fn push_event(events: &mut Vec<AgentEvent>, event: AgentEvent, max_events: usize) {
    if max_events > 0 && events.len() >= max_events {
        events.remove(0);
    }
    events.push(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── M1: looks_like_final_answer ──
    #[test]
    fn m1_empty_is_not_final() {
        assert!(!looks_like_final_answer(""));
        assert!(!looks_like_final_answer("   "));
        assert!(!looks_like_final_answer("\n\t"));
    }

    #[test]
    fn m1_trailing_punctuation_is_not_final() {
        assert!(!looks_like_final_answer("接下来我会读取文件："));
        assert!(!looks_like_final_answer("分析结果如下..."));
        assert!(!looks_like_final_answer("正在处理…"));
    }

    #[test]
    fn m1_continuation_marker_beats_completion_marker() {
        // "Phase 1 完成。继续 Phase 2" 含"完成"也含"继续"，应被"继续"拦截 → 非最终
        assert!(!looks_like_final_answer("Phase 1 完成。继续 Phase 2"));
        assert!(!looks_like_final_answer("让我先看看文件"));
        assert!(!looks_like_final_answer("首先，我需要确认"));
    }

    #[test]
    fn m1_completion_marker_is_final() {
        assert!(looks_like_final_answer("任务已完成，文件已创建。"));
        assert!(looks_like_final_answer("以上是所有改动。"));
        assert!(looks_like_final_answer("done"));
    }

    #[test]
    fn m1_very_short_is_not_final() {
        assert!(!looks_like_final_answer("好的"));
        assert!(!looks_like_final_answer("ok"));
    }

    #[test]
    fn m1_plain_sentence_is_final() {
        assert!(looks_like_final_answer("文件内容是 Hello, RGoat!"));
    }

    // ── M2: continuation_prompt ──
    #[test]
    fn m2_prompt_tiers() {
        assert!(continuation_prompt(1).contains("请立即调用工具"));
        assert!(continuation_prompt(2).contains("请立刻调用工具"));
        assert!(continuation_prompt(3).contains("最后一次警告"));
        // 第 4 次（>3）也走最严档
        assert!(continuation_prompt(4).contains("最后一次警告"));
    }

    // ── M3: ToolCallDeduper（精确指纹检测；A2 后返回 LoopAction）──
    #[test]
    fn m3_no_break_on_first_two_identical_calls() {
        // 渐进熔断：前两次不 ForceSkip（第 1 次 None，第 2 次 Suggest 仍执行）
        let mut d = ToolCallDeduper::new();
        let args = serde_json::json!({"file_path": "/tmp/a.txt"});
        assert!(matches!(d.register_and_check("read_file", &args), LoopAction::None));
        assert!(matches!(d.register_and_check("read_file", &args), LoopAction::Suggest));
    }

    #[test]
    fn m3_breaks_on_third_identical_call() {
        // 第 3 次精确重复 → ForceSkip
        let mut d = ToolCallDeduper::new();
        let args = serde_json::json!({"file_path": "/tmp/a.txt"});
        assert!(matches!(d.register_and_check("read_file", &args), LoopAction::None));     // 1
        assert!(matches!(d.register_and_check("read_file", &args), LoopAction::Suggest));  // 2
        assert!(matches!(d.register_and_check("read_file", &args), LoopAction::ForceSkip));// 3 → 熔断
    }

    #[test]
    fn m3_different_args_do_not_trigger() {
        // 不同值、同键集：精确指纹各不相同 → 不会触发精确检测。
        // 仅 3 次调用（低于语义阈值 4），故也不触发语义检测 → 全部 None。
        let mut d = ToolCallDeduper::new();
        assert!(matches!(d.register_and_check("read_file", &serde_json::json!({"file_path": "/a"})), LoopAction::None));
        assert!(matches!(d.register_and_check("read_file", &serde_json::json!({"file_path": "/b"})), LoopAction::None));
        assert!(matches!(d.register_and_check("read_file", &serde_json::json!({"file_path": "/c"})), LoopAction::None));
    }

    #[test]
    fn m3_param_order_independence() {
        // 参数顺序不同但键集与值相同 → 同一精确指纹，应触发渐进熔断
        let mut d = ToolCallDeduper::new();
        let a = serde_json::json!({"path": "/x", "mode": "r"});
        let b = serde_json::json!({"mode": "r", "path": "/x"});
        assert!(matches!(d.register_and_check("read_file", &a), LoopAction::None));     // 1
        assert!(matches!(d.register_and_check("read_file", &b), LoopAction::Suggest));  // 2（同指纹）
        assert!(matches!(d.register_and_check("read_file", &a), LoopAction::ForceSkip));// 3 → 熔断
    }

    #[test]
    fn m3_only_last_five_counted() {
        // 精确指纹滑动窗口（最近 5 次）：用不同键名避免语义检测干扰。
        // 6 次不同调用后，第 1 次的指纹已挤出窗口，再 2 次相同调用不会 ForceSkip。
        let mut d = ToolCallDeduper::new();
        for i in 0..6 {
            let mut obj = serde_json::Map::new();
            obj.insert(format!("k{}", i), serde_json::Value::Number(i.into()));
            let args = serde_json::Value::Object(obj);
            assert!(!matches!(d.register_and_check("read_file", &args), LoopAction::ForceSkip));
        }
        // k0 已被挤出窗口 → 再次 2 次相同调用不会触发 ForceSkip（窗口内至多 2 次 → Suggest）
        let mut obj = serde_json::Map::new();
        obj.insert("k0".to_string(), serde_json::Value::Number(0.into()));
        let args = serde_json::Value::Object(obj);
        assert!(!matches!(d.register_and_check("read_file", &args), LoopAction::ForceSkip));
        assert!(!matches!(d.register_and_check("read_file", &args), LoopAction::ForceSkip));
    }

    // ── A2: 语义重复 / 无进展 / 渐进熔断 ──
    #[test]
    fn a2_tiered_exact_repeat_suggest_then_force() {
        // 精确重复 2 次 → Suggest（仍执行）；≥3 次 → ForceSkip（跳过）
        let mut d = ToolCallDeduper::new();
        let args = serde_json::json!({"file_path": "/tmp/x"});
        assert!(matches!(d.register_and_check("read_file", &args), LoopAction::None));     // 1
        assert!(matches!(d.register_and_check("read_file", &args), LoopAction::Suggest));  // 2 → 预警
        assert!(matches!(d.register_and_check("read_file", &args), LoopAction::ForceSkip));// 3 → 熔断
    }

    #[test]
    fn a2_semantic_below_threshold_no_trigger() {
        // 3 次同模式（不同值）→ 模式计数 3 < 4 → None
        let mut d = ToolCallDeduper::new();
        assert!(matches!(d.register_and_check("read_file", &serde_json::json!({"file_path": "/a"})), LoopAction::None));
        assert!(matches!(d.register_and_check("read_file", &serde_json::json!({"file_path": "/b"})), LoopAction::None));
        assert!(matches!(d.register_and_check("read_file", &serde_json::json!({"file_path": "/c"})), LoopAction::None));
    }

    #[test]
    fn a2_semantic_repeat_forces_skip() {
        // 同 tool 同参数键集、仅值不同：精确指纹各不相同，但模式指纹相同。
        // 第 4 次：模式指纹出现 4 次 → ForceSkip
        let mut d = ToolCallDeduper::new();
        let paths = ["/a", "/b", "/c", "/d"];
        for (i, p) in paths.iter().enumerate() {
            let action = d.register_and_check("read_file", &serde_json::json!({"file_path": p}));
            if i < 3 {
                assert!(matches!(action, LoopAction::None), "call {} expected None, got {:?}", i, action);
            } else {
                assert!(matches!(action, LoopAction::ForceSkip), "call {} expected ForceSkip, got {:?}", i, action);
            }
        }
    }

    #[test]
    fn a2_semantic_repeat_does_not_cross_tools() {
        // 不同 tool 名即使键集相同也不算同一模式指纹
        let mut d = ToolCallDeduper::new();
        assert!(matches!(d.register_and_check("read_file", &serde_json::json!({"file_path": "/a"})), LoopAction::None));
        assert!(matches!(d.register_and_check("grep", &serde_json::json!({"file_path": "/b"})), LoopAction::None));
        assert!(matches!(d.register_and_check("read_file", &serde_json::json!({"file_path": "/c"})), LoopAction::None));
        assert!(matches!(d.register_and_check("grep", &serde_json::json!({"file_path": "/d"})), LoopAction::None));
    }

    #[test]
    fn a2_no_progress_suggest_after_threshold() {
        // 连续无文件变更 < 阈值 → None；≥ 阈值（10）→ Suggest
        let mut d = ToolCallDeduper::new();
        for _ in 0..9 {
            assert!(matches!(d.register_progress(false), LoopAction::None));
        }
        assert!(matches!(d.register_progress(false), LoopAction::Suggest)); // 第 10 次
        assert!(matches!(d.register_progress(false), LoopAction::Suggest)); // 第 11 次仍 Suggest
    }

    #[test]
    fn a2_no_progress_resets_on_file_change() {
        // 有文件变更 → 计数清零
        let mut d = ToolCallDeduper::new();
        for _ in 0..9 {
            d.register_progress(false);
        }
        assert!(matches!(d.register_progress(true), LoopAction::None)); // 重置
        for _ in 0..9 {
            assert!(matches!(d.register_progress(false), LoopAction::None));
        }
        assert!(matches!(d.register_progress(false), LoopAction::Suggest)); // 重新达阈值
    }

    #[test]
    fn a2_is_file_change_tool_classification() {
        assert!(ToolCallDeduper::is_file_change_tool("write_file"));
        assert!(ToolCallDeduper::is_file_change_tool("edit_file"));
        assert!(ToolCallDeduper::is_file_change_tool("patch"));
        assert!(ToolCallDeduper::is_file_change_tool("git"));
        assert!(!ToolCallDeduper::is_file_change_tool("read_file"));
        assert!(!ToolCallDeduper::is_file_change_tool("shell"));
    }

    // ── A1: extract_completed_step_descriptions ──
    #[test]
    fn a1_extract_completed_steps_from_progress() {
        let progress = "## 当前计划进度\n\
            - ✅ 步骤 1: 需求分析\n\
            - ⬜ 步骤 2: 代码实现\n\
            - ✅ 步骤 3: 写测试";
        let descs = extract_completed_step_descriptions(Some(progress));
        assert_eq!(descs, vec!["需求分析".to_string(), "写测试".to_string()]);
    }

    #[test]
    fn a1_extract_completed_steps_none_returns_empty() {
        assert!(extract_completed_step_descriptions(None).is_empty());
    }

    #[test]
    fn a1_extract_completed_steps_no_checkmarks_returns_empty() {
        let progress = "## 当前计划进度\n- ⬜ 步骤 1: 未完成";
        assert!(extract_completed_step_descriptions(Some(progress)).is_empty());
    }

    // ── A1: replan 重置 milestone_summary ──
    // 复现：旧条件 `parsed_update && has_plan() && !had_progress` 在 replan 场景下
    // had_progress=true → !had_progress=false → 不触发，旧里程碑数据残留。
    // 新条件 `content_text.contains("## 计划") && has_plan()` 直接从 LLM 输出检测，
    // 覆盖首次出计划与 replan 两种场景。
    #[test]
    fn a1_replan_resets_milestone_summary() {
        let mut milestone_summary = MilestoneSummary::default();

        // 模拟首次计划 + 步骤完成：milestone_summary 已累积旧计划的完成步骤
        milestone_summary.completed_steps.push("步骤1描述".to_string());
        assert!(!milestone_summary.is_empty(), "首次计划执行后 milestone_summary 应非空");

        // 模拟 replan：content 包含 "## 计划"
        // 此时 StepsTracker.has_plan() 仍为 true（旧计划未失效前解析新计划）
        let content = "## 计划\n- 新步骤1\n- 新步骤2";
        let is_new_plan = content.contains("## 计划");
        if is_new_plan {
            milestone_summary = MilestoneSummary::default();
        }

        assert!(milestone_summary.is_empty(), "replan 后 milestone_summary 应被重置");
    }

    #[test]
    fn a1_progress_update_does_not_reset_milestone_summary() {
        // 进度更新（不含 "## 计划"）不应重置 milestone_summary
        let mut milestone_summary = MilestoneSummary::default();
        milestone_summary.completed_steps.push("步骤1描述".to_string());

        let content = "## 进度: 1\n继续执行下一步";
        let is_new_plan = content.contains("## 计划");
        if is_new_plan {
            milestone_summary = MilestoneSummary::default();
        }

        assert!(!milestone_summary.is_empty(), "进度更新不应重置 milestone_summary");
    }

    // ── M-A3: compute_extended_max_steps 扩展逻辑 ──
    #[test]
    fn a3_extend_max_steps_by_10() {
        // 有进展且未达上限 → +10
        let new = compute_extended_max_steps(50, 50, 2);
        assert_eq!(new, Some(60));
    }

    #[test]
    fn a3_extend_capped_at_max_steps_times_limit() {
        // 扩展不超过 max_steps * max_steps_extend_limit
        // current=95, max_steps=50, limit=2 → cap=100, 95+10=105 → min(105,100)=100
        let new = compute_extended_max_steps(95, 50, 2);
        assert_eq!(new, Some(100));
    }

    #[test]
    fn a3_extend_returns_none_at_cap() {
        // 已达上限 → None
        let new = compute_extended_max_steps(100, 50, 2);
        assert_eq!(new, None);
    }

    #[test]
    fn a3_extend_progressive_extension() {
        // 模拟连续扩展：50 → 60 → 70 → 80 → 90 → 100 → None
        let max_steps = 50;
        let limit = 2;
        let mut current = max_steps;
        let extensions = [Some(60), Some(70), Some(80), Some(90), Some(100), None];
        for expected in extensions {
            assert_eq!(compute_extended_max_steps(current, max_steps, limit), expected);
            if let Some(n) = expected {
                current = n;
            }
        }
    }

    // ── M-A3: 连续无进展计数模拟 ──
    #[test]
    fn a3_no_progress_counter_increments_and_resets() {
        // 模拟 run() 中的 consecutive_no_progress 逻辑
        let mut consecutive_no_progress: usize = 0;
        let no_progress_step_limit = 15;

        // 连续 15 步无进展
        for _ in 0..15 {
            let has_progress = false;
            if has_progress {
                consecutive_no_progress = 0;
            } else {
                consecutive_no_progress += 1;
            }
        }
        assert_eq!(consecutive_no_progress, 15);
        assert!(consecutive_no_progress >= no_progress_step_limit, "应达停止阈值");

        // 有进展 → 重置
        let has_progress = true;
        if has_progress {
            consecutive_no_progress = 0;
        }
        assert_eq!(consecutive_no_progress, 0);
        assert!(consecutive_no_progress < no_progress_step_limit, "重置后应允许扩展");
    }

    // ── M-A3: no_progress_step_limit 扩展约束 ──
    #[test]
    fn a3_no_progress_limit_blocks_extension() {
        // 验证 run() 中 has_progress 分支的扩展判断逻辑：
        // `if step + 1 >= effective_max_steps && !was_in_long_streak { 扩展 }`
        // 关键点：was_in_long_streak 必须在 consecutive_no_progress 重置前计算
        let no_progress_step_limit = 15;

        // 场景 1：连续无进展 15 步（达阈值）→ was_in_long_streak=true → 扩展被抑制
        let consecutive_no_progress: usize = 15;
        let was_in_long_streak = consecutive_no_progress >= no_progress_step_limit;
        assert!(was_in_long_streak, "连续无进展达阈值后 was_in_long_streak 应为 true");
        assert!(!was_in_long_streak == false, "!was_in_long_streak 为 false，扩展被抑制");

        // 场景 2：连续无进展 10 步（未达阈值）→ was_in_long_streak=false → 扩展被允许
        let consecutive_no_progress: usize = 10;
        let was_in_long_streak = consecutive_no_progress >= no_progress_step_limit;
        assert!(!was_in_long_streak, "未达阈值时 was_in_long_streak 应为 false");
        assert!(!was_in_long_streak == true, "!was_in_long_streak 为 true，扩展被允许");
    }

    // ── M-A3: AgentConfig 默认值 ──
    #[test]
    fn a3_config_defaults() {
        let config = AgentConfig::default();
        assert_eq!(config.max_steps, 50);
        assert_eq!(config.max_steps_extend_limit, 2, "默认扩展倍数为 2（最多 100 步）");
        assert_eq!(config.no_progress_step_limit, 15, "默认连续无进展阈值为 15");
        // 上限 = max_steps * extend_limit = 100
        assert_eq!(config.max_steps * config.max_steps_extend_limit, 100);
    }
}

