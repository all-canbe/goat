//! 多层审批引擎
//!
//! 五层防御链：
//! 1. YOLO 快速通道（+ bypass immune guard）
//! 2. 模式规则引擎（Agent/Plan/Flow/YOLO）
//! 3. 工具类别判定（Read/Write/Shell/Network/Destructive）
//! 4. 钩子系统（可扩展）
//! 5. 沙箱强制执行

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ============================================================================
// 枚举与基础类型
// ============================================================================

/// Agent 运行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentMode {
    /// 默认模式：需要确认写入/执行操作
    Agent,
    /// Plan 模式：只读探索，生成计划
    Plan,
    /// Flow 模式：实现→审查→修复
    Flow,
    /// Auto 模式：自动接受编辑
    AcceptEdits,
    /// YOLO 模式：全部自动批准
    Yolo,
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMode::Agent => write!(f, "agent"),
            AgentMode::Plan => write!(f, "plan"),
            AgentMode::Flow => write!(f, "flow"),
            AgentMode::AcceptEdits => write!(f, "accept-edits"),
            AgentMode::Yolo => write!(f, "yolo"),
        }
    }
}

/// 工具类别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolCategory {
    /// 只读操作：读文件、搜索、glob
    Read,
    /// 写操作：写文件、编辑
    Write,
    /// Shell 命令
    Shell,
    /// 网络操作：WebFetch, WebSearch
    Network,
    /// 破坏性操作：删除文件、修改 git 历史
    Destructive,
    /// MCP 工具
    Mcp,
    /// 子 Agent 调用
    Agent,
    /// 交互式工具（需用户输入）
    Interactive,
}

/// 审批决策
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    /// 允许
    Allow,
    /// 阻止
    Block,
    /// 询问用户
    Ask,
    /// 延后（让其他层决定）
    Defer,
}

/// 单次审批结果
#[derive(Debug, Clone)]
pub struct ApprovalResult {
    pub decision: Decision,
    pub source: String,
    pub message: String,
    /// 是否为绕过免疫（即使 YOLO 模式也阻止）
    pub bypass_immune: bool,
}

/// 审批策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPolicy {
    pub tool_name: String,
    pub decision: Decision,
    pub reason: String,
}

/// 审批规则
pub trait ApprovalRule: Send + Sync {
    fn evaluate(
        &self,
        tool_name: &str,
        category: ToolCategory,
        mode: AgentMode,
    ) -> Option<Decision>;
}

/// 工具调用信息
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
    pub category: ToolCategory,
}

// ============================================================================
// 审批引擎
// ============================================================================

pub struct ApprovalEngine {
    /// 用户自定义规则
    policies: Vec<ApprovalPolicy>,
    /// 扩展规则
    rules: Vec<Box<dyn ApprovalRule>>,
    /// YOLO 模式下仍需阻止的工具
    bypass_immune_tools: HashSet<String>,
}

impl ApprovalEngine {
    /// 创建审批引擎
    pub fn new() -> Self {
        let mut immune = HashSet::new();
        // 即使在 YOLO 模式也阻止这些操作
        immune.insert("rm".to_string());
        immune.insert("rmdir".to_string());
        immune.insert("del".to_string());
        immune.insert("format".to_string());
        immune.insert("shutdown".to_string());

        Self {
            policies: Vec::new(),
            rules: Vec::new(),
            bypass_immune_tools: immune,
        }
    }

    /// 添加审批策略
    pub fn add_policy(&mut self, policy: ApprovalPolicy) {
        self.policies.push(policy);
    }

    /// 添加审批规则
    pub fn add_rule(&mut self, rule: Box<dyn ApprovalRule>) {
        self.rules.push(rule);
    }

    /// 添加绕过免疫工具
    pub fn add_immune_tool(&mut self, tool: &str) {
        self.bypass_immune_tools.insert(tool.to_string());
    }

    /// 检查单个工具调用
    pub async fn check(
        &self,
        mode: AgentMode,
        tool_name: &str,
        category: ToolCategory,
        _args: &serde_json::Value,
    ) -> ApprovalResult {
        // ========== 第一层：YOLO 快速通道 ==========
        if mode == AgentMode::Yolo {
            if self.bypass_immune_tools.contains(tool_name) {
                return ApprovalResult {
                    decision: Decision::Block,
                    source: "bypass-immune".to_string(),
                    message: format!("Tool '{}' is bypass-immune even in YOLO mode", tool_name),
                    bypass_immune: true,
                };
            }
            return ApprovalResult {
                decision: Decision::Allow,
                source: "yolo".to_string(),
                message: "YOLO mode: automatically approved".to_string(),
                bypass_immune: false,
            };
        }

        // ========== 第一层半：Flow 快速通道 ==========
        // Flow 模式下非破坏性工具自动批准（对齐 Python PermissionMode.FLOW）
        if mode == AgentMode::Flow {
            if self.bypass_immune_tools.contains(tool_name) {
                return ApprovalResult {
                    decision: Decision::Block,
                    source: "bypass-immune".to_string(),
                    message: format!("Tool '{}' is bypass-immune even in Flow mode", tool_name),
                    bypass_immune: true,
                };
            }
            return match category {
                ToolCategory::Destructive => ApprovalResult {
                    decision: Decision::Block,
                    source: "flow-mode".to_string(),
                    message: "Destructive tool blocked in Flow mode".to_string(),
                    bypass_immune: true,
                },
                // 非破坏性工具自动批准
                _ => ApprovalResult {
                    decision: Decision::Allow,
                    source: "flow".to_string(),
                    message: "Flow mode: automatically approved".to_string(),
                    bypass_immune: false,
                },
            };
        } else {
        // ========== 第二层：用户自定义策略 ==========
        for policy in &self.policies {
            if policy.tool_name == tool_name {
                return ApprovalResult {
                    decision: policy.decision,
                    source: "policy".to_string(),
                    message: policy.reason.clone(),
                    bypass_immune: false,
                };
            }
        }

        // ========== 第三层：扩展规则 ==========
        for rule in &self.rules {
            if let Some(decision) = rule.evaluate(tool_name, category, mode) {
                return ApprovalResult {
                    decision,
                    source: "rule".to_string(),
                    message: format!("Rule decision for '{}'", tool_name),
                    bypass_immune: false,
                };
            }
        }

        } // end else (non-Flow mode)

        // ========== 第四层：工具类别判定 ==========
        match category {
            ToolCategory::Read => ApprovalResult {
                decision: Decision::Allow,
                source: "category".to_string(),
                message: "Read-only tool: automatically allowed".to_string(),
                bypass_immune: false,
            },
            ToolCategory::Write => {
                if mode == AgentMode::Plan {
                    ApprovalResult {
                        decision: Decision::Block,
                        source: "plan-mode".to_string(),
                        message: "Write tool blocked in Plan mode".to_string(),
                        bypass_immune: false,
                    }
                } else {
                    ApprovalResult {
                        decision: Decision::Ask,
                        source: "category".to_string(),
                        message: "Write tool requires confirmation".to_string(),
                        bypass_immune: false,
                    }
                }
            }
            ToolCategory::Shell | ToolCategory::Network => ApprovalResult {
                decision: Decision::Ask,
                source: "category".to_string(),
                message: format!("{:?} tool requires confirmation", category),
                bypass_immune: false,
            },
            ToolCategory::Destructive => ApprovalResult {
                decision: Decision::Block,
                source: "category".to_string(),
                message: "Destructive tool blocked".to_string(),
                bypass_immune: true,
            },
            ToolCategory::Mcp => ApprovalResult {
                decision: Decision::Ask,
                source: "category".to_string(),
                message: "MCP tool requires confirmation".to_string(),
                bypass_immune: false,
            },
            ToolCategory::Agent => ApprovalResult {
                decision: Decision::Ask,
                source: "category".to_string(),
                message: "Sub-agent call requires confirmation".to_string(),
                bypass_immune: false,
            },
            ToolCategory::Interactive => ApprovalResult {
                decision: Decision::Allow,
                source: "category".to_string(),
                message: "Interactive tool: automatically allowed".to_string(),
                bypass_immune: false,
            },
        }
    }

    /// 批量检查工具调用
    pub async fn check_batch(
        &self,
        mode: AgentMode,
        calls: &[ToolCall],
    ) -> Vec<ApprovalResult> {
        let mut results = Vec::new();
        for call in calls {
            let result = self.check(mode, &call.name, call.category, &call.arguments).await;
            results.push(result);
        }
        results
    }
}

impl Default for ApprovalEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 审批决策与通道类型
// ============================================================================

/// 用户审批决策
#[derive(Debug, Clone)]
pub struct ApprovalDecision {
    /// 是否批准该次工具调用
    pub approved: bool,
    /// 是否批准后续所有同类工具调用（免审批）
    pub approve_all: bool,
}

/// Agent 与 TUI 之间的审批通道
///
/// Agent 侧通过 `oneshot::Sender` 发送审批请求，
/// TUI 侧消费 `oneshot::Receiver` 并回传 `ApprovalDecision`。
pub type ApprovalResponder = std::sync::Arc<
    tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<ApprovalDecision>>>,
>;
