from __future__ import annotations

import asyncio
import json
import sqlite3
import time
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Callable, Awaitable

from ..core.paths import TASKS_DB


class TaskType(Enum):
    BATCH = "batch"
    BACKGROUND = "background"
    SCHEDULED = "scheduled"


class TaskStatus(Enum):
    PENDING = "pending"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"
    PAUSED = "paused"


TaskFn = Callable[["TaskContext"], Awaitable[str]]


@dataclass
class TaskContext:
    task_id: str
    task_type: TaskType
    name: str
    description: str
    metadata: dict
    cancel_token: asyncio.Event
    progress: float = 0.0
    current_step: str = ""


@dataclass
class TaskRecord:
    task_id: str
    task_type: str
    name: str
    description: str
    status: str
    priority: int
    progress: float
    current_step: str
    output: str
    error: str
    metadata: dict
    created_at: str
    updated_at: str
    completed_at: str | None


@dataclass
class TaskDef:
    name: str
    description: str
    fn: TaskFn
    task_type: TaskType = TaskType.BACKGROUND
    priority: int = 0
    metadata: dict = field(default_factory=dict)


class DurableTaskManager:
    def __init__(self, db_path: str = "", max_workers: int = 4):
        if not db_path:
            db_path = str(TASKS_DB)
        self._db_path = Path(db_path)
        self._max_workers = max(max_workers, 1)
        self._semaphore = asyncio.Semaphore(max_workers)
        self._task_queue: asyncio.Queue[TaskRecord] = asyncio.Queue()
        self._workers: list[asyncio.Task] = []
        self._running = False
        self._cancel_events: dict[str, asyncio.Event] = {}
        self._task_fn_registry: dict[str, TaskDef] = {}
        self._conn: sqlite3.Connection | None = None
        self._init_db()

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
            CREATE TABLE IF NOT EXISTS tasks (
                task_id TEXT PRIMARY KEY,
                task_type TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'pending',
                priority INTEGER DEFAULT 0,
                progress REAL DEFAULT 0.0,
                current_step TEXT DEFAULT '',
                output TEXT DEFAULT '',
                error TEXT DEFAULT '',
                metadata TEXT DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
            CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created_at DESC);
        """)
        conn.commit()

    def register_task_type(self, task_def: TaskDef) -> None:
        self._task_fn_registry[task_def.name] = task_def

    async def submit(self, task_name: str, description: str = "",
                     metadata: dict | None = None) -> tuple[str, str]:
        task_def = self._task_fn_registry.get(task_name)
        if task_def is None:
            valid = list(self._task_fn_registry.keys())
            return "", f"未知任务类型 '{task_name}'，可选: {valid}"

        task_id = str(uuid.uuid4())[:8]
        now = datetime.now(timezone.utc).isoformat()

        record = TaskRecord(
            task_id=task_id,
            task_type=task_def.task_type.value,
            name=task_name,
            description=description or task_def.description,
            status=TaskStatus.PENDING.value,
            priority=task_def.priority,
            progress=0.0,
            current_step="等待调度",
            output="",
            error="",
            metadata=metadata or task_def.metadata,
            created_at=now,
            updated_at=now,
            completed_at=None,
        )

        conn = self._get_conn()
        conn.execute(
            "INSERT INTO tasks (task_id, task_type, name, description, status, priority, metadata, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (record.task_id, record.task_type, record.name, record.description,
             record.status, record.priority,
             json.dumps(record.metadata, ensure_ascii=False),
             record.created_at, record.updated_at),
        )
        conn.commit()

        await self._task_queue.put(record)
        return task_id, ""

    async def cancel(self, task_id: str) -> str:
        cancel_event = self._cancel_events.get(task_id)
        if cancel_event is not None:
            cancel_event.set()

        conn = self._get_conn()
        conn.execute(
            "UPDATE tasks SET status = ?, updated_at = ? WHERE task_id = ?",
            (TaskStatus.CANCELLED.value, datetime.now(timezone.utc).isoformat(), task_id),
        )
        conn.commit()
        return f"任务 {task_id} 已取消"

    async def pause(self, task_id: str) -> str:
        conn = self._get_conn()
        row = conn.execute(
            "SELECT status FROM tasks WHERE task_id = ?", (task_id,),
        ).fetchone()
        if row is None:
            return f"任务不存在: {task_id}"
        if row["status"] != TaskStatus.RUNNING.value:
            return f"任务 {task_id} 不在运行状态，无法暂停"

        cancel_event = self._cancel_events.get(task_id)
        if cancel_event is not None:
            cancel_event.set()

        conn.execute(
            "UPDATE tasks SET status = ?, updated_at = ? WHERE task_id = ?",
            (TaskStatus.PAUSED.value, datetime.now(timezone.utc).isoformat(), task_id),
        )
        conn.commit()
        return f"任务 {task_id} 已暂停"

    async def resume(self, task_id: str) -> str:
        conn = self._get_conn()
        row = conn.execute(
            "SELECT * FROM tasks WHERE task_id = ?", (task_id,),
        ).fetchone()
        if row is None:
            return f"任务不存在: {task_id}"

        record = _row_to_record(row)
        if record.status != TaskStatus.PAUSED.value:
            return f"任务 {task_id} 不在暂停状态，无法恢复"

        conn.execute(
            "UPDATE tasks SET status = ?, updated_at = ? WHERE task_id = ?",
            (TaskStatus.PENDING.value, datetime.now(timezone.utc).isoformat(), task_id),
        )
        conn.commit()

        record.status = TaskStatus.PENDING.value
        await self._task_queue.put(record)
        return f"任务 {task_id} 已重新提交"

    def get_status(self, task_id: str) -> TaskRecord | None:
        conn = self._get_conn()
        row = conn.execute(
            "SELECT * FROM tasks WHERE task_id = ?", (task_id,),
        ).fetchone()
        return _row_to_record(row) if row else None

    def list_tasks(self, status: str | None = None, limit: int = 50) -> list[TaskRecord]:
        conn = self._get_conn()
        if status:
            rows = conn.execute(
                "SELECT * FROM tasks WHERE status = ? ORDER BY created_at DESC LIMIT ?",
                (status, limit),
            ).fetchall()
        else:
            rows = conn.execute(
                "SELECT * FROM tasks ORDER BY created_at DESC LIMIT ?",
                (limit,),
            ).fetchall()
        return [_row_to_record(r) for r in rows]

    def list_active(self) -> list[TaskRecord]:
        return self.list_tasks(status=TaskStatus.RUNNING.value)

    async def recover(self) -> list[TaskRecord]:
        conn = self._get_conn()
        rows = conn.execute(
            "SELECT * FROM tasks WHERE status IN (?, ?) ORDER BY priority DESC, created_at ASC",
            (TaskStatus.RUNNING.value, TaskStatus.PENDING.value),
        ).fetchall()

        recovered = []
        for row in rows:
            record = _row_to_record(row)
            if record.status == TaskStatus.RUNNING.value:
                conn.execute(
                    "UPDATE tasks SET status = ?, current_step = ?, updated_at = ? WHERE task_id = ?",
                    (TaskStatus.PENDING.value, "已恢复（上次运行中断）",
                     datetime.now(timezone.utc).isoformat(), record.task_id),
                )
                record.status = TaskStatus.PENDING.value
                record.current_step = "已恢复（上次运行中断）"
            await self._task_queue.put(record)
            recovered.append(record)

        conn.commit()
        return recovered

    def start(self) -> None:
        if self._running:
            return
        self._running = True
        for i in range(self._max_workers):
            worker = asyncio.create_task(self._worker_loop(i))
            self._workers.append(worker)

    async def stop(self, wait: bool = True) -> None:
        self._running = False
        for _ in range(len(self._workers)):
            await self._task_queue.put(None)
        if wait and self._workers:
            await asyncio.gather(*self._workers, return_exceptions=True)
        self._workers.clear()

    async def _worker_loop(self, worker_id: int) -> None:
        while self._running:
            record = await self._task_queue.get()
            if record is None:
                self._task_queue.task_done()
                break

            if not self._running:
                self._task_queue.task_done()
                break

            async with self._semaphore:
                await self._execute_task(record, worker_id)

            self._task_queue.task_done()

    async def _execute_task(self, record: TaskRecord, worker_id: int) -> None:
        task_def = self._task_fn_registry.get(record.name)
        if task_def is None:
            return

        cancel_event = asyncio.Event()
        self._cancel_events[record.task_id] = cancel_event

        conn = self._get_conn()
        conn.execute(
            "UPDATE tasks SET status = ?, current_step = ?, updated_at = ? WHERE task_id = ?",
            (TaskStatus.RUNNING.value, "运行中", datetime.now(timezone.utc).isoformat(), record.task_id),
        )
        conn.commit()

        ctx = TaskContext(
            task_id=record.task_id,
            task_type=TaskType(record.task_type),
            name=record.name,
            description=record.description,
            metadata=record.metadata,
            cancel_token=cancel_event,
        )

        try:
            output = await task_def.fn(ctx)
            if not cancel_event.is_set():
                now = datetime.now(timezone.utc).isoformat()
                conn.execute(
                    "UPDATE tasks SET status = ?, progress = ?, current_step = ?, output = ?, completed_at = ?, updated_at = ? WHERE task_id = ?",
                    (TaskStatus.COMPLETED.value, 1.0, "完成", output, now, now, record.task_id),
                )
                conn.commit()
        except asyncio.CancelledError:
            pass
        except Exception as e:
            if not cancel_event.is_set():
                now = datetime.now(timezone.utc).isoformat()
                conn.execute(
                    "UPDATE tasks SET status = ?, error = ?, completed_at = ?, updated_at = ? WHERE task_id = ?",
                    (TaskStatus.FAILED.value, f"{e}", now, now, record.task_id),
                )
                conn.commit()
        finally:
            self._cancel_events.pop(record.task_id, None)

    @property
    def worker_count(self) -> int:
        return self._max_workers

    @property
    def pending_count(self) -> int:
        return self._task_queue.qsize()

    @property
    def running_count(self) -> int:
        return len(self._cancel_events)

    def close(self) -> None:
        if self._conn is not None:
            self._conn.close()
            self._conn = None


def _row_to_record(row: sqlite3.Row | None) -> TaskRecord | None:
    if row is None:
        return None
    return TaskRecord(
        task_id=row["task_id"],
        task_type=row["task_type"],
        name=row["name"],
        description=row["description"],
        status=row["status"],
        priority=row["priority"],
        progress=row["progress"],
        current_step=row["current_step"],
        output=row["output"],
        error=row["error"],
        metadata=json.loads(row["metadata"]) if row["metadata"] else {},
        created_at=row["created_at"],
        updated_at=row["updated_at"],
        completed_at=row["completed_at"],
    )