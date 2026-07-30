//! 工具注册表 — 统一管理所有可用工具

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use crate::core::cancellation::CancellationToken;
use crate::core::event_bus::EventBus;
use crate::security::approval::ToolCategory;
use crate::provider::provider::ToolDef;

/// 工具执行模式 — 决定 agent loop 中同批 tool_calls 能否并行执行
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// 无副作用工具，可与同批其他 Parallel 工具并行执行
    /// Read / Glob / Grep / WebSearch / WebFetch
    Parallel,
    /// 有副作用或依赖顺序的工具，必须串行执行
    /// Write / Edit / Shell / Git / AskUser / TaskTool
    Sequential,
}

/// 工具执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub metadata: Option<serde_json::Value>,
    /// D1-T01: 文件变更的 unified diff（仅写工具填充）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// D1-T01: 受影响文件相对路径列表（仅写工具填充）
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub affected_files: Vec<String>,
    /// P3-2: 是否终止 agent 循环。当同批所有工具结果的 terminate=true 时，agent 结束后续回合。
    #[serde(default)]
    pub terminate: bool,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
            metadata: None,
            diff: None,
            affected_files: Vec::new(),
            terminate: false,
        }
    }

    pub fn error(output: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: output.into(),
            error: Some(error.into()),
            metadata: None,
            diff: None,
            affected_files: Vec::new(),
            terminate: false,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// D1-T01: 附带 diff 与受影响文件（builder 风格）
    pub fn with_diff(mut self, diff: impl Into<String>, affected_files: Vec<String>) -> Self {
        self.diff = Some(diff.into());
        self.affected_files = affected_files;
        self
    }

    /// P3-2: 标记此工具结果为终止信号（builder 风格）
    pub fn with_terminate(mut self) -> Self {
        self.terminate = true;
        self
    }
}

// ---------------------------------------------------------------------------
// Streaming support
// ---------------------------------------------------------------------------

/// A streaming update emitted during a tool's execution.
/// Used for incremental output (e.g., shell command stdout/stderr).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStreamEvent {
    /// The tool_call_id this update belongs to.
    pub tool_call_id: String,
    /// Incremental content delta.
    pub content: String,
    /// Optional metadata (e.g., stream type "stdout" / "stderr").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Execution context passed to [`Tool::execute_ctx`].
///
/// Carries cancellation token and an optional streaming update sender.
/// Tools that support incremental output should check the sender and
/// emit `ToolStreamEvent` updates.
#[derive(Clone, Default)]
pub struct ToolExecutionContext {
    /// The model-issued tool_call_id this execution belongs to.
    /// Streaming tools must tag every `ToolStreamEvent` with this id rather
    /// than a fabricated shared id. Empty when invoked outside the agent loop.
    pub tool_call_id: String,
    /// Cancellation token for cooperative interruption.
    pub cancellation: Option<CancellationToken>,
    /// Optional sender for streaming updates.
    update_tx: Option<tokio::sync::mpsc::UnboundedSender<ToolStreamEvent>>,
}

impl ToolExecutionContext {
    /// Create a new context with just a cancellation token (no streaming).
    /// `tool_call_id` defaults to empty (non-agent invocation).
    pub fn new(cancellation: Option<CancellationToken>) -> Self {
        Self {
            tool_call_id: String::new(),
            cancellation,
            update_tx: None,
        }
    }

    /// Create a context with both cancellation and streaming support.
    ///
    /// `tool_call_id` must be the real id returned by the model so that
    /// streaming updates are correlated to the originating tool call.
    pub fn with_streaming(
        tool_call_id: String,
        cancellation: Option<CancellationToken>,
        update_tx: tokio::sync::mpsc::UnboundedSender<ToolStreamEvent>,
    ) -> Self {
        Self {
            tool_call_id,
            cancellation,
            update_tx: Some(update_tx),
        }
    }

    /// Returns `true` if a streaming sender is configured.
    pub fn has_updates(&self) -> bool {
        self.update_tx.is_some()
    }

    /// Send a streaming update. No-op if no sender is configured.
    pub fn send_update(&self, event: ToolStreamEvent) {
        if let Some(tx) = &self.update_tx {
            let _ = tx.send(event);
        }
    }
}

/// 工具 trait
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具名称
    fn name(&self) -> &str;

    /// 工具描述
    fn description(&self) -> &str;

    /// 参数 schema (JSON Schema)
    fn parameters(&self) -> serde_json::Value;

    /// 工具类别（用于审批系统分类）
    fn category(&self) -> ToolCategory;

    /// 工具执行模式 — 决定能否与同批其他工具并行执行
    /// 默认 Sequential（安全默认），无副作用工具应覆写为 Parallel
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    /// 执行工具
    async fn execute(&self, args: serde_json::Value) -> ToolResult;

    /// 执行工具（带流式上下文）
    ///
    /// 默认实现委托给 [`execute`]，忽略上下文。
    /// 支持增量输出的工具（如 ShellTool）应覆写此方法，通过
    /// `ctx.send_update()` 推送 `ToolStreamEvent` 更新。
    async fn execute_ctx(&self, args: serde_json::Value, _ctx: &ToolExecutionContext) -> ToolResult {
        self.execute(args).await
    }

    /// 转为 LLM 工具定义
    fn to_tool_def(&self) -> ToolDef {
        ToolDef {
            tool_type: "function".to_string(),
            function: crate::provider::provider::FunctionDef {
                name: self.name().to_string(),
                description: self.description().to_string(),
                parameters: self.parameters(),
            },
        }
    }
}

/// 工具注册表
#[derive(Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Pending ask-user requests (shared with desktop IPC for respond_ask_user).
    /// Keyed by request_id → oneshot::Sender<String>.
    pub pending_ask_user: Option<Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<String>>>>>,
}

impl ToolRegistry {
    /// 创建注册表（含内置工具）
    pub fn new(workspace: std::path::PathBuf) -> Self {
        Self::with_event_bus(workspace, None)
    }

    /// 创建注册表，可选择传入 EventBus 以注册 AskUserTool
    pub fn with_event_bus(workspace: std::path::PathBuf, event_bus: Option<Arc<EventBus>>) -> Self {
        let builtin = crate::tools::builtin::create_builtin_tools(
            workspace,
            reqwest::Client::new(),
            None,
            None,
            event_bus,
        );
        let mut registry = Self {
            tools: HashMap::new(),
            pending_ask_user: builtin.pending_ask_user,
        };
        for tool in builtin.tools {
            registry.register(tool);
        }
        registry
    }

    /// 创建空注册表
    pub fn empty() -> Self {
        Self {
            tools: HashMap::new(),
            pending_ask_user: None,
        }
    }

    /// 注册工具
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// 批量注册
    pub fn register_all(&mut self, tools: Vec<Arc<dyn Tool>>) {
        for tool in tools {
            self.register(tool);
        }
    }

    /// 获取工具
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// 调用工具
    pub async fn execute(&self, name: &str, args: serde_json::Value) -> ToolResult {
        match self.get(name) {
            Some(tool) => tool.execute(args).await,
            None => ToolResult::error(
                format!("Unknown tool: {}", name),
                "tool_not_found",
            ),
        }
    }

    /// 调用工具（带流式上下文）
    pub async fn execute_ctx(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> ToolResult {
        match self.get(name) {
            Some(tool) => tool.execute_ctx(args, ctx).await,
            None => ToolResult::error(
                format!("Unknown tool: {}", name),
                "tool_not_found",
            ),
        }
    }

    /// 获取所有工具定义（传给 LLM）
    pub fn all_tool_defs(&self) -> Vec<ToolDef> {
        self.tools.values().map(|t| t.to_tool_def()).collect()
    }

    /// 获取所有工具名
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// 获取所有工具数量
    pub fn count(&self) -> usize {
        self.tools.len()
    }

    /// 获取某个类别的所有工具
    pub fn by_category(&self, category: ToolCategory) -> Vec<&Arc<dyn Tool>> {
        self.tools
            .values()
            .filter(|t| t.category() == category)
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::empty()
    }
}
