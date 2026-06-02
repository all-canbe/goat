from __future__ import annotations

import asyncio
import json
import logging
from typing import Any

from fastapi import WebSocket

from goat.core.event_bus import EventBus, EventType, SubagentEvent

logger = logging.getLogger(__name__)


class WSManager:
    def __init__(self):
        self._connections: dict[str, list[WebSocket]] = {}
        self._session_manager = None

    def set_session_manager(self, mgr) -> None:
        self._session_manager = mgr

    async def connect(self, websocket: WebSocket, session_id: str) -> None:
        await websocket.accept()
        self._connections.setdefault(session_id, []).append(websocket)
        logger.info("WebSocket connected, session=%s, total_sessions=%s",
                     session_id[:8], len(self._connections))

    def disconnect(self, websocket: WebSocket, session_id: str) -> None:
        conns = self._connections.get(session_id, [])
        if websocket in conns:
            conns.remove(websocket)
        if not conns:
            self._connections.pop(session_id, None)
        logger.info("WebSocket disconnected, session=%s, remaining=%s",
                     session_id[:8], len(self._connections))

    async def broadcast_to_session(self, session_id: str, message: dict[str, Any]) -> None:
        dead: list[tuple[str, WebSocket]] = []
        for ws in self._connections.get(session_id, []):
            try:
                await ws.send_json(message)
            except Exception:
                dead.append((session_id, ws))
        for sid, ws in dead:
            conns = self._connections.get(sid, [])
            if ws in conns:
                conns.remove(ws)

    async def broadcast_to_all(self, message: dict[str, Any]) -> None:
        for session_id in list(self._connections.keys()):
            await self.broadcast_to_session(session_id, message)

    async def handle_message(self, websocket: WebSocket, data: dict[str, Any]) -> None:
        msg_type = data.get("type", "")
        payload = data.get("payload", {})
        session_id = data.get("session_id", "")

        if msg_type == "chat.send":
            await self._handle_chat_send(payload)
        elif msg_type == "chat.cancel":
            await self._handle_chat_cancel(payload)
        elif msg_type == "tool.approve":
            await self._handle_tool_approve(payload)
        elif msg_type == "tool.reject":
            await self._handle_tool_reject(payload)
        elif msg_type == "mode.change":
            await self._handle_mode_change(payload)
        elif msg_type == "session.switch":
            await self._handle_session_switch(payload)
        elif msg_type == "config.update":
            await self._handle_config_update(payload)
        elif msg_type == "ask_user.answer":
            await self._handle_ask_user_answer(payload)
        elif msg_type == "skills.install":
            await self._handle_skills_install(payload)
        else:
            logger.warning("Unknown message type: %s", msg_type)

    async def _handle_chat_send(self, payload: dict) -> None:
        text = payload.get("text", "")
        session_id = payload.get("sessionId", "default")
        if self._session_manager:
            await self._session_manager.handle_chat_send(text, session_id)
        else:
            logger.error("No session_manager set, cannot process chat.send")

    async def _handle_chat_cancel(self, payload: dict) -> None:
        session_id = payload.get("sessionId", "default")
        if self._session_manager:
            await self._session_manager.cancel_session(session_id)

    async def _handle_tool_approve(self, payload: dict) -> None:
        session_id = payload.get("sessionId", "default")
        if self._session_manager and hasattr(self._session_manager, '_engines'):
            engine = self._session_manager._engines.get(session_id)
            if engine and hasattr(engine, 'submit_approval'):
                engine.submit_approval("approved")
        logger.info("Tool approved for session %s", session_id[:8])

    async def _handle_tool_reject(self, payload: dict) -> None:
        session_id = payload.get("sessionId", "default")
        if self._session_manager and hasattr(self._session_manager, '_engines'):
            engine = self._session_manager._engines.get(session_id)
            if engine and hasattr(engine, 'submit_approval'):
                engine.submit_approval("rejected")
        logger.info("Tool rejected for session %s", session_id[:8])

    async def _handle_mode_change(self, payload: dict) -> None:
        mode = payload.get("mode", "agent")
        if self._session_manager:
            from goat.security.approval import PermissionMode
            mode_map = {
                "plan": PermissionMode.PLAN,
                "agent": PermissionMode.DEFAULT,
                "yolo": PermissionMode.YOLO,
                "flow": PermissionMode.FLOW,
            }
            resolved = mode_map.get(mode.lower(), PermissionMode.DEFAULT)
            if hasattr(self._session_manager, 'set_mode'):
                self._session_manager.set_mode(resolved)

    async def _handle_session_switch(self, payload: dict) -> None:
        session_id = payload.get("sessionId", "")
        logger.info("Session switch requested: %s", session_id)

    async def _handle_config_update(self, payload: dict) -> None:
        logger.info("Config update: %s", payload)

    async def _handle_ask_user_answer(self, payload: dict) -> None:
        answer = payload.get("answer", "")
        question_id = payload.get("questionId", "")
        logger.info("Ask user answer: %s -> %s", question_id, answer[:100])
        from goat.tools.ask_user_tool import submit_user_answer
        submit_user_answer(answer)

    async def _handle_skills_install(self, payload: dict) -> None:
        selections = payload.get("selections", [])
        session_id = payload.get("sessionId", "default")
        results: list[dict] = []

        for sel in selections:
            url = sel.get("url", "")
            scope = sel.get("scope", "project")
            if not url:
                continue
            try:
                cmd = f"npx skills add {url}"
                if scope == "global":
                    cmd += " --global"
                proc = await asyncio.create_subprocess_shell(
                    cmd,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                )
                stdout, stderr = await proc.communicate()
                if proc.returncode == 0:
                    results.append({"url": url, "success": True, "output": stdout.decode(errors="replace")})
                else:
                    results.append({"url": url, "success": False, "error": stderr.decode(errors="replace")})
            except Exception as e:
                results.append({"url": url, "success": False, "error": str(e)})

        if self._session_manager and hasattr(self._session_manager, 'skill_registry'):
            from goat.core.workspace import get_skills_dir
            skills_dir = get_skills_dir()
            if skills_dir.exists():
                self._session_manager.skill_registry.load_skills_from_directory(skills_dir)

        await self.broadcast_to_session(session_id, {
            "type": "skills.install.result",
            "payload": {"results": results}
        })


ws_manager = WSManager()


class EventBusBridge:
    def __init__(self, event_bus: EventBus, session_id: str = ""):
        self._event_bus = event_bus
        self._session_id = session_id
        self._sub_id = f"ws_bridge_{id(self)}"
        self._task: asyncio.Task | None = None

    def start(self) -> None:
        if self._session_id:
            self._event_bus.subscribe(self._sub_id, agent_id=None, session_id=self._session_id)
        else:
            self._event_bus.subscribe(self._sub_id, agent_id=None)
        self._task = asyncio.create_task(self._poll_loop())

    async def _poll_loop(self) -> None:
        queue = await self._event_bus.stream(self._sub_id)
        while True:
            event = await queue.get()
            msg = self._translate(event)
            if msg:
                await ws_manager.broadcast_to_all(msg)

    def _translate(self, event: SubagentEvent) -> dict[str, Any] | None:
        sid = event.session_id or self._session_id
        if event.event_type == EventType.LLM_STREAM:
            return {"type": "chat.stream", "payload": {"token": event.payload, "messageId": "", "sessionId": sid}}
        if event.event_type == EventType.LLM_RESPONSE:
            return {"type": "chat.response", "payload": {"messageId": "", "content": event.payload, "sessionId": sid}}
        if event.event_type == EventType.ERROR:
            return {"type": "chat.error", "payload": {"messageId": "", "error": event.payload, "sessionId": sid}}
        if event.event_type == EventType.COMPLETED:
            try:
                data = json.loads(event.payload) if isinstance(event.payload, str) else event.payload
                if isinstance(data, dict):
                    mode = data.get("mode", event.payload)
                    token_count = data.get("tokenCount", 0)
                    token_input = data.get("tokenInput", 0)
                    token_output = data.get("tokenOutput", 0)
                    cost = data.get("cost", 0)
                    message_count = data.get("messageCount", 0)
                else:
                    mode = event.payload
                    token_count = 0
                    token_input = 0
                    token_output = 0
                    cost = 0
                    message_count = 0
            except (json.JSONDecodeError, TypeError):
                mode = event.payload
                token_count = 0
                token_input = 0
                token_output = 0
                cost = 0
                message_count = 0
            return {"type": "status.update", "payload": {"model": event.agent_name, "provider": "", "mode": mode, "tokenCount": token_count, "tokenInput": token_input, "tokenOutput": token_output, "cost": cost, "messageCount": message_count, "toolRunning": False, "isThinking": False, "hasPendingApproval": False, "sessionId": sid}}
        if event.event_type == EventType.TOOL_CALL:
            try:
                data = json.loads(event.payload) if isinstance(event.payload, str) else event.payload
                return {"type": "tool.require_approval", "payload": {"toolCallId": data.get("id", ""), "toolName": data.get("name", ""), "args": data.get("args", {}), "description": data.get("description", ""), "riskLevel": data.get("riskLevel", ""), "diffContent": data.get("diffContent", ""), "sessionId": sid}}
            except (json.JSONDecodeError, TypeError):
                return {"type": "tool.require_approval", "payload": {"toolCallId": "", "toolName": event.payload, "args": {}, "description": "", "riskLevel": "", "diffContent": "", "sessionId": sid}}
        if event.event_type == EventType.TOOL_RESULT:
            try:
                data = json.loads(event.payload) if isinstance(event.payload, str) else event.payload
                return {"type": "tool.complete", "payload": {"toolName": data.get("name", ""), "result": data.get("result", ""), "sessionId": sid}}
            except (json.JSONDecodeError, TypeError):
                return {"type": "tool.complete", "payload": {"toolName": "", "result": event.payload, "sessionId": sid}}
        if event.event_type == EventType.MESSAGE:
            return {"type": "chat.system", "payload": {"messageId": "", "content": event.payload, "sessionId": sid}}
        if event.event_type == EventType.NOTIFICATION:
            return {"type": "status.notification", "payload": {"text": event.payload, "sessionId": sid}}
        if event.event_type == EventType.TOOL_RETRY:
            return {"type": "tool.retry", "payload": {"toolName": event.agent_name, "message": event.payload, "sessionId": sid}}
        if event.event_type == EventType.SUBAGENT_LIFECYCLE:
            return {"type": "subagent.lifecycle", "payload": {"agentName": event.agent_name, "status": event.payload, "sessionId": sid}}
        if event.event_type == EventType.AUDIT_LOG:
            return {"type": "audit.log", "payload": {"content": event.payload, "sessionId": sid}}
        if event.event_type == EventType.ASK_USER:
            return {"type": "ask_user", "payload": {"questionId": event.agent_id, "question": event.payload, "sessionId": sid}}
        return None

    def stop(self) -> None:
        if self._task:
            self._task.cancel()


def create_event_bridge(event_bus: EventBus, session_id: str = "") -> EventBusBridge:
    bridge = EventBusBridge(event_bus, session_id)
    bridge.start()
    return bridge