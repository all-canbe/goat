//! 模型路由 — 按任务复杂度选择最优模型
//!
//! 三层路由策略：
//! L1: 本地模型 (简单任务、离线）
//! L2: DeepSeek/GLM (编码、推理、成本敏感)
//! L3: Claude/GPT-4 (需要最强能力时)

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::provider::ProviderConfig;

/// 模型层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModelTier {
    /// L1: 本地模型 — 简单问答、离线兜底
    Local = 1,
    /// L2: 性价比模型 — 编码、多步推理、日常对话
    CostEfficient = 2,
    /// L3: 最强模型 — 复杂架构决策、安全审查
    Premium = 3,
}

/// 路由上下文
pub struct RoutingContext {
    /// 当前是否离线
    pub is_offline: bool,
    /// 任务类型 hint
    pub task_hint: Option<TaskHint>,
    /// 消息历史长度
    pub message_count: usize,
    /// 是否有工具调用历史
    pub has_tool_history: bool,
}

/// 任务类型提示
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskHint {
    /// 简单问答
    SimpleQa,
    /// 编码任务
    Coding,
    /// 多步推理
    MultiStep,
    /// 架构设计
    Architecture,
    /// 安全审查
    Security,
    /// 文档写作
    Writing,
}

/// 模型路由器
pub struct ModelRouter {
    local_config: Option<ProviderConfig>,
    cost_efficient_config: ProviderConfig,
    premium_config: Option<ProviderConfig>,
    /// 需要强制升级到 Premium 的工具名
    premium_tools: HashSet<String>,
}

impl ModelRouter {
    /// 创建新的路由器（L1=本地, L2=DeepSeek, L3=Claude）
    pub fn new(
        local: Option<ProviderConfig>,
        cost_efficient: ProviderConfig,
        premium: Option<ProviderConfig>,
    ) -> Self {
        Self {
            local_config: local,
            cost_efficient_config: cost_efficient,
            premium_config: premium,
            premium_tools: HashSet::new(),
        }
    }

    /// 添加需要 Premium 模式的工具
    pub fn register_premium_tool(&mut self, tool_name: &str) {
        self.premium_tools.insert(tool_name.to_string());
    }

    /// 根据上下文选择模型层级
    pub fn route(&self, context: &RoutingContext) -> ModelTier {
        // 离线 → 只能 L1
        if context.is_offline {
            return ModelTier::Local;
        }

        // 根据任务类型
        if let Some(hint) = &context.task_hint {
            match hint {
                TaskHint::Architecture | TaskHint::Security => {
                    if self.premium_config.is_some() {
                        return ModelTier::Premium;
                    }
                }
                TaskHint::Coding | TaskHint::MultiStep => {
                    return ModelTier::CostEfficient;
                }
                TaskHint::SimpleQa | TaskHint::Writing => {
                    return ModelTier::CostEfficient; // L2 够用
                }
            }
        }

        // 消息偏多 → L2
        if context.message_count > 20 {
            return ModelTier::CostEfficient;
        }

        // 有工具调用历史 → L2
        if context.has_tool_history {
            return ModelTier::CostEfficient;
        }

        // Default: L2
        ModelTier::CostEfficient
    }

    /// Route and return the corresponding config
    pub fn get_config(&self, tier: ModelTier) -> &ProviderConfig {
        match tier {
            ModelTier::Local => self
                .local_config
                .as_ref()
                .unwrap_or(&self.cost_efficient_config),
            ModelTier::CostEfficient => &self.cost_efficient_config,
            ModelTier::Premium => self
                .premium_config
                .as_ref()
                .unwrap_or(&self.cost_efficient_config),
        }
    }
}
