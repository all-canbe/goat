//! 子 Agent 运行时
//!
//! 功能：
//! - 并行调度多个子 Agent
//! - 限制最大并发数和递归深度
//! - 聚合子 Agent 结果

use std::sync::Arc;
use tokio::sync::{Semaphore, Mutex};
use tracing::{debug, info};

use crate::agent::react::ReActAgent;
use crate::agent::types::{AgentError, AgentRunResult};
use crate::agent::task_graph::{TaskGraph, TaskNode, TaskStatus};

/// 子 Agent 任务
#[derive(Debug, Clone)]
pub struct SubAgentTask {
    pub id: String,
    pub prompt: String,
    pub context: String,
    pub depth: usize,
}

/// 子 Agent 结果
#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub task_id: String,
    pub result: Result<AgentRunResult, AgentError>,
    pub elapsed_ms: u64,
}

/// 子 Agent 运行时
pub struct SubAgentRuntime {
    agent: Arc<ReActAgent>,
    semaphore: Arc<Semaphore>,
    max_depth: usize,
    active_count: Arc<Mutex<usize>>,
}

impl SubAgentRuntime {
    pub fn new(agent: Arc<ReActAgent>) -> Self {
        let max_concurrent = agent.config.max_subagents;
        let max_depth = agent.config.max_subagent_depth;
        Self {
            agent,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_depth,
            active_count: Arc::new(Mutex::new(0)),
        }
    }

    /// 批量运行子 Agent 任务
    pub async fn run_batch(
        &self,
        tasks: Vec<SubAgentTask>,
        session_id: &str,
        workspace: &str,
    ) -> Vec<SubAgentResult> {
        let mut handles = Vec::new();

        for task in tasks {
            let agent = self.agent.clone();
            let semaphore = self.semaphore.clone();
            let active_count = self.active_count.clone();
            let max_depth = self.max_depth;
            let session_id = session_id.to_string();
            let workspace = workspace.to_string();
            let task_id = task.id.clone(); // 在 move 前克隆 task_id

            let handle = tokio::spawn(async move {
                let start = std::time::Instant::now();
                let result = if task.depth > max_depth {
                    Err(AgentError::Tool(format!(
                        "SubAgent depth {} exceeds maximum {}",
                        task.depth, max_depth
                    )))
                } else {
                    Self::run_single(
                        agent,
                        semaphore,
                        active_count,
                        task,
                        &session_id,
                        &workspace,
                    ).await
                };
                let elapsed_ms = start.elapsed().as_millis() as u64;
                SubAgentResult {
                    task_id: task_id.clone(),
                    result,
                    elapsed_ms,
                }
            });
            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(res) => {
                    results.push(res);
                }
                Err(e) => {
                    results.push(SubAgentResult {
                        task_id: "join_error".to_string(),
                        result: Err(AgentError::Tool(format!("Task join error: {}", e))),
                        elapsed_ms: 0,
                    });
                }
            }
        }

        results
    }

    async fn run_single(
        agent: Arc<ReActAgent>,
        semaphore: Arc<Semaphore>,
        active_count: Arc<Mutex<usize>>,
        task: SubAgentTask,
        session_id: &str,
        workspace: &str,
    ) -> Result<AgentRunResult, AgentError> {
        let _permit = semaphore.acquire().await
            .map_err(|e| AgentError::Tool(format!("Semaphore error: {}", e)))?;

        {
            let mut count = active_count.lock().await;
            *count += 1;
            info!("SubAgent {} started (active: {})", task.id, *count);
        }

        let full_prompt = format!(
            "## Parent Context\n{}\n\n## Your Task\n{}\n\nProvide a concise, actionable result.",
            task.context, task.prompt
        );

        // B5: 任务图持久化 — 写入 Running 状态
        let task_graph = match TaskGraph::new(workspace).await {
            Ok(tg) => Some(tg),
            Err(e) => {
                tracing::warn!("Failed to initialize task graph: {}", e);
                None
            }
        };
        let task_node = TaskNode {
            id: task.id.clone(),
            parent_id: session_id.to_string(),
            status: TaskStatus::Running,
            prompt: task.prompt.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            result_summary: None,
            depends_on: Vec::new(),
            blocks: Vec::new(),
        };
        if let Some(ref tg) = task_graph {
            if let Err(e) = tg.save(&task_node).await {
                tracing::warn!("Failed to save task node: {}", e);
            }
        }

        // 创建独立的子会话
        let sub_session = agent.conversation
            .create_session(Some(&format!("subagent-{}", task.id)), Some(workspace))
            .await
            .map_err(|e| AgentError::Tool(e.to_string()))?;

        // B1: 创建独立的子 Agent 实例 — 不共享父的 mode/approval_responder
        let sub_agent = agent.create_sub_agent();
        let result = sub_agent.run(&sub_session.id, &full_prompt, workspace).await;

        // B5: 更新任务图状态
        let (status, summary) = match &result {
            Ok(r) => (TaskStatus::Completed, Some(r.answer.chars().take(200).collect::<String>())),
            Err(e) => (TaskStatus::Failed, Some(e.to_string())),
        };
        if let Some(ref tg) = task_graph {
            if let Err(e) = tg.update_status(&task.id, status, summary).await {
                tracing::warn!("Failed to update task node status: {}", e);
            }
        }

        // B4: 子 Agent 完成后清理子会话，避免长期积累 subagent-session-* 记录
        if let Err(e) = agent.conversation.delete_session(&sub_session.id).await {
            tracing::warn!("Failed to clean up subagent session {}: {}", sub_session.id, e);
        }

        {
            let mut count = active_count.lock().await;
            *count = count.saturating_sub(1);
            debug!("SubAgent {} finished (active: {})", task.id, *count);
        }

        result
    }
}
