from __future__ import annotations

import sqlite3
import threading
from datetime import datetime, timezone

from ..core.paths import MEMORY_DB as MEMORY_DB_PATH


class MemoryManager:
    _instance: MemoryManager | None = None
    _lock = threading.Lock()

    def __new__(cls) -> MemoryManager:
        if cls._instance is None:
            with cls._lock:
                if cls._instance is None:
                    cls._instance = super().__new__(cls)
                    cls._instance._initialized = False
        return cls._instance

    def __init__(self) -> None:
        if self._initialized:
            return
        self._initialized = True
        self._conn = sqlite3.connect(str(MEMORY_DB_PATH), check_same_thread=False)
        self._conn.execute(
            "CREATE TABLE IF NOT EXISTS memories ("
            "id INTEGER PRIMARY KEY AUTOINCREMENT,"
            "key TEXT UNIQUE NOT NULL,"
            "content TEXT NOT NULL,"
            "created_at TEXT NOT NULL DEFAULT (datetime('now')),"
            "updated_at TEXT NOT NULL DEFAULT (datetime('now'))"
            ")"
        )
        self._conn.commit()

    def set(self, key: str, content: str) -> None:
        now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S")
        self._conn.execute(
            "INSERT INTO memories (key, content, created_at, updated_at) "
            "VALUES (?, ?, ?, ?) "
            "ON CONFLICT(key) DO UPDATE SET content = excluded.content, "
            "updated_at = excluded.updated_at",
            (key, content, now, now),
        )
        self._conn.commit()

    def get(self, key: str) -> str | None:
        row = self._conn.execute(
            "SELECT content FROM memories WHERE key = ?", (key,)
        ).fetchone()
        return row[0] if row else None

    def get_all(self) -> dict[str, str]:
        rows = self._conn.execute(
            "SELECT key, content FROM memories ORDER BY updated_at DESC"
        ).fetchall()
        return {row[0]: row[1] for row in rows}

    def delete(self, key: str) -> bool:
        cur = self._conn.execute("DELETE FROM memories WHERE key = ?", (key,))
        self._conn.commit()
        return cur.rowcount > 0

    def list_keys(self) -> list[str]:
        rows = self._conn.execute(
            "SELECT key FROM memories ORDER BY updated_at DESC"
        ).fetchall()
        return [row[0] for row in rows]

    def clear_all(self) -> int:
        cur = self._conn.execute("DELETE FROM memories")
        self._conn.commit()
        return cur.rowcount