//! Checkpoint 看门狗 — 定期保存会话进度，支持崩溃检测与恢复
//!
//! 持久化文件：`<workspace>/.goat/sessions/{session_id}/checkpoint.json`
//! 每次 write 覆盖写入最新 checkpoint（非 append），仅保留最新状态。
//! 启动时通过 detect_any_crash 扫描所有会话目录，检测 status == Running
//! 的 checkpoint（视为上次异常终止）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Checkpoint 状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointStatus {
    /// 执行中（会话进行中写入的中间 checkpoint）
    Running,
    /// 已完成（会话正常结束时写入）
    Completed,
    /// 已崩溃（保留变体，当前由 Running 状态推断崩溃）
    Crashed,
}

/// Checkpoint 数据 — 序列化为 JSON 覆盖写入
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointData {
    /// 会话 ID
    pub session_id: String,
    /// 当前 ReAct 循环步骤号
    pub step: usize,
    /// checkpoint 状态
    pub status: CheckpointStatus,
    /// 当前进度摘要（StepsTracker 进度或 LLM 文本片段）
    pub progress_summary: String,
    /// ISO 8601 时间戳
    pub timestamp: String,
}

/// Checkpoint 管理器 — 覆盖写入单个 JSON 文件
pub struct Checkpoint {
    file_path: PathBuf,
}

/// 校验 session_id 合法性，防止路径遍历（与 task_persistence.rs 一致）
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

impl Checkpoint {
    /// 创建 checkpoint 管理器，确保目录 `.goat/sessions/{session_id}/` 存在
    pub async fn new(workspace: &str, session_id: &str) -> std::io::Result<Self> {
        validate_session_id(session_id)?;
        let dir = Path::new(workspace)
            .join(".goat")
            .join("sessions")
            .join(session_id);
        tokio::fs::create_dir_all(&dir).await?;
        Ok(Self {
            file_path: dir.join("checkpoint.json"),
        })
    }

    /// 覆盖写入 checkpoint（非 append，每次只保留最新）
    pub async fn write(&self, data: &CheckpointData) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        tokio::fs::write(&self.file_path, json).await?;
        Ok(())
    }

    /// 读取上次 checkpoint（文件不存在返回 None）
    pub async fn read(&self) -> Option<CheckpointData> {
        let content = tokio::fs::read_to_string(&self.file_path).await.ok()?;
        serde_json::from_str::<CheckpointData>(&content).ok()
    }

    /// 读取 checkpoint，若 status == Running 则视为上次崩溃（返回 Some 供 UI 提示）
    pub async fn detect_crash(&self) -> Option<CheckpointData> {
        let data = self.read().await?;
        if data.status == CheckpointStatus::Running {
            Some(data)
        } else {
            None
        }
    }

    /// 写入 Completed 状态的最终 checkpoint（保留已有步骤号）
    pub async fn mark_completed(&self, session_id: &str) -> std::io::Result<()> {
        let step = self.read().await.map(|d| d.step).unwrap_or(0);
        let data = CheckpointData {
            session_id: session_id.to_string(),
            step,
            status: CheckpointStatus::Completed,
            progress_summary: "Session completed".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        self.write(&data).await
    }

    /// 扫描所有会话目录，检测是否有崩溃（status == Running）的 checkpoint。
    /// 用于 TUI 启动时检测上次会话是否异常终止。
    pub async fn detect_any_crash(workspace: &str) -> Option<CheckpointData> {
        let sessions_dir = Path::new(workspace).join(".goat").join("sessions");
        let mut entries = tokio::fs::read_dir(&sessions_dir).await.ok()?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let cp_path = entry.path().join("checkpoint.json");
            if let Ok(content) = tokio::fs::read_to_string(&cp_path).await {
                if let Ok(data) = serde_json::from_str::<CheckpointData>(&content) {
                    if data.status == CheckpointStatus::Running {
                        return Some(data);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_session_id_rejects_invalid() {
        assert!(validate_session_id("").is_err());
        assert!(validate_session_id("a/b").is_err());
        assert!(validate_session_id("a\\b").is_err());
        assert!(validate_session_id("..").is_err());
        assert!(validate_session_id("a..b").is_err());
        assert!(validate_session_id("a").is_ok());
        assert!(validate_session_id("session-123").is_ok());
        assert!(validate_session_id("abc.def").is_ok());
    }

    #[tokio::test]
    async fn write_and_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().to_str().unwrap();
        let cp = Checkpoint::new(ws, "sess-rt").await.unwrap();

        let data = CheckpointData {
            session_id: "sess-rt".to_string(),
            step: 5,
            status: CheckpointStatus::Running,
            progress_summary: "正在实现 checkpoint 模块".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        cp.write(&data).await.unwrap();

        let loaded = cp.read().await.unwrap();
        assert_eq!(loaded.session_id, "sess-rt");
        assert_eq!(loaded.step, 5);
        assert_eq!(loaded.status, CheckpointStatus::Running);
        assert_eq!(loaded.progress_summary, "正在实现 checkpoint 模块");

        // 覆盖写入：新数据应替换旧数据
        let data2 = CheckpointData {
            session_id: "sess-rt".to_string(),
            step: 10,
            status: CheckpointStatus::Completed,
            progress_summary: "已完成".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        cp.write(&data2).await.unwrap();
        let loaded2 = cp.read().await.unwrap();
        assert_eq!(loaded2.step, 10);
        assert_eq!(loaded2.status, CheckpointStatus::Completed);
    }

    #[tokio::test]
    async fn detect_crash_logic() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().to_str().unwrap();
        let cp = Checkpoint::new(ws, "sess-crash").await.unwrap();

        // 无 checkpoint 文件 → None
        assert!(cp.detect_crash().await.is_none());

        // status == Running → 视为崩溃
        cp.write(&CheckpointData {
            session_id: "sess-crash".to_string(),
            step: 7,
            status: CheckpointStatus::Running,
            progress_summary: "执行中".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }).await.unwrap();
        let crashed = cp.detect_crash().await.unwrap();
        assert_eq!(crashed.step, 7);
        assert_eq!(crashed.status, CheckpointStatus::Running);

        // status == Completed → 非崩溃
        cp.write(&CheckpointData {
            session_id: "sess-crash".to_string(),
            step: 7,
            status: CheckpointStatus::Completed,
            progress_summary: "已完成".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }).await.unwrap();
        assert!(cp.detect_crash().await.is_none());

        // mark_completed 后 status == Completed → 非崩溃
        cp.write(&CheckpointData {
            session_id: "sess-crash".to_string(),
            step: 7,
            status: CheckpointStatus::Running,
            progress_summary: "执行中".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }).await.unwrap();
        cp.mark_completed("sess-crash").await.unwrap();
        assert!(cp.detect_crash().await.is_none());
    }

    #[tokio::test]
    async fn detect_any_crash_scans_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().to_str().unwrap();

        // 会话 A：正常完成
        let cp_a = Checkpoint::new(ws, "sess-a").await.unwrap();
        cp_a.write(&CheckpointData {
            session_id: "sess-a".to_string(),
            step: 3,
            status: CheckpointStatus::Completed,
            progress_summary: "done".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }).await.unwrap();

        // 会话 B：崩溃（Running）
        let cp_b = Checkpoint::new(ws, "sess-b").await.unwrap();
        cp_b.write(&CheckpointData {
            session_id: "sess-b".to_string(),
            step: 8,
            status: CheckpointStatus::Running,
            progress_summary: "crashed here".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }).await.unwrap();

        let crashed = Checkpoint::detect_any_crash(ws).await.unwrap();
        assert_eq!(crashed.status, CheckpointStatus::Running);
        assert_eq!(crashed.session_id, "sess-b");

        // 标记 B 完成后，无崩溃
        cp_b.mark_completed("sess-b").await.unwrap();
        assert!(Checkpoint::detect_any_crash(ws).await.is_none());
    }
}
