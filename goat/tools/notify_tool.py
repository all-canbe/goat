from __future__ import annotations

from enum import Enum
from typing import Type

from pydantic import BaseModel, Field
from langchain_core.tools import BaseTool

from goat.core.event_bus import EventBus, EventType, SubagentEvent


class NotificationLevel(str, Enum):
    INFO = "info"
    SUCCESS = "success"
    WARNING = "warning"
    ERROR = "error"


_NOTIFICATION_ICONS = {
    NotificationLevel.INFO: "\u2139\ufe0f",
    NotificationLevel.SUCCESS: "\u2705",
    NotificationLevel.WARNING: "\u26a0\ufe0f",
    NotificationLevel.ERROR: "\u274c",
}

_event_bus: EventBus | None = None


def _set_event_bus(bus: EventBus):
    global _event_bus
    _event_bus = bus


def _get_event_bus() -> EventBus:
    global _event_bus
    if _event_bus is None:
        _event_bus = EventBus()
    return _event_bus


class NotifyInput(BaseModel):
    message: str = Field(description="通知消息内容")
    level: str = Field(
        default="info",
        description="通知级别: info, success, warning, error",
    )
    title: str = Field(
        default="",
        description="可选的通知标题",
    )


class NotifyTool(BaseTool):
    name: str = "notify"
    description: str = (
        "发送通知给用户。用于通知用户重要事件、任务完成、错误提示或需要用户关注的信息。\n"
        "支持级别: info（普通信息）, success（成功）, warning（警告）, error（错误）\n"
        "典型用法: 任务完成时用 level='success' 提示用户，出现异常时?level='error' 提示"
    )
    args_schema: Type[BaseModel] = NotifyInput
    return_direct: bool = False

    def _run(self, message: str, level: str = "info", title: str = "") -> str:
        try:
            lvl = NotificationLevel(level)
        except ValueError:
            lvl = NotificationLevel.INFO

        icon = _NOTIFICATION_ICONS.get(lvl, "\u2139\ufe0f")
        display = f"{icon} [{lvl.value}] {title + ': ' if title else ''}{message}"

        bus = _get_event_bus()
        bus.publish_nowait(
            source_id="notify_tool",
            event_type=EventType.NOTIFICATION,
            payload=display,
            agent_name="system",
        )
        return f"通知已发? {display}"

    async def _arun(self, message: str, level: str = "info", title: str = "") -> str:
        return self._run(message, level, title)


NOTIFY_TOOL = NotifyTool()