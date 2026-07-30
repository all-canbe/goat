//! RGoat Core — lightweight AI coding assistant engine
//!
//! Rebuilt from Python Goat in pure Rust with enhanced capabilities:
//! - Zvec vector memory for semantic code search and conversation recall
//! - Candle local embedding for offline-first operation
//! - ReAct agent loop with multi-layer approval
//! - SubAgent parallel scheduling
//! - Flow pipeline (implement → review → fix)

pub mod core;
pub mod provider;
pub mod tools;
pub mod security;
pub mod conversation;
pub mod memory;
pub mod agent;
pub mod mcp;
pub mod hooks;
pub mod tasks;
pub mod cli;

// Re-export commonly used types
pub use core::event_bus::{EventBus, Event, EventType};
pub use core::cancellation::CancellationToken;
pub use core::workspace::{Workspace, resolve_workspace, get_data_dir};
pub use core::config::Settings;
pub use provider::provider::{LlmProvider, ProviderConfig, ProviderType};
pub use provider::impls::{OpenAiCompatibleProvider, AnthropicProvider};
pub use provider::switch::ProviderSwitch;
pub use security::approval::{ApprovalEngine, AgentMode, Decision, ToolCategory, ApprovalDecision, ApprovalResponder, ApprovalScope, SessionCacheKey, calculate_danger_score};
pub use security::sandbox::{Sandbox, SandboxLevel, SandboxError, create_sandbox};
pub use tools::registry::{ToolRegistry, Tool, ToolResult, ToolStreamEvent, ToolExecutionContext};
pub use tools::ask_user::AskUserTool;
pub use conversation::manager::ConversationManager;
pub use memory::vector_store::VectorMemory;
pub use agent::react::ReActAgent;
pub use agent::subagent::{SubAgentRuntime, SubAgentTask, SubAgentResult};
pub use agent::flow::{FlowPipeline, FlowResult, ReviewFinding, PlanFirstResult, GatePhase, GateInfo, GateCallback};
pub use agent::plan_runner::{PlanRunner, PlanResult, PlanPhase};
pub use agent::types::AgentConfig;
pub use mcp::client::{McpClient, StdioMcpClient, McpToolAdapter};
pub use mcp::server::{McpServer, GoatMcpServer, run_stdio_server};
pub use tasks::TaskScheduler;
pub use cli::{parse_args, parse_interactive, CliCommand};

/// Current version of rgoat-core
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
