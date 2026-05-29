from __future__ import annotations

import asyncio
import logging
import os
import signal
import sys

from .mcp_config import McpConfig, load_mcp_config
from .mcp_tool_defs import build_mcp_tools
from .mcp_dispatch import dispatch_mcp_call, McpDispatchOptions

logger = logging.getLogger(__name__)


async def start_mcp_server(
    tool_names: list[str] | None = None,
    config: McpConfig | None = None,
):
    try:
        from mcp.server import Server
        from mcp.server.stdio import stdio_server
    except ImportError:
        print("MCP SDK not installed. Run: pip install mcp>=1.10.0", file=sys.stderr)
        sys.exit(1)

    if config is None:
        config = load_mcp_config()

    allowed_names: list[str] | None = None
    if config.tools_allowlist is not None:
        allowed_names = [n for n in tool_names or config.tools_allowlist if n not in config.tools_denylist]
    elif tool_names is not None:
        allowed_names = [n for n in tool_names if n not in config.tools_denylist]

    server = Server(
        {"name": "goat", "version": "0.1.0"},
        {"capabilities": {"tools": {}}},
    )

    @server.list_tools()
    async def handle_list_tools():
        tools = build_mcp_tools(allowed_names)
        return tools

    @server.call_tool()
    async def handle_call_tool(name: str, arguments: dict):
        result = await dispatch_mcp_call(
            tool_name=name,
            params=arguments,
            opts=McpDispatchOptions(remote=True, source_id="stdio"),
        )
        return result.content

    opts = {}
    if os.environ.get("MCP_STDIO"):
        opts["skip_stdin_eof"] = True

    transport = await stdio_server(**opts)
    await server.connect(transport)

    def _shutdown(sig=None, frame=None):
        logger.info("MCP server shutting down...")
        sys.exit(0)

    signal.signal(signal.SIGTERM, _shutdown)
    signal.signal(signal.SIGINT, _shutdown)

    try:
        await asyncio.get_running_loop().create_future()
    except asyncio.CancelledError:
        pass


def _main():
    import argparse

    parser = argparse.ArgumentParser(description="Goat MCP stdio Server")
    parser.add_argument("--tools", type=str, default=None,
                        help="Comma-separated list of tools to expose")
    args = parser.parse_args()

    tool_names = None
    if args.tools:
        tool_names = [t.strip() for t in args.tools.split(",") if t.strip()]

    asyncio.run(start_mcp_server(tool_names=tool_names))


if __name__ == "__main__":
    _main()