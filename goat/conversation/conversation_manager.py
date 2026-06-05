from __future__ import annotations

import json
import sqlite3
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

from .prompt_engine import engine as prompt_engine
from ..core.paths import CONVERSATIONS_DB

from langchain_core.messages import (
    BaseMessage, SystemMessage, HumanMessage, AIMessage, ToolMessage,
)

try:
    from .context_compression import (
        ContextManager as CompressionManager,
        CompressionMessage as CMessage,
        CompactionConfig,
    )
    _HAS_COMPRESSION = True
except ImportError:
    CompressionManager = None
    CMessage = None
    CompactionConfig = None
    _HAS_COMPRESSION = False


@dataclass
class SessionInfo:
    session_id: str
    title: str
    model: str
    created_at: str
    updated_at: str
    message_count: int
    token_count: int
    subagent_count: int


@dataclass
class MessageRecord:
    id: int
    session_id: str
    role: str
    content: str
    tool_call_id: str | None
    tool_name: str | None
    metadata: dict
    created_at: str


class ConversationManager:
    def __init__(self, db_path: str = "", max_tokens: int = 128000,
                 compression_config: Optional[CompactionConfig] = None):
        if not db_path:
            db_path = str(CONVERSATIONS_DB)
        self._db_path = Path(db_path)
        self._max_tokens = max_tokens
        self._current_session_id: str | None = None
        self._conn: sqlite3.Connection | None = None
        self._compression_manager: CompressionManager | None = None
        self._compressed_summary_text: str = ""
        if _HAS_COMPRESSION and compression_config is not None:
            self._compression_manager = CompressionManager(config=compression_config)
        self._init_db()

    @property
    def current_session_id(self) -> str | None:
        return self._current_session_id

    @property
    def max_tokens(self) -> int:
        return self._max_tokens

    def _get_conn(self) -> sqlite3.Connection:
        if self._conn is None:
            self._conn = sqlite3.connect(str(self._db_path))
            self._conn.row_factory = sqlite3.Row
            try:
                self._conn.execute("PRAGMA journal_mode=WAL")
            except sqlite3.OperationalError:
                self._conn.execute("PRAGMA journal_mode=DELETE")
        return self._conn

    def _init_db(self) -> None:
        conn = self._get_conn()
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                model TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                tool_call_id TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, id);
            CREATE TABLE IF NOT EXISTS checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                title TEXT NOT NULL,
                model TEXT NOT NULL DEFAULT '',
                message_count INTEGER DEFAULT 0,
                token_count INTEGER DEFAULT 0,
                created_at TEXT NOT NULL
            );
        """)
        conn.commit()
        self._migrate(conn)

    def _migrate(self, conn: sqlite3.Connection) -> None:
        self._add_column_if_missing(conn, "sessions", "title", "TEXT NOT NULL DEFAULT '新对话'")
        self._add_column_if_missing(conn, "sessions", "message_count", "INTEGER DEFAULT 0")
        self._add_column_if_missing(conn, "sessions", "token_count", "INTEGER DEFAULT 0")
        self._add_column_if_missing(conn, "sessions", "subagent_count", "INTEGER DEFAULT 0")
        self._add_column_if_missing(conn, "sessions", "metadata", "TEXT DEFAULT '{}'")
        self._add_column_if_missing(conn, "messages", "tool_name", "TEXT")
        self._add_column_if_missing(conn, "messages", "metadata", "TEXT DEFAULT '{}'")
        conn.commit()

    @staticmethod
    def _add_column_if_missing(conn: sqlite3.Connection, table: str, column: str, col_def: str) -> None:
        existing = {row[1] for row in conn.execute(f"PRAGMA table_info({table})").fetchall()}
        if column not in existing:
            conn.execute(f"ALTER TABLE {table} ADD COLUMN {column} {col_def}")

    async def create_session(self, model: str = "", title: str = "新对话",
                             session_id: str | None = None) -> str:
        if session_id is None:
            session_id = str(uuid.uuid4())
        now = datetime.now(timezone.utc).isoformat()
        conn = self._get_conn()
        conn.execute(
            "INSERT INTO sessions (session_id, title, model, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
            (session_id, title, model, now, now),
        )
        conn.commit()
        self._current_session_id = session_id
        if self._compression_manager is not None:
            self._compression_manager.reset()
        return session_id

    def load_session(self, session_id: str) -> bool:
        conn = self._get_conn()
        row = conn.execute(
            "SELECT session_id FROM sessions WHERE session_id = ?", (session_id,),
        ).fetchone()
        if row is None:
            return False
        self._current_session_id = session_id
        if self._compression_manager is not None:
            self._compression_manager.reset()
        self._compressed_summary_text = ""
        return True

    def get_messages(self, session_id: str | None = None) -> list[BaseMessage]:
        sid = session_id or self._current_session_id
        if sid is None:
            return []

        conn = self._get_conn()
        rows = conn.execute(
            "SELECT * FROM messages WHERE session_id = ? ORDER BY id ASC", (sid,),
        ).fetchall()

        messages: list[BaseMessage] = []

        if self._compressed_summary_text:
            messages.append(SystemMessage(content=self._compressed_summary_text))

        for row in rows:
            content = row["content"] or ""
            role = row["role"]
            if role == "system":
                messages.append(SystemMessage(content=content))
            elif role == "user":
                messages.append(HumanMessage(content=content))
            elif role == "assistant":
                messages.append(AIMessage(content=content))
            elif role == "tool":
                tc_id = row["tool_call_id"]
                messages.append(ToolMessage(content=content, tool_call_id=tc_id if tc_id else ""))

        return messages

    async def add_message(self, message: BaseMessage, session_id: str | None = None) -> str:
        sid = session_id or self._current_session_id
        if sid is None:
            return ""

        now = datetime.now(timezone.utc).isoformat()

        content = message.content if isinstance(message.content, str) else str(message.content)
        role = self._get_role(message)

        tool_call_id = ""
        if isinstance(message, ToolMessage):
            tool_call_id = message.tool_call_id or ""
        elif isinstance(message, AIMessage):
            if message.tool_calls:
                for tc in message.tool_calls:
                    tool_call_id = tc.get("id", "")
                    break

        metadata = {}
        if isinstance(message, AIMessage) and message.tool_calls:
            metadata["tool_calls"] = [
                {"id": tc.get("id", ""), "name": tc.get("name", ""), "args": tc.get("args", {})}
                for tc in message.tool_calls
            ]
        if isinstance(message, ToolMessage):
            metadata["tool_result"] = content
            if message.tool_call_id:
                metadata["tool_call_id"] = message.tool_call_id

        conn = self._get_conn()

        if role == "user":
            row = conn.execute(
                "SELECT message_count, title FROM sessions WHERE session_id = ?",
                (sid,),
            ).fetchone()
            if row and row["message_count"] == 0:
                default_titles = {"新对话", "Web 会话", "默认会话", "TUI 会话", ""}
                if row["title"] in default_titles:
                    clean = content.strip().replace("\n", " ")
                    new_title = clean[:50] + ("..." if len(clean) > 50 else "")
                    conn.execute(
                        "UPDATE sessions SET title = ? WHERE session_id = ?",
                        (new_title, sid),
                    )

        metadata_json = json.dumps(metadata, ensure_ascii=False)
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_call_id, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            (sid, role, content, tool_call_id, metadata_json, now),
        )
        conn.execute(
            "UPDATE sessions SET message_count = message_count + 1, updated_at = ? WHERE session_id = ?",
            (now, sid),
        )
        conn.commit()

        if self._compression_manager is not None:
            cmsg = CMessage(role=role, content=content)
            self._compression_manager.add_message(cmsg)

        return sid

    async def add_messages(self, messages: list[BaseMessage], session_id: str | None = None) -> str:
        last_sid = ""
        for msg in messages:
            last_sid = await self.add_message(msg, session_id)
        return last_sid

    def _get_role(self, message: BaseMessage) -> str:
        if isinstance(message, SystemMessage):
            return "system"
        elif isinstance(message, HumanMessage):
            return "user"
        elif isinstance(message, AIMessage):
            return "assistant"
        elif isinstance(message, ToolMessage):
            return "tool"
        return "user"

    def list_sessions(self, limit: int = 50, offset: int = 0, search: str = "") -> list[SessionInfo]:
        conn = self._get_conn()
        if search:
            rows = conn.execute(
                "SELECT * FROM sessions WHERE title LIKE ? ORDER BY updated_at DESC LIMIT ? OFFSET ?",
                (f"%{search}%", limit, offset),
            ).fetchall()
        else:
            rows = conn.execute(
                "SELECT * FROM sessions ORDER BY updated_at DESC LIMIT ? OFFSET ?",
                (limit, offset),
            ).fetchall()

        return [
            SessionInfo(
                session_id=row["session_id"],
                title=row["title"],
                model=row["model"],
                created_at=row["created_at"],
                updated_at=row["updated_at"],
                message_count=row["message_count"],
                token_count=row["token_count"],
                subagent_count=row["subagent_count"],
            )
            for row in rows
        ]

    def search_messages(self, keyword: str, limit: int = 20) -> list[MessageRecord]:
        conn = self._get_conn()
        rows = conn.execute(
            "SELECT * FROM messages WHERE content LIKE ? ORDER BY id DESC LIMIT ?",
            (f"%{keyword}%", limit),
        ).fetchall()
        return self._rows_to_records(rows)

    def get_session_messages(self, session_id: str, limit: int = 50) -> list[MessageRecord]:
        conn = self._get_conn()
        rows = conn.execute(
            "SELECT * FROM messages WHERE session_id = ? ORDER BY id DESC LIMIT ?",
            (session_id, limit),
        ).fetchall()
        return self._rows_to_records(rows)

    def _rows_to_records(self, rows: list[sqlite3.Row]) -> list[MessageRecord]:
        return [
            MessageRecord(
                id=row["id"],
                session_id=row["session_id"],
                role=row["role"],
                content=row["content"],
                tool_call_id=row["tool_call_id"],
                tool_name=row["tool_name"],
                metadata=json.loads(row["metadata"]) if isinstance(row["metadata"], str) else (row["metadata"] or {}),
                created_at=row["created_at"],
            )
            for row in rows
        ]

    def get_token_usage(self, session_id: str | None = None) -> dict:
        sid = session_id or self._current_session_id
        if sid is None:
            return {"message_count": 0, "total_tokens": 0, "max_tokens": self._max_tokens}

        conn = self._get_conn()
        row = conn.execute(
            "SELECT message_count, token_count FROM sessions WHERE session_id = ?",
            (sid,),
        ).fetchone()
        return {
            "message_count": row["message_count"] if row else 0,
            "total_tokens": row["token_count"] if row else 0,
            "max_tokens": self._max_tokens,
        }

    def rename_session(self, session_id: str, title: str) -> bool:
        conn = self._get_conn()
        cursor = conn.execute(
            "UPDATE sessions SET title = ?, updated_at = ? WHERE session_id = ?",
            (title, datetime.now(timezone.utc).isoformat(), session_id),
        )
        conn.commit()
        return cursor.rowcount > 0

    def resolve_session_id(self, partial_id: str) -> str | None:
        conn = self._get_conn()
        rows = conn.execute(
            "SELECT session_id FROM sessions WHERE session_id LIKE ?",
            (f"{partial_id}%",),
        ).fetchall()
        if len(rows) == 1:
            return rows[0]["session_id"]
        if len(rows) > 1:
            return None
        exact = conn.execute(
            "SELECT session_id FROM sessions WHERE session_id = ?",
            (partial_id,),
        ).fetchone()
        return exact["session_id"] if exact else None

    def delete_session(self, session_id: str) -> bool:
        conn = self._get_conn()
        conn.execute("DELETE FROM messages WHERE session_id = ?", (session_id,))
        cursor = conn.execute("DELETE FROM sessions WHERE session_id = ?", (session_id,))
        conn.commit()
        if self._current_session_id == session_id:
            sessions = self.list_sessions(limit=1)
            self._current_session_id = sessions[0].session_id if sessions else None
        return cursor.rowcount > 0

    def export_session(self, session_id: str, format: str = "text") -> str:
        messages = self.get_session_messages(session_id)
        sessions = self.list_sessions()
        session = next((s for s in sessions if s.session_id == session_id), None)
        if not messages:
            return "会话为空或不存在"

        if format == "json":
            data = {
                "session_id": session_id,
                "title": session.title if session else "",
                "messages": [
                    {
                        "id": m.id,
                        "role": m.role,
                        "content": m.content[:500],
                        "created_at": m.created_at,
                    }
                    for m in reversed(messages)
                ],
            }
            return json.dumps(data, ensure_ascii=False, indent=2)
        else:
            lines = [f"会话: {session.title if session else session_id}"]
            for m in reversed(messages):
                lines.append(f"\n[{m.role}] {m.created_at[:19]}")
                lines.append(m.content[:200])
            return "\n".join(lines)

    def save_checkpoint(self) -> str | None:
        sid = self._current_session_id
        if sid is None:
            return None

        conn = self._get_conn()
        row = conn.execute(
            "SELECT title, model, message_count, token_count FROM sessions WHERE session_id = ?",
            (sid,),
        ).fetchone()
        if row is None:
            return None

        now = datetime.now(timezone.utc).isoformat()
        check_id = str(uuid.uuid4())[:8]
        conn.execute(
            "INSERT INTO checkpoints (session_id, title, model, message_count, token_count, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            (sid, row["title"], row["model"], row["message_count"], row["token_count"], now),
        )
        conn.commit()
        return check_id

    def get_last_checkpoint(self) -> dict | None:
        conn = self._get_conn()
        row = conn.execute(
            "SELECT * FROM checkpoints ORDER BY rowid DESC LIMIT 1",
        ).fetchone()
        if row is None:
            return None
        return {
            "session_id": row["session_id"],
            "title": row["title"],
            "model": row["model"],
            "message_count": row["message_count"],
            "token_count": row["token_count"],
            "created_at": row["created_at"],
        }

    def clear_checkpoint(self) -> None:
        conn = self._get_conn()
        conn.execute("DELETE FROM checkpoints")
        conn.commit()

    async def compress_context(self) -> str:
        if self._compression_manager is None:
            return ""

        if not await self._compression_manager.is_compact_needed():
            return ""

        messages = await self._compression_manager.run_compact()
        self._compressed_summary_text = messages[0].content if messages else ""
        token_info = self._compression_manager.estimate_tokens()

        conn = self._get_conn()
        conn.execute(
            "UPDATE sessions SET token_count = ? WHERE session_id = ?",
            (token_info.total, self._current_session_id),
        )
        conn.commit()

        return self._compressed_summary_text

    async def inject_subagent_result(self, agent_id: str, agent_name: str,
                                     output: str, status: str) -> None:
        sid = self._current_session_id
        if sid is None:
            return

        now = datetime.now(timezone.utc).isoformat()
        content = f"[Agent {agent_name} ({agent_id})] {status}:\n{output[:500]}"

        conn = self._get_conn()
        conn.execute(
            "INSERT INTO messages (session_id, role, content, created_at) VALUES (?, ?, ?, ?)",
            (sid, "tool", content, now),
        )
        conn.execute(
            "UPDATE sessions SET subagent_count = subagent_count + 1, token_count = token_count + ?, updated_at = ? WHERE session_id = ?",
            (len(output) // 4, now, sid),
        )
        conn.commit()

    def get_session_info(self, session_id: str) -> SessionInfo | None:
        sessions = self.list_sessions()
        return next((s for s in sessions if s.session_id == session_id), None)

    def fork_session(self, source_session_id: str, title: str = "",
                     turn_number: int | None = None) -> str:
        new_id = str(uuid.uuid4())
        now = datetime.now(timezone.utc).isoformat()
        conn = self._get_conn()

        source = conn.execute(
            "SELECT model FROM sessions WHERE session_id = ?", (source_session_id,),
        ).fetchone()
        model = source["model"] if source else ""

        conn.execute(
            "INSERT INTO sessions (session_id, title, model, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
            (new_id, title, model, now, now),
        )

        if turn_number:
            source_msgs = conn.execute(
                "SELECT * FROM messages WHERE session_id = ? ORDER BY id ASC",
                (source_session_id,),
            ).fetchall()
            turn_count = 0
            for row in source_msgs:
                if row["role"] == "assistant" and row["tool_call_id"] is None:
                    turn_count += 1
                if turn_count > turn_number:
                    break
                conn.execute(
                    "INSERT INTO messages (session_id, role, content, tool_call_id, created_at) VALUES (?, ?, ?, ?, ?)",
                    (new_id, row["role"], row["content"], row["tool_call_id"], now),
                )
                conn.execute(
                    "UPDATE sessions SET message_count = message_count + 1 WHERE session_id = ?",
                    (new_id,),
                )
        else:
            conn.execute(
                "INSERT INTO messages (session_id, role, content, tool_call_id, created_at) SELECT ?, role, content, tool_call_id, created_at FROM messages WHERE session_id = ? ORDER BY id ASC",
                (new_id, source_session_id),
            )
            count_row = conn.execute(
                "SELECT COUNT(*) as cnt FROM messages WHERE session_id = ?", (new_id,),
            ).fetchone()
            if count_row:
                conn.execute(
                    "UPDATE sessions SET message_count = ? WHERE session_id = ?",
                    (count_row["cnt"], new_id),
                )

        conn.commit()
        return new_id

    def close(self) -> None:
        if self._conn is not None:
            self._conn.close()
            self._conn = None