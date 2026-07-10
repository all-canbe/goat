//! 任务持久化 — 将 ReAct 循环每步事件写入 JSONL，支持断点续传
//!
//! 持久化文件：`<workspace>/.goat/sessions/{session_id}/tasks.jsonl`
//! 每行一个 TaskEvent 的 JSON 对象（append-only）。
//! 读取时用于构建断点续传提示，注入到新一轮 run() 的上下文。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 任务事件状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 执行中（每步中间事件）
    Running,
    /// 已完成（最终成功）
    Completed,
    /// 失败（最终错误）
    Failed,
    /// 被中断（取消 / max_steps 耗尽）
    Interrupted,
}

/// 单步任务事件 — 持久化到 JSONL 的一行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    /// ReAct 循环步骤号
    pub step: usize,
    /// 本步调用的工具名（无工具调用时为 None）
    pub tool_name: Option<String>,
    /// 事件状态
    pub status: TaskStatus,
    /// 事件摘要（LLM 文本片段或最终结果描述）
    pub summary: String,
    /// 创建时间
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 任务持久化管理器 — append-only JSONL
pub struct TaskPersistence {
    file_path: PathBuf,
}

/// 校验 session_id 合法性，防止路径遍历（参考 task_graph.rs::validate_id）
fn validate_session_id(id: &str) -> std::io::Result<()> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains(std::path::MAIN_SEPARATOR)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid session id: {}", id),
        ));
    }
    Ok(())
}

impl TaskPersistence {
    /// 创建持久化管理器，确保目录 `.goat/sessions/{session_id}/` 存在
    pub async fn new(workspace: &str, session_id: &str) -> std::io::Result<Self> {
        validate_session_id(session_id)?;
        let dir = Path::new(workspace)
            .join(".goat")
            .join("sessions")
            .join(session_id);
        tokio::fs::create_dir_all(&dir).await?;
        Ok(Self {
            file_path: dir.join("tasks.jsonl"),
        })
    }

    /// 追加一行 JSONL 事件（append 模式，每行一个 JSON 对象）
    pub async fn append_event(&self, event: &TaskEvent) -> std::io::Result<()> {
        let mut line = serde_json::to_string(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        line.push('\n');
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file_path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        Ok(())
    }

    /// 读取所有事件（用于断点续传）。读取失败返回空 Vec（容错，不阻断）
    pub async fn load_events(&self) -> Vec<TaskEvent> {
        let mut events = Vec::new();
        let content = match tokio::fs::read_to_string(&self.file_path).await {
            Ok(c) => c,
            Err(_) => return events,
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<TaskEvent>(line) {
                events.push(event);
            }
        }
        events
    }

    /// 从事件历史构建断点续传提示文本。
    ///
    /// 格式："[断点续传] 上次执行到步骤 X，共执行 Y 次工具调用。
    ///        最近一次操作: 调用 <tool>。摘要: <snippet>。
    ///        请从步骤 X+1 继续，勿重复已完成的工作。"
    /// 无事件时返回 None。
    pub async fn build_resume_summary(&self) -> Option<String> {
        let events = self.load_events().await;
        if events.is_empty() {
            return None;
        }
        let last_step = events.iter().map(|e| e.step).max()?;
        // 统计有工具调用的事件数（每步中间事件 status=Running 且 tool_name 非空）
        let tool_call_count = events
            .iter()
            .filter(|e| e.tool_name.is_some())
            .count();
        // 取最后一步的事件作为最近操作
        let last_event = events
            .iter()
            .filter(|e| e.step == last_step)
            .last()?;

        let mut summary = format!(
            "[断点续传] 上次执行到步骤 {}，共执行 {} 次工具调用。",
            last_step, tool_call_count
        );
        if let Some(tool) = &last_event.tool_name {
            summary.push_str(&format!("最近一次操作: 调用 {}。", tool));
        }
        if !last_event.summary.is_empty() {
            let snippet: String = last_event.summary.chars().take(80).collect();
            summary.push_str(&format!("摘要: {}", snippet));
        }
        summary.push_str(&format!(
            "请从步骤 {} 继续，勿重复已完成的工作。",
            last_step + 1
        ));
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_session_id_rejects_invalid() {
        // 空、含路径分隔符、含 ".." 的 ID 应被拒绝
        assert!(validate_session_id("").is_err());
        assert!(validate_session_id("a/b").is_err());
        assert!(validate_session_id("a\\b").is_err());
        assert!(validate_session_id("..").is_err());
        assert!(validate_session_id("a..b").is_err());
        // 合法 ID 应通过
        assert!(validate_session_id("a").is_ok());
        assert!(validate_session_id("session-123").is_ok());
        assert!(validate_session_id("abc.def").is_ok());
    }

    #[tokio::test]
    async fn append_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().to_str().unwrap();
        let p = TaskPersistence::new(ws, "sess-rt").await.unwrap();
        let now = chrono::Utc::now();
        let e1 = TaskEvent {
            step: 0,
            tool_name: Some("read_file".to_string()),
            status: TaskStatus::Running,
            summary: "read config".to_string(),
            created_at: now,
        };
        let e2 = TaskEvent {
            step: 1,
            tool_name: Some("write_file".to_string()),
            status: TaskStatus::Running,
            summary: "wrote output".to_string(),
            created_at: now,
        };
        p.append_event(&e1).await.unwrap();
        p.append_event(&e2).await.unwrap();

        let loaded = p.load_events().await;
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].step, 0);
        assert_eq!(loaded[0].tool_name.as_deref(), Some("read_file"));
        assert_eq!(loaded[1].step, 1);
        assert_eq!(loaded[1].summary, "wrote output");
    }

    #[tokio::test]
    async fn build_resume_summary_format() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().to_str().unwrap();
        let p = TaskPersistence::new(ws, "sess-rs").await.unwrap();
        let now = chrono::Utc::now();
        p.append_event(&TaskEvent {
            step: 5,
            tool_name: Some("read_file".to_string()),
            status: TaskStatus::Running,
            summary: "读取了配置文件".to_string(),
            created_at: now,
        }).await.unwrap();

        let summary = p.build_resume_summary().await.unwrap();
        assert!(summary.contains("步骤 5"), "应包含上次步骤号");
        assert!(summary.contains("1 次工具调用"), "应包含工具调用次数");
        assert!(summary.contains("read_file"), "应包含最近工具名");
        assert!(summary.contains("步骤 6"), "应提示下一步");
    }

    #[tokio::test]
    async fn build_resume_summary_empty_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().to_str().unwrap();
        let p = TaskPersistence::new(ws, "sess-empty").await.unwrap();
        assert!(p.build_resume_summary().await.is_none());
    }
}
