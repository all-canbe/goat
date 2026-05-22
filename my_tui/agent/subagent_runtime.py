from __future__ import annotations

import asyncio
import traceback
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from langchain_core.messages import (
    SystemMessage, HumanMessage, ToolMessage,
)
from langchain_core.tools import BaseTool
from langchain_openai import ChatOpenAI

from ..core.cancellation import CancellationToken
from .subagent_manager import SubAgentManager, SubAgent, SubAgentStatus
from .subagent_roles import RoleType, RoleDefinition, get_role
from .skill_system import SkillRegistry
from ..tools.tools import get_tools_by_names
from ..core.event_bus import EventBus, EventType, SubagentEvent
from ..conversation.prompt_engine import engine as prompt_engine
from .structured_output import parse_structured_output

try:
    from ..conversation.conversation_manager import ConversationManager
    _HAS_CONVERSATIONS = True
except ImportError:
    ConversationManager = None
    _HAS_CONVERSATIONS = False

try:
    from ..security.approval import ToolApprovalSystem, Decision
    _HAS_APPROVAL = True
except ImportError:
    ToolApprovalSystem = None
    Decision = None
    _HAS_APPROVAL = False

import json


def _format_tool_action(name: str, args: dict) -> str:
    lines = []
    target = args.get("file_path") or args.get("filepath") or args.get("directory") or args.get("path") or ""
    if target:
        lines.append(f"    改哪里: {target}")
    if name in ("write_file", "edit_file", "write") and args.get("content"):
        preview = args["content"][:200].replace("\n", "\\n")
        lines.append(f"    改什么: {preview}")
    elif name in ("edit_file",) and args.get("new_str"):
        preview = args["new_str"][:200].replace("\n", "\\n")
        lines.append(f"    改什么: {preview}")
    return "\n".join(lines)


@dataclass
class AgentContext:
    agent_id: str
    agent_name: str
    role_type: RoleType
    role_def: RoleDefinition
    cancel_token: CancellationToken
    message_queue: asyncio.Queue[str]
    event_bus: EventBus
    llm: Any
    tools: list[BaseTool]
    skill_registry: SkillRegistry | None = None
    subagent_manager: SubAgentManager | None = None
    conversation_manager: ConversationManager | None = None
    approval_system: ToolApprovalSystem | None = None
    model_router: Any | None = None
    depth: int = 0
    verbose: bool = False
    metadata: dict = field(default_factory=dict)


MAX_AGENT_TURNS = 200


async def run_subagent(ctx: AgentContext) -> str:
    return await _run_agent_loop(ctx)


async def _run_agent_loop(ctx: AgentContext) -> str:
    role_type = ctx.role_type
    role_name = role_type.value
    depth = ctx.depth

    system_text = ctx.role_def.system_prompt
    if ctx.skill_registry:
        skill_block = ctx.skill_registry.to_prompt_block()
        if skill_block:
            system_text += "\n" + skill_block

    messages: list = [SystemMessage(content=system_text)]

    output_buffer: list[str] = []
    tool_results: list[tuple[str, str, str]] = []
    agent_states: dict[str, Any] = {}
    tools_map: dict[str, BaseTool] = {t.name: t for t in ctx.tools}
    tool_names_str = ", ".join(t.name for t in ctx.tools)

    for turn in range(MAX_AGENT_TURNS):
        if ctx.cancel_token.is_cancelled():
            return "已取消"

        try:
            ctx.event_bus and await ctx.event_bus.publish(
                SubagentEvent(EventType.LLM_RESPONSE, ctx.agent_id, ctx.agent_name,
                              f"[思考中...] 回合 {turn + 1}"))
            response = await ctx.llm.ainvoke(messages)
            assistant_msg = response
            messages.append(assistant_msg)

            content = response.content or ""
            if content.strip():
                output_buffer.append(f"[{ctx.agent_name}] {content[:300]}")

            tool_calls = getattr(response, "tool_calls", [])
            if not tool_calls:
                break

            if turn >= MAX_AGENT_TURNS - 1:
                messages.append(ToolMessage(
                    content=f"已达到最大执行轮次 ({MAX_AGENT_TURNS})，请总结当前结果。",
                    tool_call_id="limit",
                ))
                break

            for tc in tool_calls:
                if ctx.cancel_token.is_cancelled():
                    return "已取消"

                tool_name = tc["name"]
                tool_args = tc.get("args", {})
                tool_id = tc.get("id", "")

                ctx.event_bus and await ctx.event_bus.publish(
                    SubagentEvent(EventType.TOOL_CALL, ctx.agent_id, ctx.agent_name,
                                  _format_tool_action(tool_name, tool_args), depth))

                if tool_name in ("agent_spawn",):
                    result = await _handle_agent_spawn(ctx, tool_args, agent_states)
                elif tool_name in ("agent_eval",):
                    result = await _handle_agent_eval(ctx, tool_args)
                elif tool_name in ("agent_list",):
                    if ctx.subagent_manager:
                        result = ctx.subagent_manager.format_agent_list()
                    else:
                        result = "子 Agent 管理器不可用"
                elif tool_name in ("agent_collect",):
                    if ctx.subagent_manager:
                        target_ids = tool_args.get("agent_ids")
                        result = await ctx.subagent_manager.collect_results(target_ids)
                    else:
                        result = "子 Agent 管理器不可用"
                elif tool_name in ("agent_cancel",):
                    if ctx.subagent_manager:
                        agent_id = tool_args.get("agent_id", "")
                        result = await ctx.subagent_manager.cancel(agent_id)
                    else:
                        result = "子 Agent 管理器不可用"
                elif tool_name in tools_map:
                    langchain_tool = tools_map[tool_name]
                    if _HAS_APPROVAL and ctx.approval_system:
                        approval_result = await ctx.approval_system.request_tool_approval(
                            agent_id=ctx.agent_id,
                            tool_name=tool_name,
                            tool_args=tool_args,
                        )
                        if approval_result.decision != Decision.APPROVED:
                            result = f"工具调用被拒绝: {approval_result.reason or '无权限'}"
                        else:
                            result = await langchain_tool.ainvoke(tool_args)
                    else:
                        result = await langchain_tool.ainvoke(tool_args)
                else:
                    result = f"未知工具: {tool_name} (可用: {tool_names_str})"

                if isinstance(result, str) and len(result) > 30000:
                    preview = result[:2000]
                    tool_info = f"[结果过长 ({len(result)} 字符)，已截断为前 2000 字符]\n{preview}"
                else:
                    tool_info = str(result) if result else "(空结果)"

                messages.append(ToolMessage(content=tool_info, tool_call_id=tool_id))
                tool_results.append((tool_name, str(tool_args)[:100], tool_info[:200]))

                ctx.event_bus and await ctx.event_bus.publish(
                    SubagentEvent(EventType.TOOL_RESULT, ctx.agent_id, ctx.agent_name,
                                  f"  {tool_name}: {tool_info[:200]}", depth))

        except asyncio.CancelledError:
            return "已取消"
        except Exception as e:
            error_msg = f"[错误] {type(e).__name__}: {str(e)[:500]}"
            messages.append(ToolMessage(content=error_msg, tool_call_id="error"))
            if ctx.verbose:
                traceback.print_exc()

    full_output = "\n".join(output_buffer) if output_buffer else "(无文本输出)"
    if tool_results:
        summary_lines = ["\n工具调用摘要:"]
        for tn, ta, tr in tool_results[-10:]:
            summary_lines.append(f"  → {tn}({ta}) => {tr[:100]}")
        full_output += "\n".join(summary_lines)

    return full_output


async def _handle_agent_spawn(ctx: AgentContext, args: dict,
                              agent_states: dict[str, Any]) -> str:
    if ctx.subagent_manager is None:
        return "子 Agent 管理器不可用"

    description = args.get("description", "")
    role_name = args.get("role", "general")
    role_type = RoleType(role_name) if role_name in RoleType._value2member_map_ else RoleType.GENERAL

    try:
        child = await ctx.subagent_manager.spawn(
            parent_id=ctx.agent_id,
            role_type=role_type,
            task_description=description,
            parent_cancel_token=ctx.cancel_token,
            spawn_depth=ctx.depth + 1,
        )
    except RuntimeError as e:
        return str(e)

    skill_registry = ctx.skill_registry
    if ctx.model_router is not None:
        child_llm = ctx.model_router.get_llm_for_role(role_name)
    else:
        child_llm = ctx.llm
    child_ctx = AgentContext(
        agent_id=child.agent_id,
        agent_name=child.name,
        role_type=role_type,
        role_def=get_role(role_type),
        cancel_token=child.cancel_token,
        message_queue=child.message_queue,
        event_bus=ctx.event_bus,
        llm=child_llm,
        tools=ctx.tools,
        skill_registry=skill_registry,
        subagent_manager=ctx.subagent_manager,
        conversation_manager=ctx.conversation_manager,
        approval_system=ctx.approval_system,
        model_router=ctx.model_router,
        depth=ctx.depth + 1,
        verbose=ctx.verbose,
    )

    task = asyncio.create_task(_run_child_agent(child, child_ctx))
    child.task_handle = task
    return f"已创建子 Agent: {child.name} [{child.agent_id}]\n角色: {role_type.value}\n任务: {description[:200]}"


async def _run_child_agent(agent: SubAgent, ctx: AgentContext) -> None:
    try:
        agent.status = SubAgentStatus.RUNNING
        output = await _run_agent_loop(ctx)
        agent.status = SubAgentStatus.COMPLETED
        agent.output = output
        parsed = parse_structured_output(output)
        agent.metadata = parsed
        if ctx.event_bus is not None:
            summary = parsed.get("summary", "")
            await ctx.event_bus.publish(
                SubagentEvent(EventType.COMPLETED, ctx.agent_id, ctx.agent_name,
                              f"[结构化摘要] {summary[:200]}", ctx.depth))
    except asyncio.CancelledError:
        agent.status = SubAgentStatus.CANCELLED
    except Exception as e:
        agent.status = SubAgentStatus.FAILED
        agent.error = str(e)
    finally:
        agent.completion_event.set()


async def _handle_agent_eval(ctx: AgentContext, args: dict) -> str:
    if ctx.subagent_manager is None:
        return "子 Agent 管理器不可用"

    agent_id = args.get("agent_id", "")
    message = args.get("message", "")
    if not agent_id:
        return "错误: 缺少 agent_id"
    if not message:
        return "错误: 缺少 message"

    return await ctx.subagent_manager.send_message(agent_id, message)


def _build_subagent_tools(ctx: AgentContext) -> list[BaseTool]:
    return ctx.tools