from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from typing import Optional

from ..core.event_bus import EventBus, EventType

logger = logging.getLogger(__name__)

MCP_CONNECTED_SERVERS: dict[str, dict] = {}


@dataclass
class McpConnectionInfo:
    name: str
    tool_count: int = 0
    status: str = "disconnected"
    tool_names: list[str] = field(default_factory=list)


@dataclass
class McpTuiState:
    connected_servers: dict[str, McpConnectionInfo] = field(default_factory=dict)
    mcp_tool_count: int = 0
    version: int = 0

    def bump(self):
        self.version += 1


class McpTuiBridge:
    def __init__(self, event_bus: EventBus, state: McpTuiState):
        self._event_bus = event_bus
        self.state = state
        self._subscription_id = "mcp_tui_bridge"

    def start(self):
        self._event_bus.subscribe(self._subscription_id)

    def stop(self):
        self._event_bus.unsubscribe(self._subscription_id)


async def publish_mcp_connect(
    event_bus: EventBus,
    server_name: str,
    tool_count: int,
    tool_names: list[str] | None = None,
):
    from goat.core.event_bus import SubagentEvent
    payload = json.dumps({
        "server": server_name,
        "tool_count": tool_count,
        "tools": tool_names or [],
    })
    event = SubagentEvent(
        event_type=EventType.NOTIFICATION,
        agent_id="mcp",
        agent_name="mcp",
        payload=payload,
    )
    await event_bus.publish(event)


async def publish_mcp_disconnect(
    event_bus: EventBus,
    server_name: str,
):
    from goat.core.event_bus import SubagentEvent
    event = SubagentEvent(
        event_type=EventType.NOTIFICATION,
        agent_id="mcp",
        agent_name="mcp",
        payload=f"MCP disconnected: {server_name}",
    )
    await event_bus.publish(event)


async def publish_mcp_error(
    event_bus: EventBus,
    server_name: str,
    error_message: str,
):
    from goat.core.event_bus import SubagentEvent
    event = SubagentEvent(
        event_type=EventType.ERROR,
        agent_id="mcp",
        agent_name="mcp",
        payload=f"MCP error [{server_name}]: {error_message}",
    )
    await event_bus.publish(event)