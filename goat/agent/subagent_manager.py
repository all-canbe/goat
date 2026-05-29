from __future__ import annotations

import asyncio
import json
import uuid
from enum import Enum
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from ..core.cancellation import CancellationToken
from .subagent_roles import RoleType, RoleDefinition, get_role, GOAT_NICKNAMES


class SubAgentStatus(Enum):
    PENDING = "pending"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"
    INTERRUPTED = "interrupted"


@dataclass
class SubAgent:
    agent_id: str
    name: str
    role_type: RoleType
    status: SubAgentStatus = SubAgentStatus.PENDING
    spawn_depth: int = 0
    parent_id: str | None = None
    session_boot_id: str = ""
    task_description: str = ""
    output: str = ""
    error: str = ""
    created_at: str = ""
    completed_at: str = ""
    cancel_token: CancellationToken = field(default_factory=CancellationToken)
    completion_event: asyncio.Event = field(default_factory=asyncio.Event)
    message_queue: asyncio.Queue[str] = field(default_factory=asyncio.Queue)
    task_handle: asyncio.Task | None = None
    metadata: dict = field(default_factory=dict)


class SubAgentManager:
    def __init__(self, max_concurrent: int = 10, max_spawn_depth: int = 3,
                 state_file: str = "subagents.json"):
        self._lock = asyncio.Lock()
        self._max_concurrent = min(max_concurrent, 20)
        self._max_spawn_depth = max_spawn_depth
        self._state_file = Path(state_file)
        self._session_boot_id = str(uuid.uuid4())
        self._subagents: dict[str, SubAgent] = {}
        self._nickname_counter = 0
        self._load_state()

    @property
    def session_boot_id(self) -> str:
        return self._session_boot_id

    @property
    def max_concurrent(self) -> int:
        return self._max_concurrent

    @property
    def max_spawn_depth(self) -> int:
        return self._max_spawn_depth

    def _next_nickname(self) -> str:
        idx = self._nickname_counter % len(GOAT_NICKNAMES)
        self._nickname_counter += 1
        return GOAT_NICKNAMES[idx]

    def running_count(self) -> int:
        return sum(
            1 for a in self._subagents.values()
            if a.status == SubAgentStatus.RUNNING
            and a.task_handle is not None
            and not a.task_handle.done()
        )

    def computing_count(self) -> int:
        return sum(
            1 for a in self._subagents.values()
            if a.status in (SubAgentStatus.PENDING, SubAgentStatus.RUNNING)
        )

    async def spawn(self, parent_id: str | None, role_type: RoleType,
                    task_description: str, parent_cancel_token: CancellationToken | None = None,
                    spawn_depth: int = 0) -> SubAgent:
        async with self._lock:
            computing = self.computing_count()
            if computing >= self._max_concurrent:
                raise RuntimeError(
                    f"达到并发上限 ({computing}/{self._max_concurrent})，"
                    f"请等待或在 agent_cancel 中释放资源"
                )

            if spawn_depth >= self._max_spawn_depth:
                raise RuntimeError(
                    f"达到最大嵌套深度 ({spawn_depth}/{self._max_spawn_depth})"
                )

            role_def = get_role(role_type)
            agent_id = str(uuid.uuid4())[:8]
            nickname = self._next_nickname()
            name = f"🐐 {nickname} ({role_def.display_name.split('(')[0].strip()})"

            if parent_cancel_token is not None:
                child_token = parent_cancel_token.child_token()
            else:
                child_token = CancellationToken()

            agent = SubAgent(
                agent_id=agent_id,
                name=name,
                role_type=role_type,
                status=SubAgentStatus.PENDING,
                spawn_depth=spawn_depth,
                parent_id=parent_id,
                session_boot_id=self._session_boot_id,
                task_description=task_description,
                created_at=datetime.now(timezone.utc).isoformat(),
                cancel_token=child_token,
            )
            self._subagents[agent_id] = agent
            self._save_state()
            return agent

    async def update_status(self, agent_id: str, status: SubAgentStatus,
                            output: str = "", error: str = "") -> None:
        async with self._lock:
            agent = self._subagents.get(agent_id)
            if agent is None:
                return
            agent.status = status
            if output:
                agent.output = output
            if error:
                agent.error = error
            if status in (SubAgentStatus.COMPLETED, SubAgentStatus.FAILED,
                          SubAgentStatus.CANCELLED, SubAgentStatus.INTERRUPTED):
                agent.completed_at = datetime.now(timezone.utc).isoformat()
                agent.completion_event.set()
            self._save_state()

    async def cancel(self, agent_id: str) -> None:
        async with self._lock:
            agent = self._subagents.get(agent_id)
            if agent is None:
                return
            agent.cancel_token.cancel()
            if agent.task_handle is not None and not agent.task_handle.done():
                agent.task_handle.cancel()
            agent.status = SubAgentStatus.CANCELLED
            agent.completed_at = datetime.now(timezone.utc).isoformat()
            agent.completion_event.set()
            self._save_state()

    def list_agents(self, current_session_only: bool = True) -> list[SubAgent]:
        agents = list(self._subagents.values())
        if current_session_only:
            agents = [a for a in agents if a.session_boot_id == self._session_boot_id]
        agents.sort(key=lambda a: a.created_at)
        return agents

    def get_agent(self, agent_id: str) -> SubAgent | None:
        return self._subagents.get(agent_id)

    async def collect_results(self, agent_ids: list[str] | None = None) -> str:
        if agent_ids is None:
            agent_ids = [a.agent_id for a in self.list_agents()
                         if a.spawn_depth == 1]

        results = []
        for aid in agent_ids:
            agent = self._subagents.get(aid)
            if agent is None:
                continue
            try:
                await asyncio.wait_for(agent.completion_event.wait(), timeout=60)
            except asyncio.TimeoutError:
                results.append(f"### {agent.name} [{aid}]\n状态: 超时")
                continue

            status_icon = {
                SubAgentStatus.COMPLETED: "✅",
                SubAgentStatus.FAILED: "❌",
                SubAgentStatus.CANCELLED: "🚫",
                SubAgentStatus.INTERRUPTED: "⏸️",
            }.get(agent.status, "❓")

            output = agent.output or agent.error or "(无输出)"
            meta_lines = []
            if agent.metadata:
                summary = agent.metadata.get("summary", "")
                risks = agent.metadata.get("risks", "")
                if summary and summary != "(未提供)":
                    meta_lines.append(f"**SUMMARY**: {summary[:200]}")
                if risks and risks != "(未提供)":
                    meta_lines.append(f"**RISKS**: {risks[:200]}")
            meta_block = "\n".join(meta_lines)
            results.append(
                f"### {status_icon} {agent.name} [{aid}]\n"
                f"角色: {agent.role_type.value}\n"
                f"深度: {agent.spawn_depth}\n"
                f"任务: {agent.task_description}\n"
                f"{meta_block}\n\n"
                f"{output}"
            )

        return "\n\n---\n\n".join(results) if results else "没有可收集的子 Agent 结果"

    async def send_message(self, agent_id: str, message: str) -> str:
        agent = self._subagents.get(agent_id)
        if agent is None:
            return f"错误: Agent {agent_id} 不存在"
        if agent.status != SubAgentStatus.RUNNING:
            return f"错误: Agent {agent_id} 状态为 {agent.status.value}"
        await agent.message_queue.put(message)
        return f"消息已发送到 Agent {agent.name} [{agent_id}]"

    def format_agent_list(self) -> str:
        agents = self.list_agents()
        if not agents:
            return "当前没有子 Agent"

        lines = []
        for a in agents:
            icon = {
                SubAgentStatus.PENDING: "⏳",
                SubAgentStatus.RUNNING: "🔄",
                SubAgentStatus.COMPLETED: "✅",
                SubAgentStatus.FAILED: "❌",
                SubAgentStatus.CANCELLED: "🚫",
                SubAgentStatus.INTERRUPTED: "⏸️",
            }.get(a.status, "❓")
            lines.append(
                f"  {icon} [{a.agent_id}] {a.name} "
                f"({a.role_type.value}, depth={a.spawn_depth}) "
                f"— {a.task_description[:50]}"
            )
        return "\n".join(lines)

    def _load_state(self) -> None:
        if not self._state_file.exists():
            return
        try:
            data = json.loads(self._state_file.read_text(encoding="utf-8"))
            for item in data.get("subagents", []):
                if item.get("session_boot_id") != self._session_boot_id:
                    item["status"] = SubAgentStatus.INTERRUPTED.value
        except Exception:
            pass

    def _save_state(self) -> None:
        try:
            data = {
                "version": 1,
                "session_boot_id": self._session_boot_id,
                "subagents": [
                    {
                        "agent_id": a.agent_id,
                        "name": a.name,
                        "role_type": a.role_type.value,
                        "status": a.status.value,
                        "spawn_depth": a.spawn_depth,
                        "parent_id": a.parent_id,
                        "session_boot_id": a.session_boot_id,
                        "task_description": a.task_description,
                        "output": a.output,
                        "error": a.error,
                        "created_at": a.created_at,
                        "completed_at": a.completed_at,
                    }
                    for a in self._subagents.values()
                ],
            }
            tmp = self._state_file.with_suffix(".tmp")
            tmp.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
            tmp.replace(self._state_file)
        except Exception:
            pass

    async def shutdown(self) -> None:
        for agent in self._subagents.values():
            agent.cancel_token.cancel()
            if agent.task_handle is not None and not agent.task_handle.done():
                agent.task_handle.cancel()
        self._save_state()