#!/usr/bin/env python3
"""
Goat TUI 入口 — 红黑主题的终端 AI 编程助手

用法:
    python tui_main.py [--provider openai_compatible] [--model gpt-4o]

环境变量:
    API_KEY, BASE_URL, MODEL
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import traceback
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")

from langchain_core.messages import (
    SystemMessage, HumanMessage, ToolMessage,
)
from langchain_openai import ChatOpenAI

from my_tui.core.event_bus import EventBus, EventType
from my_tui.core.cancellation import CancellationToken
from my_tui.core.token_tracker import TokenTracker
from my_tui.conversation.conversation_manager import ConversationManager
from my_tui.conversation.context_compression import CompactionConfig
from my_tui.conversation.prompt_engine import engine as prompt_engine
from my_tui.agent.subagent_manager import SubAgentManager
from my_tui.agent.subagent_runtime import (
    MAX_AGENT_TURNS, AgentContext, _build_subagent_tools,
)
from my_tui.agent.model_router import ModelRouter, ModelRouterConfig, DEFAULT_SUB_MODEL
from my_tui.agent.skill_system import SkillRegistry, Skill
from my_tui.agent.subagent_roles import RoleType, ROLE_REGISTRY
from my_tui.tools.tools import get_tools_by_names, BUILTIN_TOOLS
from my_tui.security.approval import ToolApprovalSystem, PermissionMode, Decision
from my_tui.provider.provider import (
    ProviderType, ProviderConfig, create_llm, parse_provider,
    get_provider_display, get_available_providers, PROVIDER_DISPLAY_NAMES,
)
from my_tui.tasks.durable_task_manager import DurableTaskManager, TaskDef, TaskType
from my_tui.tui import TUIState, TUIBridge, TuiApp

SETTINGS_FILE = Path("setting.json")


async def run_tui():
    settings = _load_settings()
    if settings:
        provider_config = _build_config_from_settings(settings)
    else:
        provider_config = await _interactive_setup()

    llm = create_llm(provider_config)

    sub_model = settings.get("sub_model", DEFAULT_SUB_MODEL) if settings else DEFAULT_SUB_MODEL
    role_models = settings.get("role_models", {}) if settings else {}
    router_config = ModelRouterConfig(sub_model=sub_model, role_models=role_models)
    model_router = ModelRouter(provider_config, router_config)

    event_bus = EventBus()
    state = TUIState()
    state.connected = True
    state.provider_name = get_provider_display(provider_config)
    state.model_name = provider_config.model

    conversations = ConversationManager(
        db_path="conversations.db",
        max_tokens=128000,
        compression_config=CompactionConfig(
            context_window=128000,
            compaction_threshold_ratio=0.7,
            micro_compact_tool_count=10,
            micro_compact_min_tokens=5000,
            hot_tail_size=3,
        ),
    )
    await conversations.create_session(model=provider_config.model, title="TUI 会话")

    manager = SubAgentManager(max_concurrent=10, max_spawn_depth=3, state_file="subagents.json")
    skill_registry = SkillRegistry()
    _init_skills(skill_registry)

    token_tracker = TokenTracker(model=provider_config.model)
    approval_system = ToolApprovalSystem()
    approval_system.auto_configure()
    approval_system.set_mode(PermissionMode.DEFAULT)

    task_manager = DurableTaskManager(db_path="tasks.db", max_workers=4)
    task_manager.start()

    bridge = TUIBridge(event_bus, state)
    bridge.start()

    cancel_token = CancellationToken()
    user_input_queue: asyncio.Queue[str] = asyncio.Queue()

    processing_task = asyncio.create_task(
        _process_input_loop(
            user_input_queue=user_input_queue,
            llm=llm,
            event_bus=event_bus,
            state=state,
            conversations=conversations,
            skill_registry=skill_registry,
            provider_config=provider_config,
            manager=manager,
            approval_system=approval_system,
            cancel_token=cancel_token,
            token_tracker=token_tracker,
            bridge=bridge,
            model_router=model_router,
        )
    )

    app = TuiApp(event_bus, state, bridge, user_input_queue=user_input_queue)
    await app.run_async()

    cancel_token.cancel()
    processing_task.cancel()
    bridge.stop()
    await task_manager.stop()
    state.connected = False


async def _process_input_loop(
    user_input_queue: asyncio.Queue[str],
    llm: ChatOpenAI,
    event_bus: EventBus,
    state: TUIState,
    conversations: ConversationManager,
    skill_registry: SkillRegistry,
    provider_config: ProviderConfig,
    manager: SubAgentManager,
    approval_system: ToolApprovalSystem,
    cancel_token: CancellationToken,
    token_tracker: TokenTracker,
    bridge: TUIBridge,
    model_router: ModelRouter,
):
    while True:
        try:
            text = await asyncio.wait_for(user_input_queue.get(), timeout=1.0)
        except asyncio.TimeoutError:
            continue
        except asyncio.CancelledError:
            break

        # 命令处理：以 / 开头且非纯空格
        if text.strip().startswith("/"):
            await _handle_cli_command(
                text, event_bus, conversations, manager,
                skill_registry, provider_config, state,
                token_tracker, task_manager, bridge,
                model_router,
            )
            continue

        try:
            await _do_chat(
                message=text,
                llm=llm,
                event_bus=event_bus,
                conversations=conversations,
                skill_registry=skill_registry,
                provider_config=provider_config,
                manager=manager,
                approval_system=approval_system,
                cancel_token=cancel_token,
                token_tracker=token_tracker,
                bridge=bridge,
                model_router=model_router,
            )
        except asyncio.CancelledError:
            break
        except Exception as e:
            event_bus.publish_nowait(
                "system", EventType.ERROR,
                f"处理失败: {type(e).__name__}: {e}",
                agent_name="system",
            )
            event_bus.publish_nowait(
                "system", EventType.COMPLETED,
                "completed",
                agent_name="system",
            )


async def _handle_cli_command(
    text: str,
    event_bus: EventBus,
    conversations: ConversationManager,
    manager: SubAgentManager,
    skill_registry: SkillRegistry,
    provider_config: ProviderConfig,
    state: TUIState,
    token_tracker: TokenTracker,
    task_manager: DurableTaskManager,
    bridge: TUIBridge,
    model_router: ModelRouter,
):
    cmd = text.strip()
    base = cmd.split()[0].lower()
    rest = cmd[len(base):].strip()

    if base == "/sessions":
        sessions = conversations.list_sessions()
        if not sessions:
            event_bus.publish_nowait("system", EventType.MESSAGE, "暂无会话")
            return
        lines = ["会话列表:"]
        for s in sessions:
            lines.append(f"  {s.session_id} — {s.title} ({s.message_count} 条消息)")
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/new":
        title = rest or "新会话"
        await conversations.create_session(model=provider_config.model, title=title)
        event_bus.publish_nowait("system", EventType.MESSAGE, f"已创建新会话: {title}")

    elif base == "/skills":
        skills = skill_registry.list_all()
        if not skills:
            event_bus.publish_nowait("system", EventType.MESSAGE, "暂无已注册技能")
            return
        lines = ["已注册技能:"] + [f"  {s.name} — {s.description}" for s in skills]
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/roles":
        lines = ["可用角色:"] + [
            f"  {r.value}" for r in RoleType
        ]
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/cost":
        u = token_tracker.summary()
        lines = [
            f"Token 输入: {u['total_input_tokens']}",
            f"Token 输出: {u['total_output_tokens']}",
            f"总成本: ${u['total_cost']:.6f}",
        ]
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/list":
        subs = list(manager.list_active())
        if not subs:
            event_bus.publish_nowait("system", EventType.MESSAGE, "无活跃子 Agent")
            return
        lines = ["子 Agent:"]
        for s in subs:
            lines.append(f"  {s.agent_id} — {s.name} ({s.status})")
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/task_list":
        tasks = task_manager.list_tasks() if hasattr(task_manager, 'list_tasks') else []
        if not tasks:
            event_bus.publish_nowait("system", EventType.MESSAGE, "无后台任务")
            return
        lines = ["后台任务:"] + [f"  {t.task_id} — {t.name} ({t.status})" for t in tasks]
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/provider":
        event_bus.publish_nowait("system", EventType.MESSAGE, f"当前 Provider: {provider_config.provider_type.value}")

    elif base == "/model":
        event_bus.publish_nowait("system", EventType.MESSAGE, f"当前模型: {provider_config.model}")

    elif base == "/sub_model":
        if rest:
            model_router.router_config.sub_model = rest
            _save_settings(provider_config, sub_model=rest)
            event_bus.publish_nowait("system", EventType.MESSAGE, f"子 Agent 默认模型已切换为: {rest}")
        else:
            event_bus.publish_nowait("system", EventType.MESSAGE,
                                     f"当前子 Agent 默认模型: {model_router.router_config.sub_model}")

    elif base == "/export":
        event_bus.publish_nowait("system", EventType.MESSAGE, "导出功能仅在 CLI 终端中可用")

    elif base in ("/search", "/session", "/session_rename", "/session_delete",
                  "/spawn", "/collect", "/cancel", "/eval",
                  "/task", "/task_cancel", "/task_pause", "/task_resume", "/task_recover"):
        event_bus.publish_nowait("system", EventType.MESSAGE, f"命令 {base} 当前仅在 CLI 终端中可用")

    elif base == "/resume":
        event_bus.publish_nowait("system", EventType.MESSAGE, "恢复功能仅在 CLI 终端中可用")

    elif base == "/fork":
        event_bus.publish_nowait("system", EventType.MESSAGE, "分叉功能仅在 CLI 终端中可用")

    elif base == "/memory":
        from my_tui.memory import MemoryManager
        mm = MemoryManager()
        if not rest:
            keys = mm.list_keys()
            if not keys:
                event_bus.publish_nowait("system", EventType.MESSAGE, "暂无跨会话记忆")
            else:
                lines = ["记忆列表:"] + [f"  - {k}" for k in keys]
                event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))
        else:
            parts = rest.split(maxsplit=1)
            key = parts[0]
            if len(parts) == 2:
                mm.set(key, parts[1])
                event_bus.publish_nowait("system", EventType.MESSAGE, f"已记住: {key}")
            else:
                content = mm.get(key)
                if content is None:
                    event_bus.publish_nowait("system", EventType.MESSAGE, f"记忆不存在: {key}")
                else:
                    event_bus.publish_nowait("system", EventType.MESSAGE, f"[{key}]\n{content}")

    else:
        event_bus.publish_nowait("system", EventType.MESSAGE, f"未知命令: {cmd}，输入 /help 查看帮助")


async def _do_chat(
    message: str,
    llm: ChatOpenAI,
    event_bus: EventBus,
    conversations: ConversationManager,
    skill_registry: SkillRegistry,
    provider_config: ProviderConfig,
    manager: SubAgentManager,
    approval_system: ToolApprovalSystem,
    cancel_token: CancellationToken,
    token_tracker: TokenTracker,
    bridge: TUIBridge,
    model_router: ModelRouter,
):
    tools = get_tools_by_names(
        ROLE_REGISTRY[RoleType.GENERAL].allowed_tools,
    )

    ctx = AgentContext(
        agent_id="main_tui",
        agent_name="main",
        role_type=RoleType.GENERAL,
        role_def=ROLE_REGISTRY[RoleType.GENERAL],
        cancel_token=cancel_token,
        message_queue=asyncio.Queue(),
        event_bus=event_bus,
        llm=llm,
        tools=tools,
        skill_registry=skill_registry,
        subagent_manager=manager,
        conversation_manager=conversations,
        approval_system=approval_system,
        model_router=model_router,
        depth=0,
    )
    subagent_tools = _build_subagent_tools(ctx)
    all_tools = tools + subagent_tools

    skills = [f"{s.name} — {s.description}" for s in skill_registry.list_all()]
    system_prompt = prompt_engine.render_main_system(
        "general", skills=skills, cwd=str(Path.cwd()),
        model=provider_config.model,
        provider=PROVIDER_DISPLAY_NAMES.get(
            provider_config.provider_type,
            provider_config.provider_type.value,
        ),
    )

    user_msg = HumanMessage(content=message)
    await conversations.add_message(user_msg)
    await conversations.compress_context()

    llm_with_tools = llm.bind_tools(all_tools)

    event_bus.publish_nowait(
        "system", EventType.COMPLETED, "started", agent_name="system",
    )

    for turn in range(MAX_AGENT_TURNS):
        if cancel_token.is_cancelled():
            event_bus.publish_nowait(
                "system", EventType.MESSAGE, "任务被取消", agent_name="system",
            )
            event_bus.publish_nowait(
                "system", EventType.COMPLETED, "cancelled", agent_name="system",
            )
            return

        messages = [
            SystemMessage(content=system_prompt),
            *conversations.get_messages(),
        ]

        collected_chunks = []

        try:
            async for chunk in llm_with_tools.astream(messages):
                if cancel_token.is_cancelled():
                    break
                collected_chunks.append(chunk)
                if chunk.content:
                    event_bus.publish_nowait(
                        "llm", EventType.LLM_STREAM,
                        chunk.content,
                        agent_name="assistant",
                    )
        except Exception as e:
            error_msg = f"LLM 调用失败: {type(e).__name__}: {e}"
            event_bus.publish_nowait(
                "system", EventType.ERROR, error_msg, agent_name="system",
            )
            event_bus.publish_nowait(
                "system", EventType.COMPLETED, "completed", agent_name="system",
            )
            return

        if not collected_chunks:
            event_bus.publish_nowait(
                "system", EventType.ERROR, "未获取到响应", agent_name="system",
            )
            event_bus.publish_nowait(
                "system", EventType.COMPLETED, "completed", agent_name="system",
            )
            return

        response = collected_chunks[0]
        for c in collected_chunks[1:]:
            response += c

        input_text = "\n".join(m.content or "" for m in messages)
        output_text = str(response.content) if response.content else ""
        token_tracker.record_turn(input_text, output_text)

        if not response.tool_calls:
            content = str(response.content) if response.content else "(无内容)"
            event_bus.publish_nowait(
                "llm", EventType.LLM_RESPONSE,
                content,
                agent_name="assistant",
            )
            await conversations.add_message(response)
            event_bus.publish_nowait(
                "system", EventType.COMPLETED, "completed", agent_name="system",
            )
            return

        await conversations.add_message(response)

        tool_name_map = {t.name: t for t in all_tools}
        for tc in response.tool_calls:
            if cancel_token.is_cancelled():
                event_bus.publish_nowait(
                    "system", EventType.MESSAGE, "工具执行被取消", agent_name="system",
                )
                event_bus.publish_nowait(
                    "system", EventType.COMPLETED, "cancelled", agent_name="system",
                )
                return

            tc_name = tc.get("name", tc.get("function", {}).get("name", "unknown"))
            tc_args = tc.get("args", tc.get("function", {}).get("arguments", {}))
            if isinstance(tc_args, str):
                try:
                    tc_args = json.loads(tc_args)
                except json.JSONDecodeError:
                    tc_args = {}
            tc_id = tc.get("id", "")

            event_bus.publish_nowait(
                "tool", EventType.TOOL_CALL,
                json.dumps({"tool_name": tc_name, "args": tc_args, "tool_call_id": tc_id}),
                agent_name="assistant",
            )

            if approval_system is not None:
                approval_result = await approval_system.evaluate_tool_call(
                    name=tc_name,
                    arguments=tc_args,
                    command=str(tc_args.get("command", tc_args.get("filepath", ""))),
                    target_path=str(tc_args.get("filepath", tc_args.get("directory", ""))),
                )
                if approval_result.decision == Decision.BLOCK:
                    msg = f"工具 '{tc_name}' 被审批系统拒绝: {approval_result.message}"
                    event_bus.publish_nowait(
                        "tool", EventType.ERROR, msg, agent_name="system",
                    )
                    tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                    await conversations.add_message(tool_msg)
                    continue
                elif approval_result.decision == Decision.ASK:
                    event_bus.publish_nowait(
                        "tool", EventType.MESSAGE,
                        json.dumps({
                            "type": "approval_request",
                            "tool_name": tc_name,
                            "args": tc_args,
                            "message": approval_result.message,
                            "tool_call_id": tc_id,
                        }),
                        agent_name="system",
                    )
                    confirm = await _request_approval_dialog(
                        event_bus=event_bus,
                        tool_name=tc_name,
                        tc_args=tc_args,
                        detail_msg=approval_result.message,
                        bridge=bridge,
                    )
                    if not confirm:
                        msg = f"用户拒绝工具 '{tc_name}'"
                        event_bus.publish_nowait(
                            "tool", EventType.MESSAGE, msg, agent_name="system",
                        )
                        tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                        await conversations.add_message(tool_msg)
                        continue

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

            result_str = str(result)
            tool_result_payload = json.dumps({
                "tool_name": tc_name,
                "result": result_str[:2000],
                "tool_call_id": tc_id,
            })
            event_bus.publish_nowait(
                "tool", EventType.TOOL_RESULT,
                tool_result_payload,
                agent_name="assistant",
            )

            tool_msg = ToolMessage(content=result_str, tool_call_id=tc_id)
            await conversations.add_message(tool_msg)

    event_bus.publish_nowait(
        "system", EventType.MESSAGE,
        f"达到最大轮次 ({MAX_AGENT_TURNS})",
        agent_name="system",
    )
    event_bus.publish_nowait(
        "system", EventType.COMPLETED, "completed", agent_name="system",
    )


async def _request_approval_dialog(
    event_bus: EventBus,
    tool_name: str,
    tc_args: dict,
    detail_msg: str,
    timeout: float = 30.0,
    bridge: TUIBridge | None = None,
) -> bool:
    if bridge is None:
        return True

    try:
        future = bridge.start_approval(
            tool_name=tool_name,
            args=tc_args,
            message=detail_msg,
        )
        result = await asyncio.wait_for(future, timeout=timeout)
        return result
    except asyncio.TimeoutError:
        return False


def _load_settings() -> dict | None:
    try:
        if SETTINGS_FILE.exists():
            data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
            if data.get("api_key"):
                return data
    except (json.JSONDecodeError, OSError):
        pass
    return None


def _build_config_from_settings(settings: dict) -> ProviderConfig:
    provider_str = settings.get("provider", "openai_compatible")
    provider_type = parse_provider(provider_str) or ProviderType.OPENAI_COMPATIBLE
    return ProviderConfig(
        provider_type=provider_type,
        base_url=settings.get("base_url", ""),
        api_key=settings["api_key"],
        model=settings.get("model", ""),
    )


async def _interactive_setup() -> ProviderConfig:
    providers = get_available_providers()
    print("\n  选择 LLM Provider:")
    for i, p in enumerate(providers, 1):
        print(f"    {i}. {p['display']} (默认模型: {p['default_model']})")

    choice = input(f"\n  请选择 (1-{len(providers)}, 默认 1): ").strip() or "1"
    try:
        idx = int(choice) - 1
        selected = providers[idx] if 0 <= idx < len(providers) else providers[0]
        provider_type = ProviderType(selected["key"])
    except (ValueError, IndexError):
        provider_type = ProviderType.OPENAI_COMPATIBLE
        selected = providers[0]

    base_url = input(f"  Base URL (默认 {selected['default_base_url']}): ").strip()
    if not base_url:
        base_url = selected["default_base_url"]

    api_key = input("  API Key: ").strip()
    while not api_key:
        api_key = input("  API Key: ").strip()

    model = input(f"  模型名称 (默认 {selected['default_model']}): ").strip()
    if not model:
        model = selected["default_model"]

    config = ProviderConfig(
        provider_type=provider_type,
        base_url=base_url,
        api_key=api_key,
        model=model,
    )

    _save_settings(config)
    return config


def _save_settings(config: ProviderConfig, sub_model: str | None = None,
                   role_models: dict[str, str] | None = None):
    data: dict = {
        "provider": config.provider_type.value,
        "base_url": config.base_url,
        "api_key": config.api_key,
        "model": config.model,
    }
    if sub_model is not None:
        data["sub_model"] = sub_model
    if role_models is not None:
        data["role_models"] = role_models
    try:
        SETTINGS_FILE.write_text(
            json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8"
        )
    except OSError:
        pass


def _init_skills(registry: SkillRegistry):
    skills_dir = Path("skills")
    loaded = registry.load_skills_from_directory(skills_dir)
    if loaded:
        return
    bt = BUILTIN_TOOLS
    for name, desc, tools, role in [
        ("code_explorer", "代码库探索与分析", ["list_files", "read_file", "search_code"], "explore"),
        ("code_writer", "代码编写与修改", ["read_file", "write_file", "search_code", "execute_command"], "implementer"),
        ("code_reviewer", "代码审查", ["read_file", "search_code", "execute_command"], "review"),
        ("test_runner", "测试执行与验证", ["read_file", "execute_command", "search_code"], "verifier"),
        ("task_planner", "任务分解与规划", ["list_files", "read_file", "write_file"], "plan"),
    ]:
        tool_objs = [bt[t] for t in tools if t in bt]
        registry.register(Skill(name=name, description=desc, tools=tool_objs, metadata={"role": role}))


def main():
    if sys.platform == "win32":
        asyncio.set_event_loop_policy(asyncio.WindowsSelectorEventLoopPolicy())

    try:
        asyncio.run(run_tui())
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()