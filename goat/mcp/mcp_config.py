from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path


def _get_goat_home() -> Path:
    return Path(os.environ.get("GOAT_HOME", Path.home() / ".goat"))


@dataclass
class McpServerConnection:
    name: str
    command: str | None = None
    args: list[str] = field(default_factory=list)
    url: str | None = None
    env: dict = field(default_factory=dict)
    transport: str | None = None
    headers: dict[str, str] | None = None


@dataclass
class McpConfig:
    enabled: bool = False
    connections: list[McpServerConnection] = field(default_factory=list)
    tools_allowlist: list[str] | None = None
    tools_denylist: list[str] = field(default_factory=list)

    sse_enabled: bool = False
    sse_host: str = "127.0.0.1"
    sse_port: int = 8800
    sse_token: str | None = None

    rate_limit_enabled: bool = True
    ip_rate_limit: int = 30
    token_rate_limit: int = 60
    body_cap_bytes: int = 1_048_576


def get_config_path() -> str:
    p = os.environ.get("MCP_CONFIG_PATH")
    if p:
        return p
    cwd_path = str(Path.cwd() / ".goat" / "mcp_config.json")
    if os.path.exists(cwd_path):
        return cwd_path
    return str(_get_goat_home() / "mcp_config.json")


def load_mcp_config(config_path: str | None = None) -> McpConfig:
    if config_path is None:
        config_path = get_config_path()

    if not os.path.exists(config_path):
        return McpConfig()

    with open(config_path, "r", encoding="utf-8") as f:
        data = json.load(f)

    connections = []

    for name, server_def in data.get("mcpServers", {}).items():
        connections.append(McpServerConnection(
            name=name,
            command=server_def.get("command"),
            args=server_def.get("args", []),
            url=server_def.get("url"),
            env=server_def.get("env", {}),
            transport=server_def.get("transport"),
            headers=server_def.get("headers"),
        ))

    for conn_data in data.get("connections", []):
        connections.append(McpServerConnection(
            name=conn_data["name"],
            command=conn_data.get("command"),
            args=conn_data.get("args", []),
            url=conn_data.get("url"),
            env=conn_data.get("env", {}),
            transport=conn_data.get("transport"),
            headers=conn_data.get("headers"),
        ))

    return McpConfig(
        enabled=data.get("enabled", True) if connections else data.get("enabled", False),
        connections=connections,
        tools_allowlist=data.get("tools_allowlist"),
        tools_denylist=data.get("tools_denylist", []),
        sse_enabled=data.get("sse_enabled", False),
        sse_host=data.get("sse_host", "127.0.0.1"),
        sse_port=data.get("sse_port", 8800),
        sse_token=data.get("sse_token"),
        rate_limit_enabled=data.get("rate_limit_enabled", True),
        ip_rate_limit=data.get("ip_rate_limit", 30),
        token_rate_limit=data.get("token_rate_limit", 60),
        body_cap_bytes=data.get("body_cap_bytes", 1_048_576),
    )


def save_mcp_config(connections: list[McpServerConnection], config_path: str | None = None) -> str:
    if config_path is None:
        config_path = get_config_path()

    existing = {}
    if os.path.exists(config_path):
        with open(config_path, "r", encoding="utf-8") as f:
            existing = json.load(f)

    mcp_servers = {}
    for conn in connections:
        entry: dict = {}
        if conn.command:
            entry["command"] = conn.command
            entry["args"] = conn.args
        if conn.url:
            entry["url"] = conn.url
        if conn.env:
            entry["env"] = conn.env
        if conn.transport:
            entry["transport"] = conn.transport
        if conn.headers:
            entry["headers"] = conn.headers
        mcp_servers[conn.name] = entry

    existing["mcpServers"] = mcp_servers
    existing.pop("connections", None)
    existing["enabled"] = True

    os.makedirs(os.path.dirname(config_path), exist_ok=True)
    with open(config_path, "w", encoding="utf-8") as f:
        json.dump(existing, f, ensure_ascii=False, indent=2)

    return config_path