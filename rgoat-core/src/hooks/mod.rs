//! 生命周期钩子
//!
//! 在 Agent 执行的关键节点插入自定义逻辑：
//! - 工具调用前（pre-tool）
//! - 工具调用后（post-tool）
//! - 消息发送前（pre-message）
//! - Agent 完成时（on-finish）

use async_trait::async_trait;
use serde_json::Value;

/// 钩子触发时机
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPoint {
    PreTool,
    PostTool,
    PreMessage,
    OnFinish,
}

/// 钩子上下文
#[derive(Debug, Clone)]
pub struct HookContext {
    pub hook_point: HookPoint,
    pub tool_name: Option<String>,
    pub arguments: Option<Value>,
    pub result: Option<Value>,
    pub message: Option<String>,
}

/// 钩子 trait
#[async_trait]
pub trait Hook: Send + Sync {
    fn name(&self) -> &str;
    fn points(&self) -> Vec<HookPoint>;
    async fn run(&self, ctx: &HookContext) -> Result<Option<String>, HookError>;
}

/// 钩子错误
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("Hook blocked: {0}")]
    Blocked(String),
    #[error("Hook error: {0}")]
    Error(String),
}

/// 钩子管理器
pub struct HookManager {
    hooks: Vec<Box<dyn Hook>>,
}

impl HookManager {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn register(&mut self, hook: Box<dyn Hook>) {
        self.hooks.push(hook);
    }

    /// 运行指定时机的钩子
    pub async fn run(&self, point: HookPoint, ctx: &mut HookContext) -> Result<Option<String>, HookError> {
        let mut combined = String::new();
        for hook in &self.hooks {
            if hook.points().contains(&point) {
                if let Some(output) = hook.run(ctx).await? {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&output);
                }
            }
        }
        Ok(if combined.is_empty() { None } else { Some(combined) })
    }
}

impl Default for HookManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 示例：日志钩子
pub struct LoggingHook;

#[async_trait]
impl Hook for LoggingHook {
    fn name(&self) -> &str { "logging" }

    fn points(&self) -> Vec<HookPoint> {
        vec![HookPoint::PreTool, HookPoint::PostTool]
    }

    async fn run(&self, ctx: &HookContext) -> Result<Option<String>, HookError> {
        match ctx.hook_point {
            HookPoint::PreTool => {
                tracing::info!(
                    "[hook] pre-tool: {:?} args: {:?}",
                    ctx.tool_name, ctx.arguments
                );
            }
            HookPoint::PostTool => {
                tracing::info!(
                    "[hook] post-tool: {:?} result: {:?}",
                    ctx.tool_name, ctx.result
                );
            }
            _ => {}
        }
        Ok(None)
    }
}
