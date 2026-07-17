//! 对话管理器 — SQLite 持久化
//!
//! 功能：
//! - 会话（Session）CRUD
//! - 消息（Message）CRUD
//! - 会话搜索与导出
//! - Fork 会话

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use uuid::Uuid;

use crate::core::workspace::get_data_dir;

// ============================================================================
// 数据模型
// ============================================================================

/// 会话信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: i64,
    pub workspace: Option<String>,
}

/// 会话详情（含消息）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub info: SessionInfo,
    pub messages: Vec<MessageRecord>,
}

/// 消息记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>, // JSON
    pub tool_call_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// 管理器
// ============================================================================

pub struct ConversationManager {
    pool: SqlitePool,
}

impl ConversationManager {
    /// 创建管理器并初始化数据库（默认路径：工作区/.goat/conversations.db）
    pub async fn new() -> Result<Self, ConversationError> {
        // 优先使用工作区目录（兼容沙箱限制）
        let cwd = std::env::current_dir().unwrap_or_default();
        let workspace_db = if !cwd.as_os_str().is_empty() {
            let dir = cwd.join(".goat");
            std::fs::create_dir_all(&dir).ok();
            Some(dir.join("conversations.db"))
        } else {
            None
        };

        // 尝试工作区路径，回退到 data_dir
        if let Some(db) = &workspace_db {
            let result = Self::new_with_path(db).await;
            if result.is_ok() {
                return result;
            }
        }

        let db_path = get_data_dir().join("conversations.db");
        Self::new_with_path(&db_path).await
    }

    /// 使用自定义路径创建管理器（用于测试/隔离环境）
    pub async fn new_with_path(db_path: &std::path::Path) -> Result<Self, ConversationError> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 使用 sqlite:/// URI + create_if_missing（避免 ?mode=rwc 不被 sqlx 支持）
        let uri = format!(
            "sqlite:///{}",
            db_path.display().to_string().replace('\\', "/")
        );
        let opts = SqliteConnectOptions::from_str(&uri)
            .unwrap_or_else(|_| SqliteConnectOptions::new().filename(":memory:"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        // Create tables
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT 'New Session',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                workspace TEXT
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                tool_calls TEXT,
                tool_call_id TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Create index on session_id to accelerate WHERE session_id = ? lookups
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)",
        )
        .execute(&pool)
        .await?;

        // Enable WAL mode for better concurrent read performance
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;

        // Set busy timeout to 5 seconds — allows multiple processes (TUI + Desktop) to
        // coexist by waiting instead of immediately failing with SQLITE_BUSY
        sqlx::query("PRAGMA busy_timeout=5000")
            .execute(&pool)
            .await?;

        // Enable foreign keys
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await?;

        Ok(Self { pool })
    }

    // ========================================================
    // Session CRUD
    // ========================================================

    /// Create a new session
    pub async fn create_session(&self, title: Option<&str>, workspace: Option<&str>) -> Result<SessionInfo, ConversationError> {
        let id = Uuid::new_v4().to_string();
        let title = title.unwrap_or("New Session");
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO sessions (id, title, created_at, updated_at, workspace) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(title)
        .bind(&now)
        .bind(&now)
        .bind(workspace)
        .execute(&self.pool)
        .await?;

        Ok(SessionInfo {
            id,
            title: title.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            message_count: 0,
            workspace: workspace.map(|s| s.to_string()),
        })
    }

    /// Get an existing session by ID, or create a new one if it doesn't exist.
    /// This is useful for callers (like the Desktop app) that need to use a
    /// stable session ID supplied by the frontend.
    pub async fn get_or_create_session(
        &self,
        session_id: &str,
        title: Option<&str>,
        workspace: Option<&str>,
    ) -> Result<SessionInfo, ConversationError> {
        if let Some(existing) = self.get_session(session_id).await? {
            return Ok(existing);
        }

        let title = title.unwrap_or("New Session");
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO sessions (id, title, created_at, updated_at, workspace) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(title)
        .bind(&now)
        .bind(&now)
        .bind(workspace)
        .execute(&self.pool)
        .await?;

        Ok(SessionInfo {
            id: session_id.to_string(),
            title: title.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            message_count: 0,
            workspace: workspace.map(|s| s.to_string()),
        })
    }
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>, ConversationError> {
        let rows = sqlx::query_as::<_, SessionRow>(
            r#"
            SELECT s.id, s.title, s.created_at, s.updated_at, s.workspace,
                   COUNT(m.id) as message_count
            FROM sessions s
            LEFT JOIN messages m ON s.id = m.session_id
            GROUP BY s.id
            ORDER BY s.updated_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Get a session by ID
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionInfo>, ConversationError> {
        let row = sqlx::query_as::<_, SessionRow>(
            r#"
            SELECT s.id, s.title, s.created_at, s.updated_at, s.workspace,
                   COUNT(m.id) as message_count
            FROM sessions s
            LEFT JOIN messages m ON s.id = m.session_id
            WHERE s.id = ?
            GROUP BY s.id
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.into()))
    }

    /// Update session title
    pub async fn update_title(&self, session_id: &str, title: &str) -> Result<(), ConversationError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE sessions SET title = ?, updated_at = ? WHERE id = ?")
            .bind(title)
            .bind(&now)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<(), ConversationError> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Fork a session (copy messages up to a point)
    pub async fn fork_session(
        &self,
        session_id: &str,
        new_title: Option<&str>,
        up_to_message_id: Option<i64>,
    ) -> Result<SessionInfo, ConversationError> {
        let source = self.get_session(session_id)
            .await?
            .ok_or(ConversationError::NotFound(session_id.to_string()))?;

        let fallback_title = format!("Fork of {}", source.title);
        let title = new_title.unwrap_or(&fallback_title);
        let new_session = self.create_session(Some(title), source.workspace.as_deref()).await?;

        // Copy messages
        let messages = self.get_messages(session_id).await?;
        for msg in messages {
            if let Some(up_to) = up_to_message_id {
                if msg.id > up_to {
                    break;
                }
            }
            self.add_message(
                &new_session.id,
                &msg.role,
                &msg.content,
                msg.tool_calls.as_deref(),
                msg.tool_call_id.as_deref(),
            ).await?;
        }

        Ok(new_session)
    }

    // ========================================================
    // Message CRUD
    // ========================================================

    /// Add a message to a session
    pub async fn add_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        tool_calls: Option<&str>,
        tool_call_id: Option<&str>,
    ) -> Result<i64, ConversationError> {
        let now = Utc::now().to_rfc3339();

        let id = sqlx::query(
            "INSERT INTO messages (session_id, role, content, tool_calls, tool_call_id, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(tool_calls)
        .bind(tool_call_id)
        .bind(&now)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        // Update session timestamp
        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .execute(&self.pool)
            .await?;

        Ok(id)
    }

    /// Get all messages for a session
    pub async fn get_messages(&self, session_id: &str) -> Result<Vec<MessageRecord>, ConversationError> {
        let messages = sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, role, content, tool_calls, tool_call_id, created_at FROM messages WHERE session_id = ? ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(messages.into_iter().map(|r| r.into()).collect())
    }

    /// Get recent N messages
    pub async fn get_recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<MessageRecord>, ConversationError> {
        let messages = sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, role, content, tool_calls, tool_call_id, created_at FROM messages WHERE session_id = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut messages: Vec<MessageRecord> = messages.into_iter().map(|r| r.into()).collect();
        messages.reverse(); // Restore chronological order
        Ok(messages)
    }

    /// Replace all messages in a session with the given list (用于压缩后持久化)
    pub async fn replace_messages(
        &self,
        session_id: &str,
        messages: &[MessageRecord],
    ) -> Result<(), ConversationError> {
        let mut tx = self.pool.begin().await?;

        // 先删除所有旧消息
        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        // 批量插入新消息
        for msg in messages {
            let created_at = msg.created_at.to_rfc3339();
            sqlx::query(
                "INSERT INTO messages (session_id, role, content, tool_calls, tool_call_id, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(session_id)
            .bind(&msg.role)
            .bind(&msg.content)
            .bind(msg.tool_calls.as_deref())
            .bind(msg.tool_call_id.as_deref())
            .bind(&created_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Delete messages from a session
    pub async fn delete_messages(&self, session_id: &str, before_id: i64) -> Result<u64, ConversationError> {
        let result = sqlx::query("DELETE FROM messages WHERE session_id = ? AND id < ?")
            .bind(session_id)
            .bind(before_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Search messages across sessions
    pub async fn search_messages(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(MessageRecord, String)>, ConversationError> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, SearchRow>(
            r#"
            SELECT m.id, m.session_id, m.role, m.content, m.tool_calls, m.tool_call_id, m.created_at, s.title as session_title
            FROM messages m
            JOIN sessions s ON m.session_id = s.id
            WHERE m.content LIKE ?
            ORDER BY m.created_at DESC
            LIMIT ?
            "#,
        )
        .bind(&pattern)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let msg = MessageRecord {
                    id: r.id,
                    session_id: r.session_id,
                    role: r.role,
                    content: r.content,
                    tool_calls: r.tool_calls,
                    tool_call_id: r.tool_call_id,
                    created_at: DateTime::parse_from_rfc3339(&r.created_at)
                        .unwrap()
                        .with_timezone(&Utc),
                };
                (msg, r.session_title)
            })
            .collect())
    }

    /// Get total message count for a session
    pub async fn message_count(&self, session_id: &str) -> Result<i64, ConversationError> {
        let row = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }
}

// ============================================================================
// SQL row types
// ============================================================================

#[derive(Debug, sqlx::FromRow)]
struct SessionRow {
    id: String,
    title: String,
    created_at: String,
    updated_at: String,
    workspace: Option<String>,
    message_count: i64,
}

impl From<SessionRow> for SessionInfo {
    fn from(row: SessionRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            created_at: DateTime::parse_from_rfc3339(&row.created_at)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&row.updated_at)
                .unwrap()
                .with_timezone(&Utc),
            message_count: row.message_count,
            workspace: row.workspace,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct MessageRow {
    id: i64,
    session_id: String,
    role: String,
    content: String,
    tool_calls: Option<String>,
    tool_call_id: Option<String>,
    created_at: String,
}

impl From<MessageRow> for MessageRecord {
    fn from(row: MessageRow) -> Self {
        Self {
            id: row.id,
            session_id: row.session_id,
            role: row.role,
            content: row.content,
            tool_calls: row.tool_calls,
            tool_call_id: row.tool_call_id,
            created_at: DateTime::parse_from_rfc3339(&row.created_at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SearchRow {
    id: i64,
    session_id: String,
    role: String,
    content: String,
    tool_calls: Option<String>,
    tool_call_id: Option<String>,
    created_at: String,
    session_title: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Session not found: {0}")]
    NotFound(String),
}
