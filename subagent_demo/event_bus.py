from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass, field
from enum import Enum


class EventType(Enum):
    STATUS_CHANGE = "status_change"
    TOOL_CALL = "tool_call"
    TOOL_RESULT = "tool_result"
    LLM_STREAM = "llm_stream"
    LLM_RESPONSE = "llm_response"
    COMPLETED = "completed"
    ERROR = "error"
    MESSAGE = "message"


@dataclass
class SubagentEvent:
    event_type: EventType
    agent_id: str
    agent_name: str
    payload: str
    depth: int = 0
    timestamp: float = field(default_factory=time.time)

    def short(self) -> str:
        icon = {
            EventType.STATUS_CHANGE: "🔄",
            EventType.TOOL_CALL: "🔧",
            EventType.TOOL_RESULT: "📋",
            EventType.LLM_STREAM: "💬",
            EventType.LLM_RESPONSE: "🤖",
            EventType.COMPLETED: "✅",
            EventType.ERROR: "❌",
            EventType.MESSAGE: "📝",
        }.get(self.event_type, "❓")
        indent = "  " * min(self.depth, 4)
        prefix = f"{indent}{icon} [{self.agent_name}]"
        preview = self.payload[:120].replace("\n", " ")
        return f"{prefix} {preview}"


class Subscription:
    def __init__(self):
        self._queue: asyncio.Queue[SubagentEvent] = asyncio.Queue()


class EventBus:
    def __init__(self):
        self._lock = asyncio.Lock()
        self._subscriptions: dict[str, Subscription] = {}
        self._agent_filters: dict[str, set[str | None]] = {}

    def subscribe(self, sub_id: str, agent_id: str | None = None) -> Subscription:
        sub = Subscription()
        self._subscriptions[sub_id] = sub
        if agent_id not in self._agent_filters:
            self._agent_filters[agent_id] = set()
        self._agent_filters[agent_id].add(sub_id)
        return sub

    def unsubscribe(self, sub_id: str) -> None:
        self._subscriptions.pop(sub_id, None)
        for filters in self._agent_filters.values():
            filters.discard(sub_id)

    async def publish(self, event: SubagentEvent) -> None:
        targets: set[str] = set()
        if None in self._agent_filters:
            targets.update(self._agent_filters[None])
        if event.agent_id in self._agent_filters:
            targets.update(self._agent_filters[event.agent_id])
        for sub_id in targets:
            sub = self._subscriptions.get(sub_id)
            if sub is not None:
                await sub._queue.put(event)

    async def stream(self, sub_id: str, timeout: float | None = None
                     ) -> asyncio.Queue[SubagentEvent]:
        sub = self._subscriptions.get(sub_id)
        if sub is None:
            sub = self.subscribe(sub_id)
        return sub._queue
