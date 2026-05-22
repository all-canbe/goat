from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Optional

from my_tui.security.approval import PermissionMode


class MessageRole(str, Enum):
    USER = "user"
    ASSISTANT = "assistant"
    TOOL_CALL = "tool_call"
    TOOL_RESULT = "tool_result"
    SYSTEM = "system"
    ERROR = "error"


@dataclass
class MessageData:
    role: MessageRole
    content: str
    agent_name: str = ""
    metadata: dict = field(default_factory=dict)
    collapsed: bool = False


@dataclass
class TokenUsage:
    input_tokens: int = 0
    output_tokens: int = 0
    cache_hit_tokens: int = 0
    total_cost: float = 0.0

    @property
    def total_tokens(self) -> int:
        return self.input_tokens + self.output_tokens


@dataclass
class ApprovalRequest:
    tool_name: str
    args: str
    description: str = ""
    callback_approve: callable = lambda: None
    callback_reject: callable = lambda: None
    callback_edit: callable = lambda: None


@dataclass
class SubAgentInfo:
    agent_id: str
    name: str
    role: str
    status: str
    depth: int = 0
    task: str = ""


@dataclass
class TaskInfo:
    task_id: str
    name: str
    status: str
    progress: float = 0.0
    current_step: str = ""


@dataclass
class TUIState:
    version: int = 0
    connected: bool = False
    provider_name: str = ""
    model_name: str = ""

    mode: PermissionMode = PermissionMode.DEFAULT
    is_command_mode: bool = False
    is_streaming: bool = False
    is_thinking: bool = False
    streaming_content: str = ""

    messages: list[MessageData] = field(default_factory=list)
    sub_agents: dict[str, SubAgentInfo] = field(default_factory=dict)
    tasks: dict[str, TaskInfo] = field(default_factory=dict)

    token_usage: TokenUsage = field(default_factory=TokenUsage)
    context_usage_pct: float = 0.0

    pending_approval: Optional[ApprovalRequest] = None

    session_title: str = "默认会话"
    session_id: str = ""
    message_count: int = 0

    def bump(self):
        self.version += 1