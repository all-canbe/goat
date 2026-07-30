//! 事件总线 — 基于 tokio::sync::broadcast 的发布/订阅系统
//!
//! 解耦各模块通信，支持：
//! - LLM 输出流事件
//! - 工具调用结果事件
//! - 子 Agent 生命周期事件
//! - 审批请求事件

use std::sync::Arc;
use tokio::sync::broadcast;

/// 事件类型枚举
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventType {
    LlmStreamChunk,
    LlmStreamDone,
    ToolCallStart,
    ToolCallResult,
    SubAgentSpawned,
    SubAgentCompleted,
    SubAgentFailed,
    ApprovalRequested,
    ApprovalDecided,
    SessionCreated,
    SessionSwitched,
    ConversationArchived,
    FlowRoundStart,
    FlowRoundComplete,
    PlanGenerated,
    PlanExecuting,
    TaskCreated,
    TaskCompleted,
    Error,
    Shutdown,
    MessageDelta,
    Usage,
    ApprovalRequired,
    AgentCancelled,
    /// P2-1: 工具运行时增量输出流事件
    ToolStreamUpdate,
}

/// 事件数据结构
#[derive(Debug, Clone)]
pub struct Event {
    pub event_type: EventType,
    pub source: String,
    pub data: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub type EventCallback = Arc<dyn Fn(&Event) + Send + Sync>;

/// 事件总线
///
/// # Example
/// ```ignore
/// let bus = EventBus::new(256);
/// let mut rx = bus.subscribe();
/// bus.emit(EventType::LlmStreamDone, "main", json!({"turns": 5}));
/// let event = rx.recv().await.unwrap();
/// ```
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Subscribe to all events
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// Emit an event
    pub fn emit(
        &self,
        event_type: EventType,
        source: impl Into<String>,
        data: serde_json::Value,
    ) {
        let event = Event {
            event_type,
            source: source.into(),
            data,
            timestamp: chrono::Utc::now(),
        };
        let _ = self.sender.send(event);
    }

    /// Get number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}
