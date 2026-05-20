from __future__ import annotations

import json
import sqlite3
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

from langchain_core.messages import (
    BaseMessage, SystemMessage, HumanMessage, AIMessage, ToolMessage,
)


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
    def __init__(self, db_path: str = "conversations.db", max_tokens: int = 128000):
        self._db_path = Path(db_path)
        self._max_tokens = max_tokens
        self._current_session_id: str | None = None
        self._conn: sqlite3.Connection | None = None
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
            self._conn.execute("PRAGMA journal_mode=WAL")
        return self._conn

    def _init_db(self) -> None:
        conn = self._get_conn()
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '新对话',
                model TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                message_count INTEGER DEFAULT 0,
                token_count INTEGER DEFAULT 0,
                subagent_count INTEGER DEFAULT 0,
                metadata TEXT DEFAULT '{}'
            );
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                tool_call_id TEXT,
                tool_name TEXT,
                name TEXT,
                metadata TEXT DEFAULT '{}',
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(session_id)
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session_id
                ON messages(session_id, id);
            CREATE INDEX IF NOT EXISTS idx_sessions_updated
                ON sessions(updated_at DESC);
        """)
        conn.commit()

    async def create_session(self, model: str = "", title: str = "新对话") -> str:
        session_id = str(uuid.uuid4())
        now = datetime.now(timezone.utc).isoformat()
        conn = self._get_conn()
        conn.execute(
            "INSERT INTO sessions (session_id, title, model, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
            (session_id, title, model, now, now),
        )
        conn.commit()
        self._current_session_id = session_id
        return session_id

    async def add_message(self, message: BaseMessage) -> int:
        session_id = self._ensure_session()
        now = datetime.now(timezone.utc).isoformat()

        role = _map_role(message.type)
        metadata = {}
        if hasattr(message, "additional_kwargs") and message.additional_kwargs:
            metadata = message.additional_kwargs

        tool_call_id = None
        tool_name = None
        if isinstance(message, ToolMessage):
            tool_call_id = message.tool_call_id
        if hasattr(message, "name"):
            tool_name = message.name

        conn = self._get_conn()
        cursor = conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_call_id, tool_name, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (session_id, role, message.content or "", tool_call_id, tool_name, json.dumps(metadata, ensure_ascii=False), now),
        )
        msg_id = cursor.lastrowid

        token_count = _estimate_tokens(message.content or "")
        conn.execute(
            "UPDATE sessions SET updated_at = ?, message_count = message_count + 1, token_count = token_count + ? WHERE session_id = ?",
            (now, token_count, session_id),
        )
        conn.commit()
        return msg_id

    def get_context(self, session_id: str | None = None) -> list[BaseMessage]:
        sid = session_id or self._current_session_id
        if sid is None:
            return []

        conn = self._get_conn()
        rows = conn.execute(
            "SELECT * FROM messages WHERE session_id = ? ORDER BY id ASC",
            (sid,),
        ).fetchall()

        if not rows:
            return []

        all_msgs: list[BaseMessage] = []
        for row in rows:
            msg = _row_to_message(row)
            if msg is not None:
                all_msgs.append(msg)

        total_tokens = sum(
            _estimate_tokens(getattr(m, "content", "") or "")
            for m in all_msgs
        )

        if total_tokens <= self._max_tokens:
            return all_msgs

        system_msgs = [m for m in all_msgs if isinstance(m, SystemMessage)]
        non_system = [m for m in all_msgs if not isinstance(m, SystemMessage)]

        system_tokens = sum(
            _estimate_tokens(m.content or "") for m in system_msgs
        )
        remaining = self._max_tokens - system_tokens

        result = list(system_msgs)
        for msg in reversed(non_system):
            tokens = _estimate_tokens(msg.content or "")
            if remaining - tokens >= 0:
                result.insert(len(system_msgs), msg)
                remaining -= tokens
            else:
                break

        return result

    async def inject_subagent_result(self, agent_id: str, agent_name: str,
                                     output: str, status: str) -> None:
        sid = self._ensure_session()
        now = datetime.now(timezone.utc).isoformat()
        content = f"[子 Agent 完成] {agent_name} [{agent_id}]\n状态: {status}\n输出:\n{output}"

        conn = self._get_conn()
        conn.execute(
            "INSERT INTO messages (session_id, role, content, metadata, created_at) VALUES (?, ?, ?, ?, ?)",
            (sid, "tool", content, json.dumps({"agent_id": agent_id, "agent_name": agent_name, "source": "subagent_result"}, ensure_ascii=False), now),
        )
        token_count = _estimate_tokens(content)
        conn.execute(
            "UPDATE sessions SET updated_at = ?, message_count = message_count + 1, token_count = token_count + ? WHERE session_id = ?",
            (now, token_count, sid),
        )
        conn.commit()

    def list_sessions(self, limit: int = 50) -> list[SessionInfo]:
        conn = self._get_conn()
        rows = conn.execute(
            "SELECT * FROM sessions ORDER BY updated_at DESC LIMIT ?",
            (limit,),
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

    def load_session(self, session_id: str) -> bool:
        conn = self._get_conn()
        row = conn.execute(
            "SELECT session_id FROM sessions WHERE session_id = ?",
            (session_id,),
        ).fetchone()
        if row is None:
            return False
        self._current_session_id = session_id
        return True

    def update_session_title(self, session_id: str, title: str) -> None:
        conn = self._get_conn()
        conn.execute(
            "UPDATE sessions SET title = ?, updated_at = ? WHERE session_id = ?",
            (title, datetime.now(timezone.utc).isoformat(), session_id),
        )
        conn.commit()

    def update_subagent_count(self, session_id: str, count: int) -> None:
        conn = self._get_conn()
        conn.execute(
            "UPDATE sessions SET subagent_count = ? WHERE session_id = ?",
            (count, session_id),
        )
        conn.commit()

    def delete_session(self, session_id: str) -> bool:
        conn = self._get_conn()
        conn.execute("DELETE FROM messages WHERE session_id = ?", (session_id,))
        conn.execute("DELETE FROM sessions WHERE session_id = ?", (session_id,))
        conn.commit()
        if self._current_session_id == session_id:
            self._current_session_id = None
        return True

    def search_messages(self, query: str, limit: int = 20) -> list[MessageRecord]:
        conn = self._get_conn()
        rows = conn.execute(
            "SELECT * FROM messages WHERE content LIKE ? ORDER BY id DESC LIMIT ?",
            (f"%{query}%", limit),
        ).fetchall()

        return [
            MessageRecord(
                id=row["id"],
                session_id=row["session_id"],
                role=row["role"],
                content=row["content"][:500],
                tool_call_id=row["tool_call_id"],
                tool_name=row["tool_name"],
                metadata=_safe_json_loads(row["metadata"]),
                created_at=row["created_at"],
            )
            for row in rows
        ]

    def get_session_messages(self, session_id: str) -> list[MessageRecord]:
        conn = self._get_conn()
        rows = conn.execute(
            "SELECT * FROM messages WHERE session_id = ? ORDER BY id ASC",
            (session_id,),
        ).fetchall()

        return [
            MessageRecord(
                id=row["id"],
                session_id=row["session_id"],
                role=row["role"],
                content=row["content"],
                tool_call_id=row["tool_call_id"],
                tool_name=row["tool_name"],
                metadata=_safe_json_loads(row["metadata"]),
                created_at=row["created_at"],
            )
            for row in rows
        ]

    def get_token_usage(self, session_id: str | None = None) -> dict:
        sid = session_id or self._current_session_id
        if sid is None:
            return {"total_tokens": 0, "message_count": 0, "max_tokens": self._max_tokens}

        conn = self._get_conn()
        row = conn.execute(
            "SELECT message_count, token_count FROM sessions WHERE session_id = ?",
            (sid,),
        ).fetchone()

        if row is None:
            return {"total_tokens": 0, "message_count": 0, "max_tokens": self._max_tokens}

        return {
            "total_tokens": row["token_count"],
            "message_count": row["message_count"],
            "max_tokens": self._max_tokens,
        }

    def export_session(self, session_id: str, format: str = "text") -> str:
        messages = self.get_session_messages(session_id)
        if not messages:
            return ""

        if format == "json":
            return json.dumps([
                {
                    "role": m.role,
                    "content": m.content,
                    "tool_name": m.tool_name,
                    "created_at": m.created_at,
                }
                for m in messages
            ], ensure_ascii=False, indent=2)

        lines = []
        for m in messages:
            role_icon = {
                "user": "🧑",
                "assistant": "🤖",
                "system": "⚙️",
                "tool": "🔧",
            }.get(m.role, "❓")
            prefix = f"{role_icon} [{m.role}]"
            if m.tool_name:
                prefix += f" ({m.tool_name})"
            content_preview = m.content[:200].replace("\n", " ")
            lines.append(f"{prefix}: {content_preview}")
            if len(m.content) > 200:
                lines.append(f"   ... (共 {len(m.content)} 字符)")

        return "\n".join(lines)

    def _ensure_session(self) -> str:
        if self._current_session_id is None:
            session_id = str(uuid.uuid4())
            now = datetime.now(timezone.utc).isoformat()
            conn = self._get_conn()
            conn.execute(
                "INSERT INTO sessions (session_id, title, model, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
                (session_id, "新对话", "", now, now),
            )
            conn.commit()
            self._current_session_id = session_id
        return self._current_session_id

    def close(self) -> None:
        if self._conn is not None:
            self._conn.close()
            self._conn = None


def _map_role(msg_type: str) -> str:
    mapping = {
        "human": "user",
        "ai": "assistant",
        "system": "system",
        "tool": "tool",
    }
    return mapping.get(msg_type, msg_type)


def _estimate_tokens(text: str) -> int:
    if not text:
        return 0
    return max(1, len(text) // 4)


def _row_to_message(row: sqlite3.Row) -> BaseMessage | None:
    role = row["role"]
    content = row["content"] or ""

    if role == "system":
        return SystemMessage(content=content)
    elif role == "user":
        return HumanMessage(content=content)
    elif role == "assistant":
        return AIMessage(content=content)
    elif role == "tool":
        return ToolMessage(
            content=content,
            tool_call_id=row["tool_call_id"] or "",
        )
    return None


def _safe_json_loads(text: str | None) -> dict:
    if not text:
        return {}
    try:
        return json.loads(text)
    except (json.JSONDecodeError, TypeError):
        return {}
