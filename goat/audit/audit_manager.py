from __future__ import annotations

import json
import sqlite3
import threading
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from ..core.paths import AUDIT_DB


class AuditManager:
    def __init__(self, db_path: str = "") -> None:
        if not db_path:
            db_path = str(AUDIT_DB)
        self._lock = threading.Lock()
        self._db_path = str(Path(db_path))
        self._conn = sqlite3.connect(self._db_path, check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._init_db()

    def _init_db(self) -> None:
        self._conn.execute(
            "CREATE TABLE IF NOT EXISTS audit_log ("
            "id INTEGER PRIMARY KEY AUTOINCREMENT,"
            "timestamp TEXT NOT NULL,"
            "agent_id TEXT NOT NULL,"
            "tool_name TEXT NOT NULL,"
            "arguments TEXT NOT NULL,"
            "duration_ms REAL NOT NULL,"
            "result_summary TEXT,"
            "success INTEGER NOT NULL DEFAULT 1,"
            "session_id TEXT"
            ")"
        )
        self._conn.commit()

    def log_call(
        self,
        agent_id: str,
        tool_name: str,
        arguments: dict[str, Any] | str,
        duration_ms: float,
        result_summary: str | None = None,
        success: bool = True,
        session_id: str | None = None,
    ) -> int:
        timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S")
        args_str = json.dumps(arguments, ensure_ascii=False) if isinstance(arguments, dict) else arguments
        summary = (result_summary or "")[:200]

        with self._lock:
            cur = self._conn.execute(
                "INSERT INTO audit_log "
                "(timestamp, agent_id, tool_name, arguments, duration_ms, result_summary, success, session_id) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (timestamp, agent_id, tool_name, args_str, duration_ms, summary, 1 if success else 0, session_id),
            )
            self._conn.commit()
            return cur.lastrowid

    def query(
        self,
        session_id: str | None = None,
        tool_name: str | None = None,
        limit: int = 50,
    ) -> list[dict[str, Any]]:
        parts: list[str] = ["SELECT * FROM audit_log WHERE 1=1"]
        params: list[Any] = []

        if session_id is not None:
            parts.append("AND session_id = ?")
            params.append(session_id)
        if tool_name is not None:
            parts.append("AND tool_name = ?")
            params.append(tool_name)

        parts.append("ORDER BY id DESC LIMIT ?")
        params.append(limit)

        with self._lock:
            rows = self._conn.execute(" ".join(parts), params).fetchall()

        return [dict(r) for r in rows]

    def get_stats(self) -> dict[str, Any]:
        with self._lock:
            total = self._conn.execute("SELECT COUNT(*) FROM audit_log").fetchone()[0]
            success = self._conn.execute("SELECT COUNT(*) FROM audit_log WHERE success = 1").fetchone()[0]
            failed = self._conn.execute("SELECT COUNT(*) FROM audit_log WHERE success = 0").fetchone()[0]
            avg_duration = self._conn.execute("SELECT AVG(duration_ms) FROM audit_log").fetchone()[0] or 0.0

            per_tool = self._conn.execute(
                "SELECT tool_name, COUNT(*) AS calls, AVG(duration_ms) AS avg_dur "
                "FROM audit_log GROUP BY tool_name ORDER BY calls DESC"
            ).fetchall()

        return {
            "total_calls": total,
            "success": success,
            "failed": failed,
            "avg_duration_ms": round(avg_duration, 2),
            "per_tool": [dict(r) for r in per_tool],
        }

    def export_json(self, filepath: str) -> str:
        with self._lock:
            rows = self._conn.execute("SELECT * FROM audit_log ORDER BY id").fetchall()

        data = [dict(r) for r in rows]
        path = Path(filepath)
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
        return str(path.absolute())

    def close(self) -> None:
        self._conn.close()