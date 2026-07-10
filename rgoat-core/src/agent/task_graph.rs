//! 任务图持久化 — 将子 Agent 任务节点写入文件系统
//!
//! 持久化目录：`<workspace>/.goat/task_graph/`
//! 每个任务节点为一个 JSON 文件，文件名为 `<task_id>.json`

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// 任务节点状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Completed,
    Failed,
}

/// 持久化的任务节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub parent_id: String,
    pub status: TaskStatus,
    pub prompt: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub result_summary: Option<String>,
    pub depends_on: Vec<String>,
    pub blocks: Vec<String>,
}

/// 任务图持久化管理器
pub struct TaskGraph {
    dir: PathBuf,
}

/// 校验 task ID 合法性，防止路径遍历
fn validate_id(id: &str) -> std::io::Result<()> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains(std::path::MAIN_SEPARATOR) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid task id: {}", id),
        ));
    }
    Ok(())
}

impl TaskGraph {
    /// 创建任务图管理器，确保目录存在
    pub async fn new(workspace: &str) -> std::io::Result<Self> {
        let dir = Path::new(workspace).join(".goat").join("task_graph");
        tokio::fs::create_dir_all(&dir).await?;
        Ok(Self { dir })
    }

    /// 写入任务节点（创建或更新）
    pub async fn save(&self, node: &TaskNode) -> std::io::Result<()> {
        validate_id(&node.id)?;
        let path = self.dir.join(format!("{}.json", node.id));
        let content = serde_json::to_string_pretty(node)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        tokio::fs::write(path, content).await
    }

    /// 更新任务状态
    pub async fn update_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        result_summary: Option<String>,
    ) -> std::io::Result<()> {
        validate_id(task_id)?;
        let path = self.dir.join(format!("{}.json", task_id));
        let content = tokio::fs::read_to_string(&path).await?;
        let mut node: TaskNode = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        node.status = status;
        node.completed_at = Some(chrono::Utc::now().to_rfc3339());
        node.result_summary = result_summary;
        self.save(&node).await
    }

    /// 列出所有任务节点
    pub async fn list(&self) -> Vec<TaskNode> {
        let mut nodes = Vec::new();
        if let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                    if let Ok(node) = serde_json::from_str::<TaskNode>(&content) {
                        nodes.push(node);
                    }
                }
            }
        }
        nodes
    }
}
