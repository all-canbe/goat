//! Task tool — sub-agent launcher
//!
//! Launches a sub-agent to execute complex tasks autonomously and
//! returns aggregated results in Claude Code-style compact format.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use super::registry::{Tool, ToolResult};
use crate::agent::subagent::{SubAgentRuntime, SubAgentTask};
use crate::core::cancellation::CancellationToken;
use crate::security::approval::ToolCategory;

/// TaskTool — launches a sub-agent with its own ReAct loop
pub struct TaskTool {
    runtime: Arc<SubAgentRuntime>,
    cancellation: CancellationToken,
    workspace: std::path::PathBuf,
}

impl TaskTool {
    /// Create a new TaskTool backed by a sub-agent runtime
    pub fn new(
        runtime: Arc<SubAgentRuntime>,
        cancellation: CancellationToken,
        workspace: std::path::PathBuf,
    ) -> Self {
        Self {
            runtime,
            cancellation,
            workspace,
        }
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn name(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        "Launch a sub-agent to execute a complex task autonomously. Returns the sub-agent's result."
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "subagent_name": {
                    "type": "string",
                    "description": "Name identifier for the sub-agent"
                },
                "description": {
                    "type": "string",
                    "description": "Short description of the sub-agent's purpose"
                },
                "prompt": {
                    "type": "string",
                    "description": "The task for the sub-agent to execute"
                },
                "context": {
                    "type": "string",
                    "description": "Optional context from the parent agent"
                },
                "max_turns": {
                    "type": "integer",
                    "description": "Maximum number of ReAct turns for the sub-agent (honored via AgentConfig)"
                },
                "context_files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Files for the sub-agent to read before starting the task"
                }
            },
            "required": ["subagent_name", "description", "prompt"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        // Honour cancellation before spawning
        if self.cancellation.is_cancelled() {
            return ToolResult::error("Task cancelled before sub-agent launch", "cancelled");
        }

        let subagent_name = args["subagent_name"]
            .as_str()
            .unwrap_or("subagent")
            .to_string();
        let _description = args["description"].as_str().unwrap_or("").to_string();
        let prompt = args["prompt"].as_str().unwrap_or("").to_string();
        let context = args["context"].as_str().unwrap_or("").to_string();

        let context_files: Vec<String> = args["context_files"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        if prompt.is_empty() {
            return ToolResult::error("prompt is required for sub-agent task", "missing_prompt");
        }

        // Generate unique identifiers for the sub-agent session
        let task_id = uuid::Uuid::new_v4().to_string();
        let session_id = format!("subagent-session-{}", uuid::Uuid::new_v4());

        // Read context_files and append their contents to the context
        let mut full_context = context.clone();
        if !context_files.is_empty() {
            let mut files_content = String::new();
            for file_path in &context_files {
                // 使用 safe_path 解析路径，并用 is_safe_path 校验是否在 workspace 内，
                // 防止 context_files 通过 ../ 等方式遍历到 workspace 之外
                let resolved = crate::core::paths::safe_path(file_path, &self.workspace);
                if !crate::core::paths::is_safe_path(&resolved, &self.workspace) {
                    files_content.push_str(&format!(
                        "\n\n--- File: {} (path error: outside workspace) ---\n",
                        file_path
                    ));
                    continue;
                }
                match tokio::fs::read_to_string(&resolved).await {
                    Ok(content) => {
                        files_content
                            .push_str(&format!("\n\n--- File: {} ---\n{}\n", file_path, content));
                    }
                    Err(e) => {
                        files_content
                            .push_str(&format!("\n\n--- File: {} (read error: {}) ---\n", file_path, e));
                    }
                }
            }
            if !files_content.is_empty() {
                full_context = format!("{}\n## Context Files{}", context, files_content);
            }
        }

        let task = SubAgentTask {
            id: task_id.clone(),
            prompt,
            context: full_context,
            depth: 1,
        };

        let workspace = self.workspace.to_string_lossy().to_string();
        let results = self
            .runtime
            .run_batch(vec![task], &session_id, &workspace)
            .await;

        match results.into_iter().next() {
            Some(sub_result) => {
                let status = if sub_result.result.is_ok() {
                    "success"
                } else {
                    "error"
                };

                let (answer, steps, tool_calls) = match &sub_result.result {
                    Ok(r) => (r.answer.clone(), r.steps_taken, r.tool_calls),
                    Err(e) => (format!("Error: {}", e), 0usize, 0usize),
                };

                let output = format!(
                    "## SubAgent Result: {}\n\
                     **Status**: {}\n\
                     **Turns**: {}\n\
                     **Tool calls**: {}\n\
                     **Elapsed**: {}ms\n\n\
                     ### Output\n\
                     {}",
                    subagent_name, status, steps, tool_calls, sub_result.elapsed_ms, answer
                );

                let metadata = json!({
                    "subagent_name": subagent_name,
                    "task_id": task_id,
                    "status": status,
                    "elapsed_ms": sub_result.elapsed_ms,
                    "steps": steps,
                    "tool_calls": tool_calls,
                });

                ToolResult::success(output).with_metadata(metadata)
            }
            None => ToolResult::error(
                "Sub-agent produced no results (empty batch)".to_string(),
                "no_results",
            ),
        }
    }
}
