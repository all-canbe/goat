from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from fastapi import APIRouter, HTTPException, Query

from goat.api.chat_handler import web_chat_handler
from goat.api.websocket import ws_manager
from goat.mcp.mcp_config import (
    load_mcp_config,
    save_mcp_config,
    McpServerConnection,
)
from goat.provider.provider import (
    get_available_providers,
    ProviderConfig,
    ProviderType,
    create_llm,
    parse_provider,
)

router = APIRouter(prefix="/api")


@router.get("/health")
async def health_check():
    return {"status": "ok"}


@router.get("/config")
async def get_config():
    if web_chat_handler is None or web_chat_handler.provider_config is None:
        return {
            "provider_type": None,
            "base_url": None,
            "model": None,
            "api_key": None,
            "availableProviders": get_available_providers(),
            "mode": "Agent",
        }
    cfg = web_chat_handler.provider_config
    raw_key = cfg.api_key or ""
    masked = raw_key[:4] + "****" + raw_key[-4:] if len(raw_key) > 8 else "****"
    return {
        "provider_type": cfg.provider_type.value,
        "base_url": cfg.base_url or "",
        "model": cfg.model or "",
        "api_key": masked,
        "availableProviders": get_available_providers(),
        "mode": "Agent",
    }


@router.post("/config")
async def update_config(payload: dict):
    if web_chat_handler is None:
        return {"success": False, "error": "WebChatHandler not initialized"}
    provider_type_str = payload.get("provider_type", "openai_compatible")
    provider_type = parse_provider(provider_type_str) or ProviderType.OPENAI_COMPATIBLE
    new_config = ProviderConfig(
        provider_type=provider_type,
        api_key=payload.get("api_key", ""),
        base_url=payload.get("base_url", ""),
        model=payload.get("model", ""),
    )
    web_chat_handler.provider_config = new_config
    web_chat_handler.llm = create_llm(new_config)
    return {"success": True}


@router.get("/workspace")
async def get_workspace():
    if web_chat_handler is None:
        return {"workspace": str(Path.cwd().resolve())}
    return {"workspace": str(web_chat_handler.workspace)}


@router.put("/workspace")
async def update_workspace(payload: dict):
    if web_chat_handler is None:
        return {"success": False, "error": "WebChatHandler not initialized"}
    path = payload.get("path", "")
    if not path:
        return {"success": False, "error": "path is required"}
    ok, msg = web_chat_handler.set_workspace(path)
    if ok:
        return {"success": True, "workspace": msg}
    raise HTTPException(status_code=400, detail=msg)


@router.get("/skills")
async def list_skills():
    if web_chat_handler is None:
        return {"skills": []}
    reg = getattr(web_chat_handler, 'skill_registry', None)
    if reg is None:
        return {"skills": []}
    return {
        "skills": [
            {"name": s.name, "description": s.description}
            for s in reg.list_all()
        ]
    }


@router.post("/skills/reload")
async def reload_skills():
    if web_chat_handler is None:
        return {"success": False, "error": "WebChatHandler not initialized"}
    reg = getattr(web_chat_handler, 'skill_registry', None)
    if reg is None:
        return {"success": False, "error": "SkillRegistry not available"}
    from goat.core.workspace import get_skills_dir
    skills_dir = get_skills_dir()
    if skills_dir.exists():
        reg.load_skills_from_directory(skills_dir)
    return {"success": True, "count": len(reg.list_all())}


@router.get("/sessions")
async def list_sessions():
    if web_chat_handler is None or web_chat_handler.conversations is None:
        return {"sessions": []}
    sessions = web_chat_handler.conversations.list_sessions()
    running_sessions: set[str] = set()
    if hasattr(web_chat_handler, '_engines'):
        running_sessions = {
            sid for sid, engine in web_chat_handler._engines.items()
            if engine.is_running
        }
    return {
        "sessions": [
            {
                "id": s.session_id,
                "title": s.title,
                "createdAt": s.created_at,
                "messageCount": s.message_count,
                "isRunning": s.session_id in running_sessions,
            }
            for s in sessions
        ]
    }


@router.post("/sessions")
async def create_session():
    if web_chat_handler is None or web_chat_handler.conversations is None:
        return {"sessionId": "", "title": ""}
    model = web_chat_handler.provider_config.model if web_chat_handler.provider_config else ""
    session_id = await web_chat_handler.conversations.create_session(model=model)
    return {"sessionId": session_id, "title": "新对话"}


@router.post("/sessions/{session_id}/cancel")
async def cancel_session(session_id: str):
    if web_chat_handler is not None and hasattr(web_chat_handler, 'cancel_session'):
        await web_chat_handler.cancel_session(session_id)
    return {"cancelled": session_id}


@router.get("/sessions/{session_id}/status")
async def get_session_status(session_id: str):
    status = "idle"
    if web_chat_handler is not None and hasattr(web_chat_handler, '_engines'):
        engine = web_chat_handler._engines.get(session_id)
        if engine and engine.is_running:
            status = "running"
    return {"sessionId": session_id, "status": status}


@router.put("/sessions/{session_id}")
async def rename_session(session_id: str, payload: dict):
    if web_chat_handler is None or web_chat_handler.conversations is None:
        return {"success": False}
    title = payload.get("title", "")
    web_chat_handler.conversations.rename_session(session_id, title)
    return {"success": True}


@router.delete("/sessions/{session_id}")
async def delete_session(session_id: str):
    if web_chat_handler is None or web_chat_handler.conversations is None:
        return {"deleted": session_id}
    web_chat_handler.conversations.delete_session(session_id)
    return {"deleted": session_id}


@router.get("/sessions/{session_id}/messages")
async def get_messages(session_id: str):
    if web_chat_handler is None or web_chat_handler.conversations is None:
        return {"sessionId": session_id, "messages": []}
    messages = web_chat_handler.conversations.get_session_messages(session_id)

    records = list(messages)
    records.reverse()

    tool_results: list[dict] = []
    assistant_tool_call_indices: list[int] = []

    for idx, m in enumerate(records):
        meta = m.metadata or {}
        if m.role == "assistant" and meta.get("tool_calls"):
            assistant_tool_call_indices.append(idx)
        elif m.role == "tool":
            tool_results.append({
                "tool_call_id": meta.get("tool_call_id", ""),
                "result": m.content,
            })

    tool_call_map: dict[int, list[dict]] = {}
    used_indices: set[int] = set()

    for ai_idx in assistant_tool_call_indices:
        m = records[ai_idx]
        meta = m.metadata or {}
        paired = []

        for tc in meta.get("tool_calls", []):
            tc_id = tc.get("id", "")
            result_text = ""
            found = -1

            if tc_id:
                for ri, r in enumerate(tool_results):
                    if ri not in used_indices and r["tool_call_id"] == tc_id:
                        result_text = r["result"]
                        found = ri
                        break

            if found < 0:
                for ri, r in enumerate(tool_results):
                    if ri not in used_indices:
                        result_text = r["result"]
                        found = ri
                        break

            if found >= 0:
                used_indices.add(found)

            paired.append({
                "name": tc.get("name", ""),
                "args": tc.get("args", {}),
                "result": result_text,
                "status": "complete",
            })
        tool_call_map[ai_idx] = paired

    return {
        "sessionId": session_id,
        "messages": [
            {
                "id": m.id,
                "role": m.role,
                "content": m.content,
                "createdAt": m.created_at,
                "metadata": m.metadata or {},
                "toolCalls": tool_call_map.get(idx, None),
            }
            for idx, m in enumerate(records)
        ],
    }


@router.get("/mcp/servers")
async def list_mcp_servers():
    cfg = load_mcp_config()
    return {
        "servers": [
            {
                "name": c.name,
                "command": c.command,
                "args": c.args,
                "url": c.url,
                "env": c.env,
                "transport": c.transport,
            }
            for c in cfg.connections
        ]
    }


@router.post("/mcp/servers")
async def add_mcp_server(payload: dict):
    name = payload.get("name", "")
    if not name:
        return {"success": False, "error": "name is required"}
    cfg = load_mcp_config()
    for c in cfg.connections:
        if c.name == name:
            return {"success": False, "error": f"server '{name}' already exists"}
    server = McpServerConnection(
        name=name,
        command=payload.get("command"),
        args=payload.get("args", []),
        url=payload.get("url"),
        env=payload.get("env", {}),
        transport=payload.get("transport"),
    )
    cfg.connections.append(server)
    save_mcp_config(cfg.connections)
    return {"success": True}


@router.put("/mcp/servers/{name}")
async def update_mcp_server(name: str, payload: dict):
    cfg = load_mcp_config()
    for c in cfg.connections:
        if c.name == name:
            c.command = payload.get("command", c.command)
            c.args = payload.get("args", c.args)
            c.url = payload.get("url", c.url)
            c.env = payload.get("env", c.env)
            c.transport = payload.get("transport", c.transport)
            save_mcp_config(cfg.connections)
            return {"success": True}
    return {"success": False, "error": f"server '{name}' not found"}


@router.delete("/mcp/servers/{name}")
async def delete_mcp_server(name: str):
    cfg = load_mcp_config()
    cfg.connections = [c for c in cfg.connections if c.name != name]
    save_mcp_config(cfg.connections)
    return {"deleted": name}


@router.get("/files/read")
async def read_file(path: str = Query(...)):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            content = f.read()
        return {
            "path": path,
            "content": content,
            "lineCount": content.count("\n") + 1,
        }
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"File not found: {path}")
    except PermissionError:
        raise HTTPException(status_code=403, detail=f"Permission denied: {path}")


@router.post("/sessions/{session_id}/fork")
async def fork_session(session_id: str, payload: dict = None):
    if web_chat_handler is None or web_chat_handler.conversations is None:
        return {"forkedId": ""}
    turn = (payload or {}).get("turn", 10)
    messages = web_chat_handler.conversations.get_session_messages(session_id)
    recent = messages[-turn:] if len(messages) > turn else messages
    model = web_chat_handler.provider_config.model if web_chat_handler.provider_config else ""
    new_id = await web_chat_handler.conversations.create_session(model=model)
    for m in recent:
        web_chat_handler.conversations.add_message(new_id, m.role, m.content)
    return {"forkedId": new_id, "title": f"Forked from {session_id[:8]}"}


@router.get("/sessions/{session_id}/export")
async def export_session(session_id: str, format: str = Query("json")):
    if web_chat_handler is None or web_chat_handler.conversations is None:
        return {"sessionId": session_id, "messages": []}
    messages = web_chat_handler.conversations.get_session_messages(session_id)
    if format == "md":
        lines = [f"# Session: {session_id}", ""]
        for m in messages:
            role_label = {"user": "**User**", "assistant": "**Assistant**", "system": "**System**"}.get(m.role, "**" + m.role + "**")
            lines.append(f"### {role_label}")
            lines.append(m.content)
            lines.append("")
        return {"sessionId": session_id, "format": "md", "content": "\n".join(lines)}
    return {
        "sessionId": session_id,
        "messages": [{"id": m.id, "role": m.role, "content": m.content, "createdAt": m.created_at} for m in messages],
    }


@router.get("/files/tree")
async def get_file_tree(path: str | None = None):
    if path is None:
        if web_chat_handler is not None:
            root = str(web_chat_handler.workspace)
        else:
            root = "."
    else:
        root = path

    def build_tree(dir_path: str, max_depth: int = 3) -> list:
        if max_depth <= 0:
            return []
        result = []
        try:
            entries = sorted(os.scandir(dir_path), key=lambda e: (not e.is_dir(), e.name.lower()))
            for entry in entries:
                if entry.name.startswith('.') or entry.name in ('node_modules', '__pycache__', '.git', 'dist', 'build'):
                    continue
                node = {
                    "name": entry.name,
                    "path": entry.path.replace("\\", "/"),
                    "type": "directory" if entry.is_dir() else "file",
                }
                if entry.is_dir():
                    node["children"] = build_tree(entry.path, max_depth - 1)
                result.append(node)
        except (PermissionError, OSError):
            pass
        return result

    return {"tree": build_tree(root), "root": os.path.abspath(root).replace("\\", "/")}