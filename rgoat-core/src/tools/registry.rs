//! 工具注册表 — 统一管理所有可用工具

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use crate::core::event_bus::EventBus;
use crate::security::approval::ToolCategory;
use crate::provider::provider::ToolDef;

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

    /// 执行工具
    async fn execute(&self, args: serde_json::Value) -> ToolResult;

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
