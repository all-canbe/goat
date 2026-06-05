from __future__ import annotations

from enum import Enum
from dataclasses import dataclass, field

from ..conversation.prompt_engine import engine as prompt_engine


class RoleType(Enum):
    GENERAL = "general"
    EXPLORE = "explore"
    PLAN = "plan"
    IMPLEMENTER = "implementer"
    REVIEW = "review"
    VERIFIER = "verifier"
    CUSTOM = "custom"


@dataclass
class RoleDefinition:
    role_type: RoleType
    display_name: str
    system_prompt: str = ""
    allowed_tools: list[str] = field(default_factory=list)
    can_spawn: bool = True
    max_spawn_depth: int = 3


ROLE_REGISTRY: dict[RoleType, RoleDefinition] = {
    RoleType.GENERAL: RoleDefinition(
        role_type=RoleType.GENERAL,
        display_name="通用助手",
        allowed_tools=[
            "list_files", "read_file", "write_file", "search_code",
            "async_execute_command", "agent_spawn", "agent_eval", "agent_list",
            "agent_collect", "agent_cancel",
            "git_status", "git_diff", "git_log", "git_commit",
            "web_run", "web_screenshot",
            "ask_user", "apply_patch",
        ],
        can_spawn=True,
    ),
    RoleType.EXPLORE: RoleDefinition(
        role_type=RoleType.EXPLORE,
        display_name="代码探索",
        allowed_tools=["list_files", "read_file", "search_code"],
        can_spawn=False,
    ),
    RoleType.PLAN: RoleDefinition(
        role_type=RoleType.PLAN,
        display_name="任务规划",
        allowed_tools=["list_files", "read_file", "save_plan_doc", "search_code"],
        can_spawn=False,
    ),
    RoleType.IMPLEMENTER: RoleDefinition(
        role_type=RoleType.IMPLEMENTER,
        display_name="代码实现",
        allowed_tools=["read_file", "write_file", "apply_patch", "search_code", "async_execute_command",
                       "git_status", "git_diff", "git_log", "git_commit",
                       "web_run", "web_screenshot"],
        can_spawn=False,
    ),
    RoleType.REVIEW: RoleDefinition(
        role_type=RoleType.REVIEW,
        display_name="代码审查",
        allowed_tools=["read_file", "search_code"],
        can_spawn=False,
    ),
    RoleType.VERIFIER: RoleDefinition(
        role_type=RoleType.VERIFIER,
        display_name="测试验证",
        allowed_tools=["read_file", "async_execute_command", "search_code"],
        can_spawn=False,
    ),
    RoleType.CUSTOM: RoleDefinition(
        role_type=RoleType.CUSTOM,
        display_name="自定义",
        allowed_tools=[
            "list_files", "read_file", "write_file", "search_code",
            "async_execute_command",
        ],
        can_spawn=False,
    ),
}


GOAT_NICKNAMES = [
    "Ibex", "Markhor", "Boer", "Saanen", "Alpine",
    "Nubian", "Angora", "Kiko", "Pygmy", "Cashmere",
]


def get_role(role_type: RoleType, **kwargs) -> RoleDefinition:
    role = ROLE_REGISTRY[role_type]
    role.system_prompt = prompt_engine.render_system(
        role_type.value, **kwargs,
    )
    return role


def list_roles() -> list[RoleDefinition]:
    return list(ROLE_REGISTRY.values())


def get_tools_for_role(role_type: RoleType) -> list[str]:
    return ROLE_REGISTRY[role_type].allowed_tools