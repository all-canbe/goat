from __future__ import annotations

import asyncio
import traceback
from dataclasses import dataclass, field
from pathlib import Path

from langchain_core.messages import (
    SystemMessage, HumanMessage, ToolMessage,
)
from langchain_core.tools import BaseTool
from langchain_openai import ChatOpenAI

from .cancellation import CancellationToken
from .subagent_manager import SubAgentManager, SubAgent, SubAgentStatus
from .subagent_roles import RoleType, RoleDefinition, get_role
from .skill_system import SkillRegistry
from .tools import get_tools_by_names
from .event_bus import EventBus, EventType, SubagentEvent
from .prompt_engine import engine as prompt_engine


MAX_AGENT_TURNS = 30


@dataclass
class AgentContext:
    llm: ChatOpenAI
    manager: SubAgentManager
    skill_registry: SkillRegistry
    event_bus: EventBus | None = None
    agent_id: str | None = None
    role_def: RoleDefinition | None = None
    spawn_depth: int = 0
    parent_id: str | None = None
    cancel_token: CancellationToken = field(default_factory=CancellationToken)
    parent_completion_event: asyncio.Event | None = None


def _build_subagent_tools(ctx: AgentContext) -> list[BaseTool]:
    """构建子 Agent 管理工具"""

    from langchain_core.tools import tool as langchain_tool

    @langchain_tool
    async def agent_spawn(role: str, task: str) -> str:
        """创建子 Agent 来并行处理子任务。

        Args:
            role: 子 Agent 角色类型，可选: general, explore, plan, implementer, review, verifier
            task: 分配给子 Agent 的任务描述
        """
        try:
            role_type = RoleType(role)
        except ValueError:
            valid = [r.value for r in RoleType]
            return f"错误: 无效的角色 '{role}'，可选: {valid}"

        try:
            agent = await ctx.manager.spawn(
                parent_id=ctx.agent_id,
                role_type=role_type,
                task_description=task,
                parent_cancel_token=ctx.cancel_token,
                spawn_depth=ctx.spawn_depth + 1,
            )
        except RuntimeError as e:
            return f"创建子 Agent 失败: {e}"

        agent_role_def = get_role(role_type)
        tools = get_tools_by_names(agent_role_def.allowed_tools)
        subagent_tools = _build_subagent_tools(
            AgentContext(
                llm=ctx.llm,
                manager=ctx.manager,
                skill_registry=ctx.skill_registry,
                event_bus=ctx.event_bus,
            )
        )
        if agent_role_def.can_spawn:
            tools = tools + subagent_tools

        agent.status = SubAgentStatus.RUNNING
        agent.task_handle = asyncio.create_task(
            _run_agent_loop(
                ctx=AgentContext(
                    llm=ctx.llm,
                    manager=ctx.manager,
                    skill_registry=ctx.skill_registry,
                    event_bus=ctx.event_bus,
                    agent_id=agent.agent_id,
                    role_def=agent_role_def,
                    spawn_depth=ctx.spawn_depth + 1,
                    parent_id=ctx.agent_id,
                    cancel_token=agent.cancel_token,
                    parent_completion_event=(
                        ctx.parent_completion_event
                        if ctx.spawn_depth + 1 == 1
                        else None
                    ),
                ),
                agent=agent,
                tools=tools,
                task=task,
            )
        )

        return (
            f"子 Agent 已创建并开始运行:\n"
            f"  ID: {agent.agent_id}\n"
            f"  名称: {agent.name}\n"
            f"  角色: {role}\n"
            f"  深度: {ctx.spawn_depth + 1}\n"
            f"  任务: {task}\n"
            f"使用 agent_list 查看所有子 Agent 状态"
        )

    @langchain_tool
    async def agent_eval(agent_id: str, message: str) -> str:
        """向指定的子 Agent 发送消息/指令。

        Args:
            agent_id: 子 Agent ID
            message: 发送的消息内容
        """
        return await ctx.manager.send_message(agent_id, message)

    @langchain_tool
    def agent_list() -> str:
        """列出所有子 Agent 及其状态。"""
        return ctx.manager.format_agent_list()

    @langchain_tool
    async def agent_collect(agent_ids: str = "") -> str:
        """收集已完成子 Agent 的输出结果。

        Args:
            agent_ids: 要收集的 Agent ID 列表，用逗号分隔，留空则收集所有 depth=1 的子 Agent
        """
        ids = [i.strip() for i in agent_ids.split(",") if i.strip()] if agent_ids else None
        return await ctx.manager.collect_results(ids)

    @langchain_tool
    async def agent_cancel(agent_id: str) -> str:
        """取消指定的子 Agent（会级联取消其所有子 Agent）。

        Args:
            agent_id: 要取消的 Agent ID
        """
        await ctx.manager.cancel(agent_id)
        return f"子 Agent {agent_id} 已取消"

    return [agent_spawn, agent_eval, agent_list, agent_collect, agent_cancel]


async def _publish(ctx: AgentContext, event_type: EventType, payload: str) -> None:
    if ctx.event_bus is None:
        return
    name = getattr(getattr(ctx, 'role_def', None), 'display_name', 'Agent') or 'Agent'
    await ctx.event_bus.publish(SubagentEvent(
        event_type=event_type,
        agent_id=ctx.agent_id or "",
        agent_name=name,
        payload=payload,
        depth=ctx.spawn_depth,
    ))


async def _run_agent_loop(ctx: AgentContext, agent: SubAgent,
                          tools: list[BaseTool], task: str) -> None:
    """子 Agent 的核心执行循环（带事件发布）"""
    try:
        role_type = ctx.role_def.role_type.value if ctx.role_def else "custom"
        system_prompt = prompt_engine.render_system(
            role_type,
            agent_id=agent.agent_id,
            agent_name=agent.name,
            depth=str(ctx.spawn_depth),
            cwd=str(Path.cwd()),
        )

        messages = [
            SystemMessage(content=system_prompt),
            HumanMessage(content=task),
        ]

        await _publish(ctx, EventType.STATUS_CHANGE, f"开始执行: {task[:100]}")

        llm_with_tools = ctx.llm.bind_tools(tools)

        for turn in range(MAX_AGENT_TURNS):
            if ctx.cancel_token.is_cancelled():
                await _publish(ctx, EventType.STATUS_CHANGE, "任务被取消")
                await ctx.manager.update_status(
                    agent.agent_id, SubAgentStatus.CANCELLED,
                    output="任务被取消",
                )
                return

            try:
                msg = await asyncio.wait_for(
                    agent.message_queue.get(), timeout=0.1,
                )
                messages.append(HumanMessage(
                    content=f"[来自父 Agent 的消息]\n{msg}"
                ))
            except asyncio.TimeoutError:
                pass

            try:
                response = await llm_with_tools.ainvoke(messages)
            except Exception as e:
                await _publish(ctx, EventType.ERROR, f"LLM 调用失败: {e}")
                await ctx.manager.update_status(
                    agent.agent_id, SubAgentStatus.FAILED,
                    error=f"LLM 调用失败: {e}",
                )
                return

            messages.append(response)
            content = str(response.content or "")
            if content:
                await _publish(ctx, EventType.LLM_RESPONSE, content[:200])

            if not response.tool_calls:
                output = content
                await _publish(ctx, EventType.COMPLETED, output[:200])
                await ctx.manager.update_status(
                    agent.agent_id, SubAgentStatus.COMPLETED,
                    output=output,
                )
                return

            tool_name_map = {t.name: t for t in tools}
            for tc in response.tool_calls:
                if ctx.cancel_token.is_cancelled():
                    await _publish(ctx, EventType.STATUS_CHANGE, "任务被取消")
                    await ctx.manager.update_status(
                        agent.agent_id, SubAgentStatus.CANCELLED,
                        output="任务被取消",
                    )
                    return

                tc_name = tc.get("name", "")
                tc_id = tc.get("id", "")
                tc_args = tc.get("args", {})

                await _publish(ctx, EventType.TOOL_CALL, f"{tc_name}({str(tc_args)[:100]})")

                tool = tool_name_map.get(tc_name)
                if tool is None:
                    result = f"工具不可用: {tc_name}"
                else:
                    try:
                        if asyncio.iscoroutinefunction(tool.ainvoke):
                            result = await tool.ainvoke(tc_args)
                        else:
                            result = tool.invoke(tc_args)
                    except Exception as e:
                        result = f"工具执行失败: {e}\n{traceback.format_exc()}"

                result_str = str(result) if not isinstance(result, str) else result
                await _publish(ctx, EventType.TOOL_RESULT, result_str[:200])
                messages.append(ToolMessage(
                    content=result_str, tool_call_id=tc_id,
                ))

        final_msg = f"达到最大轮次 ({MAX_AGENT_TURNS})"
        await _publish(ctx, EventType.COMPLETED, final_msg)
        await ctx.manager.update_status(
            agent.agent_id, SubAgentStatus.COMPLETED,
            output=final_msg,
        )

    except asyncio.CancelledError:
        await _publish(ctx, EventType.STATUS_CHANGE, "任务被取消")
        await ctx.manager.update_status(
            agent.agent_id, SubAgentStatus.CANCELLED,
            output="任务被取消",
        )
    except Exception as e:
        await _publish(ctx, EventType.ERROR, f"{e}")
        await ctx.manager.update_status(
            agent.agent_id, SubAgentStatus.FAILED,
            error=f"{e}\n{traceback.format_exc()}",
        )
    finally:
        if ctx.parent_completion_event is not None:
            ctx.parent_completion_event.set()


async def run_subagent(ctx: AgentContext, role_type: RoleType,
                       task: str) -> SubAgent:
    """启动一个子 Agent 并返回 Agent 对象（非阻塞）"""
    agent = await ctx.manager.spawn(
        parent_id=ctx.agent_id,
        role_type=role_type,
        task_description=task,
        parent_cancel_token=ctx.cancel_token,
        spawn_depth=ctx.spawn_depth + 1,
    )

    role_def = get_role(role_type)
    tools = get_tools_by_names(role_def.allowed_tools)
    subagent_tools = _build_subagent_tools(
        AgentContext(
            llm=ctx.llm,
            manager=ctx.manager,
            skill_registry=ctx.skill_registry,
            event_bus=ctx.event_bus,
        )
    )
    if role_def.can_spawn:
        tools = tools + subagent_tools

    agent.status = SubAgentStatus.RUNNING
    agent.task_handle = asyncio.create_task(
        _run_agent_loop(
            ctx=AgentContext(
                llm=ctx.llm,
                manager=ctx.manager,
                skill_registry=ctx.skill_registry,
                event_bus=ctx.event_bus,
                agent_id=agent.agent_id,
                role_def=role_def,
                spawn_depth=ctx.spawn_depth + 1,
                parent_id=ctx.agent_id,
                cancel_token=agent.cancel_token,
                parent_completion_event=(
                    ctx.parent_completion_event
                    if ctx.spawn_depth + 1 == 1
                    else None
                ),
            ),
            agent=agent,
            tools=tools,
            task=task,
        )
    )
    return agent