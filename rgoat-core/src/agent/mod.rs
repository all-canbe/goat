//! Agent 编排 — ReAct 循环、子Agent管理、Flow 流水线
//!
//! 模块：
//! - `types`：Agent 运行时类型与配置
//! - `react`：ReAct 主循环
//! - `subagent`：子 Agent 并行调度
//! - `flow`：实现→审查→修复流水线

pub mod types;
pub mod react;
pub mod subagent;
pub mod flow;
pub mod plan_runner;
pub mod steps_tracker;
pub mod task_graph;
pub mod task_persistence;
pub mod planner;
pub mod checkpoint;

pub use types::{AgentConfig, AgentError, AgentEvent, AgentRunResult, ParsedToolCall, StepResult};
pub use react::ReActAgent;
pub use subagent::{SubAgentRuntime, SubAgentTask, SubAgentResult};
pub use flow::{FlowPipeline, FlowResult, ReviewFinding, PlanFirstResult, GatePhase, GateInfo, GateCallback};
pub use plan_runner::{PlanRunner, PlanResult, PlanPhase};
pub use task_graph::{TaskGraph, TaskNode, TaskStatus};
// 注：task_persistence::TaskStatus 不在顶层 re-export，避免与 task_graph::TaskStatus 冲突。
// 通过 crate::agent::task_persistence::TaskStatus 路径访问（含 Interrupted 变体）。
pub use task_persistence::{TaskPersistence, TaskEvent};
pub use planner::{Planner, ExecutionPlan, PlanStep, ActionType, Complexity};
pub use checkpoint::{Checkpoint, CheckpointData, CheckpointStatus};
