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
from ..hooks.lifecycle import HookLifecycleSystem, HookDecision
from ..tools.steps_tracker import StepsTracker

try:
    from ..conversation.conversation_manager import ConversationManager
    _HAS_CONVERSATIONS = True
except ImportError:
    ConversationManager = None
    _HAS_CONVERSATIONS = False

try:
    from ..security.approval import ToolApprovalSystem, Decision, PermissionMode
    _HAS_APPROVAL = True
except ImportError:
    ToolApprovalSystem = None
    Decision = None
    PermissionMode = None
    _HAS_APPROVAL = False

import json


class ToolCallDeduper:
    """检测工具调用的重复循环，触发熔断让 Agent 更换策略。"""
    def __init__(self):
        self.history: list[tuple[str, frozenset]] = []

    @staticmethod
    def _fingerprint(tool_name: str, params: dict) -> tuple[str, frozenset]:
        items = sorted((k, str(v)) for k, v in params.items())
        return (tool_name, frozenset(items))

    def register_call(self, tool_name: str, params: dict):
        self.history.append(self._fingerprint(tool_name, params))

    def is_duplicate_loop(self, tool_name: str, params: dict) -> bool:
        fp = self._fingerprint(tool_name, params)
        recent = self.history[-5:]
        return recent.count(fp) >= 3


def _format_tool_action(name: str, args: dict) -> str:
    return json.dumps({"tool_name": name, "args": args}, ensure_ascii=False)


def _stream_command_output(ctx: AgentContext, result: str) -> None:
    in_stderr = False
    for line in result.splitlines():
        if line.strip() == "[stderr]":
            in_stderr = True
            continue
        etype = EventType.TOOL_STDERR if in_stderr else EventType.TOOL_STDOUT
        ctx.event_bus.publish_nowait(
            ctx.agent_id, etype, line, ctx.agent_name, ctx.depth,
        )


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
    hook_system: HookLifecycleSystem | None = None
    metadata: dict = field(default_factory=dict)
    steps_tracker: StepsTracker | None = None


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

    # P1: 显式 Plan 追踪 — 将进度报告注入 System Prompt
    if ctx.steps_tracker:
        progress = ctx.steps_tracker.get_progress()
        if progress:
            system_text += "\n\n" + progress + "\n"
        system_text += (
            "\n## 计划追踪\n"
            "你可以使用以下方式来汇报你的计划进度：\n"
            "- 在回复中输出 `## 计划` 后跟步骤列表来制定计划\n"
            "- 完成一个步骤后，输出 `## 进度: N` (N 为步骤编号，从 1 开始) 来标记完成\n"
        )

    messages: list = [SystemMessage(content=system_text)]

    # P2: 工具调用去重与熔断
    deduper = ToolCallDeduper()

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

            # P1: 解析进度更新
            if ctx.steps_tracker and content:
                import re as _re
                plan_match = _re.search(r"## 计划\n(.+?)(?:\n##|\Z)", content, _re.DOTALL)
                if plan_match:
                    steps = [s.strip().lstrip("- ").lstrip("* ") for s in plan_match.group(1).strip().split("\n") if s.strip()]
                    if steps:
                        ctx.steps_tracker.add_plan(steps)
                progress_match = _re.findall(r"## 进度:\s*(\d+)", content)
                for pm in progress_match:
                    ctx.steps_tracker.mark_done(int(pm) - 1)

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

                # P2: 熔断检测
                deduper.register_call(tool_name, tool_args)
                if deduper.is_duplicate_loop(tool_name, tool_args):
                    msg = f"检测到重复调用 {tool_name}，请更换策略"
                    messages.append(ToolMessage(content=msg, tool_call_id=tool_id))
                    tool_results.append((tool_name, str(tool_args)[:100], msg[:200]))
                    ctx.event_bus and await ctx.event_bus.publish(
                        SubagentEvent(EventType.TOOL_RESULT, ctx.agent_id, ctx.agent_name,
                                      f"  {msg}", depth))
                    continue

                if ctx.hook_system:
                    hook_cmd = str(tool_args.get("command", tool_args.get("filepath", "")))
                    hook_target = str(tool_args.get("file_path", tool_args.get("filepath", tool_args.get("directory", ""))))
                    hook_output = await ctx.hook_system.on_pre_tool_use(
                        tool_name, tool_args, command=hook_cmd, target_path=hook_target,
                    )
                    if hook_output.decision == HookDecision.BLOCK:
                        result = f"工具 '{tool_name}' 被钩子系统阻止: {hook_output.reason}"
                        tool_info = str(result)
                        messages.append(ToolMessage(content=tool_info, tool_call_id=tool_id))
                        tool_results.append((tool_name, str(tool_args)[:100], tool_info[:200]))
                        ctx.event_bus and await ctx.event_bus.publish(
                            SubagentEvent(EventType.TOOL_RESULT, ctx.agent_id, ctx.agent_name,
                                          f"  {tool_name}: {tool_info[:200]}", depth))
                        continue

                if tool_name in ("agent_spawn",):
                    if ctx.hook_system:
                        await ctx.hook_system.on_subagent_start(
                            tool_args.get("role", "general"),
                            tool_args.get("description", ""),
                        )
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
                    if ctx.cancel_token.is_cancelled():
                        messages.append(ToolMessage(content="已取消", tool_call_id=tool_id))
                        continue
                    langchain_tool = tools_map[tool_name]
                    try:
                        if _HAS_APPROVAL and ctx.approval_system:
                            approval_result = await ctx.approval_system.request_tool_approval(
                                agent_id=ctx.agent_id,
                                tool_name=tool_name,
                                tool_args=tool_args,
                            )
                            if approval_result.decision != Decision.ALLOW:
                                result = f"工具调用被拒绝: {approval_result.message or '无权限'}"
                            else:
                                if ctx.cancel_token.is_cancelled():
                                    result = "已取消"
                                else:
                                    result = await langchain_tool.ainvoke(tool_args)
                        else:
                            if ctx.cancel_token.is_cancelled():
                                result = "已取消"
                            else:
                                result = await langchain_tool.ainvoke(tool_args)
                        if ctx.hook_system:
                            await ctx.hook_system.on_post_tool_use(tool_name, tool_args, str(result))
                    except Exception as e:
                        result = f"工具执行失败: {e}"
                        if ctx.hook_system:
                            await ctx.hook_system.on_post_tool_use_failure(tool_name, tool_args, str(e))
                else:
                    result = f"未知工具: {tool_name} (可用: {tool_names_str})"
                    if ctx.hook_system:
                        await ctx.hook_system.on_post_tool_use(tool_name, tool_args, result)

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

                if ctx.event_bus and tool_name in (
                    "execute_command", "async_execute_command",
                ) and isinstance(result, str):
                    _stream_command_output(ctx, result)

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

    parsed = parse_structured_output(full_output)
    completeness = sum(1 for v in parsed.values() if v != "(未提供)")
    if completeness == 0 and tool_results:
        tool_summary_parts = []
        for tn, _, tr in tool_results[-5:]:
            tool_summary_parts.append(f"- {tn}: {tr[:80]}")
        full_output += "\n\n### SUMMARY\n" + "\n".join(tool_summary_parts) + "\n### CHANGES\n(由守卫自动生成)\n### EVIDENCE\n(见工具调用结果)\n### RISKS\n(未提供)\n### BLOCKERS\n(未提供)"

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
        hook_system=ctx.hook_system,
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
        if ctx.hook_system:
            await ctx.hook_system.on_subagent_stop(ctx.role_type.value, output[:200])
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