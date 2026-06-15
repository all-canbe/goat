from __future__ import annotations

import asyncio
import logging
from contextlib import AsyncExitStack
from dataclasses import dataclass, field
from typing import Any

from langchain_core.tools import BaseTool
from pydantic import BaseModel, create_model

from ..tools.tools import BUILTIN_TOOLS
from ..core.event_bus import EventBus, EventType
from .mcp_config import McpServerConnection, load_mcp_config

logger = logging.getLogger(__name__)

_exit_stack: AsyncExitStack | None = None


@dataclass
class _ConnectedServer:
    connection: McpServerConnection
    session: Any = None
    tools: list[dict] = field(default_factory=list)
    running: bool = True


def _json_schema_to_pydantic(name: str, schema: dict) -> type[BaseModel]:
    properties = schema.get("properties", {})
    required = schema.get("required", [])

    fields: dict[str, Any] = {}

    for prop_name, prop_def in properties.items():
        ptype = prop_def.get("type", "string")
        is_required = prop_name in required

        if ptype == "string":
            py_type = str
        elif ptype == "number":
            py_type = float
        elif ptype == "integer":
            py_type = int
        elif ptype == "boolean":
            py_type = bool
        elif ptype == "array":
            py_type = list
        elif ptype == "object":
            py_type = dict
        else:
            py_type = str

        default = ... if is_required else prop_def.get("default", None)
        description = prop_def.get("description", "")

        fields[prop_name] = (py_type, default)

    if not fields:
        return create_model(name, __base__=BaseModel)

    return create_model(name, **fields)


class McpToolWrapper(BaseTool):
    name: str = ""
    description: str = ""
    args_schema: type[BaseModel] | None = None

    _connection: McpServerConnection | None = None
    _session: Any = None
    _session_lock: asyncio.Lock | None = None
    _event_bus: EventBus | None = None

    def __init__(
        self,
        name: str,
        description: str,
        input_schema: dict,
        connection: McpServerConnection,
        session: Any,
        event_bus: EventBus | None = None,
    ):
        super().__init__()
        self.name = name
        self.description = description
        self._connection = connection
        self._session = session
        self._session_lock = asyncio.Lock()
        self._event_bus = event_bus

        safe_name = name.replace("-", "_").replace(".", "_").replace("/", "_")
        model_name = f"McpArgs_{safe_name}"
        self.args_schema = _json_schema_to_pydantic(model_name, input_schema)

    def _run(self, **kwargs) -> str:
        raise RuntimeError("MCP tools require async execution; use _arun instead")

    async def _ensure_session(self):
        if self._session is None:
            raise RuntimeError(f"MCP tool '{self.name}' has no active session")
        return self._session

    async def _arun(self, **kwargs) -> str:
        # TOOL_CALL 事件由 mcp_dispatch.py 发射，此处不重复发布

        try:
            session = await self._ensure_session()
            result = await session.call_tool(self.name, arguments=kwargs)

            text_parts = []
            for item in result.content:
                if hasattr(item, "text"):
                    text_parts.append(item.text)
                elif hasattr(item, "data"):
                    text_parts.append(str(item.data))
                elif isinstance(item, dict):
                    text_parts.append(str(item))
                else:
                    text_parts.append(str(item))

            output = "\n".join(text_parts) if text_parts else str(result)

            if self._event_bus:
                self._event_bus.publish_nowait(
                    source_id="mcp_client",
                    event_type=EventType.TOOL_RESULT,
                    payload=output[:500],
                    agent_name=f"mcp:{self.name}",
                )

            return output
        except Exception as e:
            error_msg = f"MCP tool '{self.name}' error: {e}"
            logger.error(error_msg)
            return error_msg


async def _connect_stdio(stack: AsyncExitStack, conn: McpServerConnection):
    from mcp import ClientSession
    from mcp.client.stdio import stdio_client, StdioServerParameters

    params = StdioServerParameters(
        command=conn.command,
        args=conn.args,
        env=conn.env or None,
    )
    read, write = await stack.enter_async_context(stdio_client(params))
    session = await stack.enter_async_context(ClientSession(read, write))
    await session.initialize()
    return session


async def _connect_url(stack: AsyncExitStack, conn: McpServerConnection):
    from mcp import ClientSession

    transport = (conn.transport or "").lower()

    if transport == "sse":
        from mcp.client.sse import sse_client
        read, write = await stack.enter_async_context(sse_client(conn.url))
    elif transport == "streamable_http":
        from mcp.client.streamable_http import streamablehttp_client
        read, write = await stack.enter_async_context(
            streamablehttp_client(conn.url, headers=conn.headers or None)
        )
    else:
        try:
            from mcp.client.streamable_http import streamablehttp_client
            read, write = await stack.enter_async_context(
                streamablehttp_client(conn.url, headers=conn.headers or None)
            )
        except Exception:
            from mcp.client.sse import sse_client
            read, write = await stack.enter_async_context(sse_client(conn.url))

    session = await stack.enter_async_context(ClientSession(read, write))
    await session.initialize()
    return session


async def connect_mcp_servers(
    connections: list[McpServerConnection] | None = None,
    event_bus: EventBus | None = None,
) -> list[BaseTool]:
    try:
        from mcp import ClientSession
    except ImportError:
        logger.warning("MCP SDK not installed. Skipping MCP client connections.")
        return []

    if connections is None:
        config = load_mcp_config()
        if not config.connections:
            return []
        connections = config.connections

    global _exit_stack
    if _exit_stack is not None:
        await _exit_stack.aclose()
    _exit_stack = AsyncExitStack()
    await _exit_stack.__aenter__()

    tools: list[BaseTool] = []
    connected: list[_ConnectedServer] = []

    for conn in connections:
        try:
            if conn.command:
                session = await _connect_stdio(_exit_stack, conn)
            elif conn.url:
                session = await _connect_url(_exit_stack, conn)
            else:
                logger.warning(f"MCP connection '{conn.name}' has no command or url, skipping.")
                continue

            tool_list = await session.list_tools()

            server = _ConnectedServer(connection=conn, session=session, tools=[])
            for tool_def in tool_list.tools:
                wrapper = McpToolWrapper(
                    name=tool_def.name,
                    description=tool_def.description or "",
                    input_schema=tool_def.inputSchema,
                    connection=conn,
                    session=session,
                    event_bus=event_bus,
                )
                tools.append(wrapper)
                server.tools.append({
                    "name": tool_def.name,
                    "description": tool_def.description or "",
                })

                if wrapper.name not in BUILTIN_TOOLS:
                    BUILTIN_TOOLS[wrapper.name] = wrapper

            connected.append(server)

            if event_bus:
                event_bus.publish_nowait(
                    source_id="mcp_client",
                    event_type=EventType.NOTIFICATION,
                    payload=f"MCP connected: {conn.name} ({len(server.tools)} tools)",
                    agent_name="mcp",
                )

            logger.info(f"MCP connected: {conn.name} ({len(server.tools)} tools)")

        except Exception as e:
            logger.error(f"Failed to connect MCP server '{conn.name}': {e}")
            if event_bus:
                event_bus.publish_nowait(
                    source_id="mcp_client",
                    event_type=EventType.ERROR,
                    payload=f"MCP connection failed: {conn.name}: {e}",
                    agent_name="mcp",
                )

    logger.info(f"MCP client: {len(connected)} servers, {len(tools)} total tools")
    return tools


async def disconnect_mcp_servers():
    global _exit_stack
    if _exit_stack is not None:
        try:
            await _exit_stack.aclose()
        except (RuntimeError, Exception):
            pass
        _exit_stack = None
        logger.info("MCP servers disconnected")
