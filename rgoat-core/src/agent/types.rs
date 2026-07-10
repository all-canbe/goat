//! Agent 运行时类型

use serde::{Deserialize, Serialize};

/// Agent 执行错误
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Tool error: {0}")]
    Tool(String),
    #[error("Approval denied: {0}")]
    ApprovalDenied(String),
    #[error("Max steps exceeded ({0})")]
    MaxStepsExceeded(usize),
    #[error("Cancelled")]
    Cancelled,
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("JSON error: {0}")]
    Json(serde_json::Error),
}

impl Clone for AgentError {
    fn clone(&self) -> Self {
        match self {
            Self::Llm(s) => Self::Llm(s.clone()),
            Self::Tool(s) => Self::Tool(s.clone()),
            Self::ApprovalDenied(s) => Self::ApprovalDenied(s.clone()),
            Self::MaxStepsExceeded(n) => Self::MaxStepsExceeded(*n),
            Self::Cancelled => Self::Cancelled,
            Self::ParseError(s) => Self::ParseError(s.clone()),
            Self::Io(e) => Self::Io(std::io::Error::new(e.kind(), e.to_string())),
            Self::Json(e) => Self::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))),
        }
    }
}

impl From<std::io::Error> for AgentError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for AgentError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Agent 事件（用于 UI/日志观察）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// 开始运行
    Started { mode: String, prompt: String },
    /// 思考内容
    Thought { step: usize, content: String },
    /// 工具调用请求
    ToolCall {
        step: usize,
        tool_name: String,
        arguments: serde_json::Value,
    },
    /// 工具执行结果
    ToolResult {
        step: usize,
        tool_name: String,
        success: bool,
        output: String,
    },
    /// 审批结果
    Approval {
        tool_name: String,
        decision: String,
        message: String,
    },
    /// LLM 回复（最终答案）
    Message { role: String, content: String },
    /// 步骤完成
    StepCompleted { step: usize, total_steps: usize },
    /// 完成
    Finished { answer: String, steps: usize },
    /// 错误
    Error { message: String },
    /// 流式逐词推送（按空格分词后的单个词）
    MessageDelta { delta: String },
    /// Token 使用统计（流式结束后发送）
    Usage { input_tokens: u64, output_tokens: u64 },
    /// Agent 被用户取消（携带已生成的部分回答）
    Cancelled { partial_answer: String },
    /// 需要用户审批的工具调用
    ApprovalRequired {
        tool_name: String,
        /// "Shell" | "Write" | "Network"
        tool_type: String,
        summary: String,
        /// "LOW" | "MEDIUM" | "HIGH"
        risk_level: String,
        /// Shell 工具专用
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// Write 工具专用
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        /// Network 工具专用
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// 上下文压缩完成（M6: 防上下文溢出）
    ContextCompacted {
        level: String,
        messages_before: usize,
        messages_after: usize,
        estimated_tokens: usize,
    },
    /// M15 PostToolUseFailure: 工具执行失败的分类事件
    ToolFailed {
        tool_name: String,
        error: String,
        /// "retryable"(网络/超时), "argument"(无效参数), "fatal"(其他)
        failure_type: String,
    },
}

/// 单次 ReAct 步骤的结果
#[derive(Debug, Clone)]
pub enum StepResult {
    /// 继续下一步（需要执行工具）
    Continue { tool_name: String, arguments: serde_json::Value, observation: String },
    /// 已完成，给出最终答案
    Finished { answer: String },
    /// 需要用户确认
    NeedsApproval { tool_name: String, arguments: serde_json::Value, reason: String },
}

/// Agent 配置
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// 最大 ReAct 步骤数
    pub max_steps: usize,
    /// 模型名称
    pub model: String,
    /// 温度
    pub temperature: f64,
    /// 最大 token
    pub max_tokens: u32,
    /// 是否启用流式输出
    pub stream: bool,
    /// 子 Agent 最大并发数
    pub max_subagents: usize,
    /// 子 Agent 最大深度
    pub max_subagent_depth: usize,
    /// Flow 模式最大审查轮数
    pub max_flow_rounds: usize,
    /// M13: 自动验证（Agent 声称完成前跑 cargo check / npx tsc）
    pub auto_verify: bool,
    /// M14: 实际上下文窗口（从 /models API 获取，用于动态压缩阈值）
    pub context_window: usize,
    /// M-A3: 最大步骤自适应扩展上限（有进展时可扩展到此倍数）
    pub max_steps_extend_limit: usize,
    /// M-A3: 连续无进展步骤阈值（超过此值才考虑停止）
    pub no_progress_step_limit: usize,
    /// A7: events 列表上限（超过后移除最旧事件），0 表示无限制
    pub max_events: usize,
    /// B2: 历史消息加载上限（build_messages 使用）
    pub max_history_messages: usize,
    /// C1: 是否启用强制规划阶段（主循环前先生成 ## 计划）
    pub planning_enabled: bool,
    /// C4: 是否启用任务持久化 + 断点续传（每步事件写入 tasks.jsonl）
    pub task_persistence_enabled: bool,
    /// D1: 看门狗超时阈值（秒），LLM 调用超过此时间无响应则保存 checkpoint 并暂停
    pub watchdog_timeout_secs: u64,
    /// D1: checkpoint 写入间隔（步数），每 N 步自动写入一次 checkpoint
    pub checkpoint_interval_steps: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: 50,
            model: "deepseek-chat".to_string(),
            temperature: 0.3,
            max_tokens: 4096,
            stream: true,
            max_subagents: 10,
            max_subagent_depth: 3,
            max_flow_rounds: 3,
            auto_verify: true,
            context_window: 128_000,
            max_steps_extend_limit: 2,
            no_progress_step_limit: 15,
            max_events: 2000,
            max_history_messages: 200,
            planning_enabled: false,
            task_persistence_enabled: true,
            watchdog_timeout_secs: 300,
            checkpoint_interval_steps: 10,
        }
    }
}

/// 从 LLM 输出中解析出的工具调用
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Agent 运行结果
#[derive(Debug, Clone)]
pub struct AgentRunResult {
    pub answer: String,
    pub steps_taken: usize,
    pub tool_calls: usize,
    pub events: Vec<AgentEvent>,
}
