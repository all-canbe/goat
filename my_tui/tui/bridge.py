from __future__ import annotations

import asyncio
from typing import Optional, Callable

from my_tui.core.event_bus import EventBus, EventType, SubagentEvent
from .state import TUIState, MessageData, MessageRole, TokenUsage, SubAgentInfo, TaskInfo, ApprovalRequest


class TUIBridge:
    def __init__(self, event_bus: EventBus, state: TUIState):
        self._event_bus = event_bus
        self.state = state
        self._subscription_id = "tui_bridge"
        self._listener_task: Optional[asyncio.Task] = None
        self.on_state_changed: Optional[Callable[[], None]] = None
        self._approval_futures: dict[str, asyncio.Future] = {}

    def start(self):
        self._listener_task = asyncio.create_task(self._listen_loop())

    def stop(self):
        if self._listener_task and not self._listener_task.done():
            self._listener_task.cancel()
        self._event_bus.unsubscribe(self._subscription_id)

    async def _listen_loop(self):
        queue = await self._event_bus.stream(self._subscription_id)
        while True:
            try:
                event = await asyncio.wait_for(queue.get(), timeout=1.0)
            except asyncio.TimeoutError:
                continue
            except asyncio.CancelledError:
                break

            try:
                self._handle_event(event)
                self.state.bump()
                if self.on_state_changed:
                    self.on_state_changed()
            except Exception:
                pass
            finally:
                queue.task_done()

    def _handle_event(self, event: SubagentEvent):
        match event.event_type:
            case EventType.LLM_STREAM:
                self.state.streaming_content += event.payload
                if not self.state.is_streaming:
                    self.state.is_streaming = True

            case EventType.LLM_RESPONSE:
                self.state.is_streaming = False
                self.state.messages.append(
                    MessageData(
                        role=MessageRole.ASSISTANT,
                        content=self.state.streaming_content or event.payload,
                        agent_name=event.agent_name,
                    )
                )
                self.state.streaming_content = ""
                self.state.message_count += 1

            case EventType.TOOL_CALL:
                self.state.is_thinking = True
                self.state.messages.append(
                    MessageData(
                        role=MessageRole.TOOL_CALL,
                        content=event.payload,
                        agent_name=event.agent_name,
                    )
                )

            case EventType.TOOL_RESULT:
                self.state.messages.append(
                    MessageData(
                        role=MessageRole.TOOL_RESULT,
                        content=event.payload,
                        agent_name=event.agent_name,
                    )
                )

            case EventType.STATUS_CHANGE:
                parts = event.payload.split("|", 3)
                if len(parts) >= 2:
                    agent_id = event.agent_id
                    self.state.sub_agents[agent_id] = SubAgentInfo(
                        agent_id=agent_id,
                        name=event.agent_name,
                        role=parts[1] if len(parts) > 1 else "",
                        status=parts[0],
                        task=parts[2] if len(parts) > 2 else "",
                    )

            case EventType.COMPLETED:
                self.state.is_streaming = False
                self.state.is_thinking = False

            case EventType.ERROR:
                self.state.is_streaming = False
                self.state.messages.append(
                    MessageData(
                        role=MessageRole.ERROR,
                        content=event.payload,
                        agent_name=event.agent_name,
                    )
                )

            case EventType.MESSAGE:
                self.state.messages.append(
                    MessageData(
                        role=MessageRole.SYSTEM,
                        content=event.payload,
                    )
                )

    def set_mode(self, mode_name: str):
        from my_tui.security.approval import PermissionMode
        mode_map = {
            "plan": PermissionMode.PLAN,
            "agent": PermissionMode.DEFAULT,
            "yolo": PermissionMode.YOLO,
        }
        self.state.mode = mode_map.get(mode_name.lower(), PermissionMode.DEFAULT)
        self.state.bump()

    def add_user_message(self, content: str):
        self.state.messages.append(
            MessageData(role=MessageRole.USER, content=content)
        )
        self.state.message_count += 1
        self.state.bump()
        if self.on_state_changed:
            self.on_state_changed()

    def set_approval(self, req: ApprovalRequest | None):
        self.state.pending_approval = req
        self.state.bump()

    def start_approval(self, tool_name: str, args: dict, message: str,
                       tool_call_id: str = "") -> asyncio.Future:
        import uuid
        req_id = str(uuid.uuid4())
        future: asyncio.Future = asyncio.get_event_loop().create_future()
        self._approval_futures[req_id] = future

        self.state.pending_approval = ApprovalRequest(
            tool_name=tool_name,
            args=str(args),
            description=message,
            callback_approve=lambda: self._resolve_approval(req_id, True),
            callback_reject=lambda: self._resolve_approval(req_id, False),
        )
        self.state.bump()
        if self.on_state_changed:
            self.on_state_changed()
        return future

    def _resolve_approval(self, req_id: str, result: bool):
        future = self._approval_futures.pop(req_id, None)
        if future and not future.done():
            future.set_result(result)
        self.state.pending_approval = None
        self.state.bump()
        if self.on_state_changed:
            self.on_state_changed()