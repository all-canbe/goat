#!/usr/bin/env python3
"""
[DEPRECATED] TUI 入口 — 不再维护。

TUI 方式已废弃，请使用以下方式启动 Goat：

    CLI:     python main.py
    Web:     python main.py web

本文件保留仅作参考，入口函数 main() 和 goat_tui 仍可用但不再更新。
"""

# # ── 原始文档（保留供参考）──
# """
# Goat TUI 入口 - 山羊主题的终端 AI 编程助手
#
# 用法:
#     # 运行 TUI
#     goat_tui
#     # 或    python tui_main.py [--provider openai_compatible] [--model gpt-4o]
#
# 环境变量:
#     API_KEY, BASE_URL, MODEL
# """

from __future__ import annotations

import asyncio
import json
import logging
import os
import sys
import traceback
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")
sys.stdin.reconfigure(encoding="utf-8")

try:
    from dotenv import load_dotenv
    load_dotenv()
except ImportError:
    pass

from langchain_core.messages import (
    SystemMessage, HumanMessage, ToolMessage,
)
from langchain_openai import ChatOpenAI

from goat.core.event_bus import EventBus, EventType
from goat.core.cancellation import CancellationToken
from goat.core.token_tracker import TokenTracker
from goat.core.workspace import get_goat_home, get_skills_dir, resolve_workspace
from goat.conversation.conversation_manager import ConversationManager
from goat.conversation.context_compression import CompactionConfig
from goat.conversation.prompt_engine import engine as prompt_engine
from goat.agent.subagent_manager import SubAgentManager
from goat.agent.subagent_runtime import (
    MAX_AGENT_TURNS, AgentContext, _build_subagent_tools,
    looks_like_final_answer, continuation_prompt,
)
from goat.agent.model_router import ModelRouter, ModelRouterConfig, DEFAULT_SUB_MODEL
from goat.agent.skill_system import SkillRegistry, Skill
from goat.agent.subagent_roles import RoleType, ROLE_REGISTRY
from goat.agent.pipeline import (
    FlowPipeline, is_complex_task,
    has_mutation_tools, get_git_diff, run_mid_flow_review, _format_findings_text,
)
from goat.tools.tools import get_tools_by_names, BUILTIN_TOOLS
from goat.tools.async_executor import execute_command_async
from goat.hooks.lifecycle import HookLifecycleSystem, HookDecision
from goat.security.approval import ToolApprovalSystem, PermissionMode, Decision
from goat.provider.provider import (
    ProviderType, ProviderConfig, create_llm, parse_provider,
    get_provider_display, get_available_providers, PROVIDER_DISPLAY_NAMES,
)
from goat.tasks.durable_task_manager import DurableTaskManager, TaskDef, TaskType
from tui_legacy.tui import TUIState, TUIBridge, TuiApp

logger = logging.getLogger(__name__)

GOAT_HOME = get_goat_home()
SETTINGS_FILE = GOAT_HOME / "setting.json"


async def run_tui(workspace: str | None = None):
    workspace_path = resolve_workspace(workspace)
    settings = _load_settings()
    if settings:
        provider_config = _build_config_from_settings(settings)
    else:
        provider_config = await _interactive_setup()

    llm = create_llm(provider_config)

    sub_model = settings.get("sub_model") or provider_config.model if settings else provider_config.model
    review_model = settings.get("review_model", "") if settings else ""
    review_provider = None
    review_llm = llm  # default: same as main model
    if settings and settings.get("review_api_key"):
        try:
            review_provider = ProviderConfig(
                provider_type=parse_provider(settings.get("review_provider", "")) or provider_config.provider_type,
                base_url=settings.get("review_base_url", provider_config.base_url),
                api_key=settings["review_api_key"],
                model=review_model or provider_config.model,
            )
        except Exception:
            review_provider = None
    if review_provider:
        review_llm = create_llm(review_provider)
    elif review_model and settings:
        review_llm = create_llm(ProviderConfig(
            provider_type=provider_config.provider_type,
            base_url=provider_config.base_url,
            api_key=provider_config.api_key,
            model=review_model,
        ))
    router_config = ModelRouterConfig(
        main_model=provider_config.model,
        sub_model=sub_model,
        review_model=review_model,
        review_provider=review_provider,
    )
    model_router = ModelRouter(provider_config, router_config)

    event_bus = EventBus()
    from goat.tools.retry import init_retry
    init_retry(event_bus)
    state = TUIState()
    state.connected = True
    state.provider_name = get_provider_display(provider_config)
    state.model_name = provider_config.model

    conversations = ConversationManager(
        db_path=str(GOAT_HOME / "conversations.db"),
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

    manager = SubAgentManager(max_concurrent=10, max_spawn_depth=3, state_file=str(GOAT_HOME / "subagents.json"))
    skill_registry = SkillRegistry()
    _init_skills(skill_registry)

    cancel_token = CancellationToken()

    approval_system = ToolApprovalSystem()
    approval_system.auto_configure()
    approval_system.set_mode(PermissionMode.DEFAULT)

    hook_system = HookLifecycleSystem()
    if settings and "hooks" in settings:
        try:
            from goat.hooks.lifecycle import HookConfigLoader
            raw = json.dumps({"hooks": settings["hooks"]}, ensure_ascii=False)
            hooks = HookConfigLoader.from_json(raw)
            for h in hooks:
                hook_system.register(h)
        except Exception:
            pass

    pipeline = FlowPipeline(
        impl_llm=llm,
        review_llm=review_llm,
        manager=manager,
        skill_registry=skill_registry,
        event_bus=event_bus,
        cancel_token=cancel_token,
        approval_system=approval_system,
        hook_system=hook_system,
    )

    token_tracker = TokenTracker(model=provider_config.model)

    task_manager = DurableTaskManager(db_path=str(GOAT_HOME / "tasks.db"), max_workers=4)
    task_manager.start()

    bridge = TUIBridge(event_bus, state, approval_system=approval_system)
    bridge.start()

    _mcp_connect_task = asyncio.create_task(_init_mcp_connections(event_bus, settings))

    user_input_queue: asyncio.Queue[str] = asyncio.Queue()

    session_injected_context = ""
    try:
        session_results = await hook_system.on_session_start()
        if session_results:
            parts = []
            for r in session_results:
                if isinstance(r, dict) and r.get("injected_prompt"):
                    parts.append(r["injected_prompt"])
            if parts:
                session_injected_context = "\n".join(parts)
    except Exception:
        pass

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
            pipeline=pipeline,
            approval_system=approval_system,
            cancel_token=cancel_token,
            token_tracker=token_tracker,
            bridge=bridge,
            model_router=model_router,
            hook_system=hook_system,
            workspace=workspace_path,
            session_injected_context=session_injected_context,
        )
    )

    app = TuiApp(event_bus, state, bridge, user_input_queue=user_input_queue)
    await app.run_async()

    cancel_token.cancel()
    processing_task.cancel()
    bridge.stop()
    await hook_system.on_session_end("cleanup")
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
    pipeline: FlowPipeline,
    approval_system: ToolApprovalSystem,
    cancel_token: CancellationToken,
    token_tracker: TokenTracker,
    bridge: TUIBridge,
    model_router: ModelRouter,
    hook_system: HookLifecycleSystem | None = None,
    workspace: Path | None = None,
    session_injected_context: str = "",
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
            # /skills find<描述> ?/skills find <描述> ?构?Prompt 注入正常 LLM 消息?            find_query: str | None = None
            parts = text.strip().split(maxsplit=1)
            if len(parts) >= 2 and parts[0] == "/skills":
                after_skills = parts[1].strip()
                if after_skills.lower().startswith("find"):
                    find_query = after_skills[4:].lstrip()
            if find_query:
                text = (
                    f"请使?find-skills skill 搜索与「{find_query}」相关的可用 skill。\n"
                    f"如果 find-skills skill 不可用，请通过 WebSearch 搜索 npx skills 仓库。\n"
                    f"请清晰地列出找到的每?skill 的：名称、描述、安?URL。\n"
                    f"搜索完毕后我会告知你安装选项。"
                )
                try:
                    if state.mode == PermissionMode.FLOW and is_complex_task(text):
                        await _check_tui_review_model(event_bus, review_llm, llm, settings)
                        await _do_flow_chat(
                            message=text,
                            pipeline=pipeline,
                            event_bus=event_bus,
                            conversations=conversations,
                            token_tracker=token_tracker,
                            cancel_token=cancel_token,
                        )
                    else:
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
                            hook_system=hook_system,
                            workspace=workspace,
                            session_injected_context=session_injected_context,
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

                # ── 交互式安装选择 ──
                event_bus.publish_nowait("system", EventType.MESSAGE,
                    "\n📋 以上是搜索结果。输?skill URL 进行安装（从上方列表复制），或输?0 取消:")
                try:
                    choice = await asyncio.wait_for(user_input_queue.get(), timeout=120.0)
                except asyncio.TimeoutError:
                    choice = "0"
                choice = choice.strip()
                if choice and choice != "0":
                    event_bus.publish_nowait("system", EventType.MESSAGE,
                        "安装? (1) 本项? (2) 全局? [默认 1]")
                    try:
                        scope = await asyncio.wait_for(user_input_queue.get(), timeout=30.0)
                    except asyncio.TimeoutError:
                        scope = "1"
                    install_cmd = f"/skills install {choice}"
                    if scope.strip() == "2":
                        install_cmd += " --global"
                    await _handle_cli_command(
                        install_cmd, event_bus, conversations, manager,
                        skill_registry, provider_config, state,
                        token_tracker, task_manager, bridge,
                        model_router, llm, cancel_token,
                        approval_system, hook_system,
                    )
                else:
                    event_bus.publish_nowait("system", EventType.MESSAGE, "已取消安装")
                continue
            else:
                new_llm = await _handle_cli_command(
                    text, event_bus, conversations, manager,
                    skill_registry, provider_config, state,
                    token_tracker, task_manager, bridge,
                    model_router, llm, cancel_token,
                    approval_system, hook_system,
                )
                if new_llm:
                    llm = new_llm
                continue

        try:
            if state.mode == PermissionMode.FLOW and is_complex_task(text):
                await _check_tui_review_model(event_bus, review_llm, llm, settings)
                await _do_flow_chat(
                    message=text,
                    pipeline=pipeline,
                    event_bus=event_bus,
                    conversations=conversations,
                    token_tracker=token_tracker,
                    cancel_token=cancel_token,
                )
            else:
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
                    hook_system=hook_system,
                    workspace=workspace,
                    session_injected_context=session_injected_context,
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
    llm: ChatOpenAI | None = None,
    cancel_token: CancellationToken | None = None,
    approval_system: ToolApprovalSystem | None = None,
    hook_system: HookLifecycleSystem | None = None,
) -> ChatOpenAI | None:
    cmd = text.strip()
    base = cmd.split()[0].lower()
    rest = cmd[len(base):].strip()

    if base == "/sessions":
        sessions = conversations.list_sessions()
        if not sessions:
            event_bus.publish_nowait("system", EventType.MESSAGE, "暂无会话")
            return
        current = conversations.current_session_id
        lines = ["会话列表:"]
        for s in sessions:
            marker = " ◀" if s.session_id == current else ""
            lines.append(f"  {s.session_id[:8]} | {s.title} ({s.message_count} 条消?{marker}")
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/new":
        title = rest or "新会话"
        await conversations.create_session(model=provider_config.model, title=title)
        event_bus.publish_nowait("system", EventType.MESSAGE, f"已创建新会话: {title}")

    elif base == "/skills":
        sub = rest.split(maxsplit=1)
        subcmd = sub[0].lower() if sub else ""
        sub_rest = sub[1] if len(sub) > 1 else ""

        # 兼容 find<描述> 无空格输入
        if subcmd.startswith("find"):
            query = subcmd[4:].strip() or sub_rest
            if not query:
                event_bus.publish_nowait("system", EventType.MESSAGE,
                    "用法: /skills find <描述>\n"
                    "搜索可用 skill 并展示结果供你选择安装")
                return
            # /skills find 已在 _process_input_loop 中被拦截并转?Agent Prompt
            event_bus.publish_nowait("system", EventType.MESSAGE,
                f"🔍 正在搜索 skill: {query}，请等待 Agent 响应...")

        elif subcmd == "install":
            install_parts = sub_rest.split()
            if not install_parts:
                event_bus.publish_nowait("system", EventType.MESSAGE,
                    "用法: /skills install <repo_url> [--skill <name>] [--global]\n"
                    "示例: /skills install https://github.com/vercel-labs/skills --skill find-skills\n"
                    "      /skills install https://github.com/vercel-labs/skills --skill find-skills --global")
                return
            use_global = "--global" in install_parts
            if use_global:
                install_parts = [p for p in install_parts if p != "--global"]
            import shutil
            npx = shutil.which("npx")
            if not npx:
                event_bus.publish_nowait("system", EventType.MESSAGE,
                    "?未找?npx，请先安?Node.js (https://nodejs.org)")
                return
            cmd = f"npx skills add {' '.join(install_parts)}"
            event_bus.publish_nowait("system", EventType.MESSAGE, f"📦 执行: {cmd}")
            try:
                proc = await asyncio.create_subprocess_shell(
                    cmd,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                )
                stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=120)
                out = stdout.decode("utf-8", errors="replace").strip()
                err = stderr.decode("utf-8", errors="replace").strip()
                if proc.returncode == 0:
                    skills_dir = GOAT_HOME / "skills" if use_global else get_skills_dir()
                    skills_dir.mkdir(parents=True, exist_ok=True)
                    skill_registry.load_skills_from_directory(skills_dir)
                    loaded_names = [s.name for s in skill_registry.list_all()]
                    scope = "全局" if use_global else "本项目"
                    event_bus.publish_nowait("system", EventType.MESSAGE,
                        f"✅安装成功 ({scope})\n{out}\n当前技能 {', '.join(loaded_names) if loaded_names else '无'}")
                else:
                    event_bus.publish_nowait("system", EventType.MESSAGE,
                        f"?安装失败 (exit {proc.returncode})\n{err or out}")
            except asyncio.TimeoutError:
                try:
                    proc.kill()
                except ProcessLookupError:
                    pass
                event_bus.publish_nowait("system", EventType.MESSAGE, "?安装超时 (120s)")
            except Exception as e:
                try:
                    proc.kill()
                except (ProcessLookupError, UnboundLocalError):
                    pass
                event_bus.publish_nowait("system", EventType.MESSAGE, f"?安装出错: {e}")

        elif subcmd == "list" or not subcmd:
            skills = skill_registry.list_all()
            if not skills:
                event_bus.publish_nowait("system", EventType.MESSAGE,
                    "暂无已注册技能\n\n安装技?\n"
                    "  /skills install <repo_url> [--skill <name>] [--global]\n"
                    "搜索技?\n"
                    "  /skills find <描述>")
                return
            lines = ["已注册技?"]
            for s in skills:
                detail = s.description or "无描述"
                if s.metadata.get("has_agents"):
                    agent_list = ", ".join(s.agents.keys())
                    detail += f" (子角? {agent_list})"
                lines.append(f"  📦 {s.name} ?{detail}")
            lines.append("\n/skills find <描述> — 搜索新技能")
            lines.append("/skills install <url> [--skill <名称>] [--global] — 安装技能")
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
            f"总成? ${u['total_cost']:.6f}",
        ]
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/list":
        subs = list(manager.list_active())
        if not subs:
            event_bus.publish_nowait("system", EventType.MESSAGE, "无活跃子 Agent")
            return
        lines = ["?Agent:"]
        for s in subs:
            lines.append(f"  {s.agent_id} ?{s.name} ({s.status})")
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/task_list":
        tasks = task_manager.list_tasks() if hasattr(task_manager, 'list_tasks') else []
        if not tasks:
            event_bus.publish_nowait("system", EventType.MESSAGE, "无后台任务")
            return
        lines = ["后台任务:"] + [f"  {t.task_id} ?{t.name} ({t.status})" for t in tasks]
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/provider":
        lines = [
            f"当前 Provider: {get_provider_display(provider_config)}",
            f"Base URL: {provider_config.base_url}",
            f"模型: {provider_config.model}",
            "",
            "可用 Provider:",
        ]
        for p in get_available_providers():
            lines.append(f"  {p['key']:20s} ?{p['display']}")
        lines.append("")
        lines.append("切换 Provider 请使? /model add <名称> <url> <key>")
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/model":
        parts = rest.split(None, 2) if rest else []
        sub_cmd = parts[0] if parts else ""

        if not sub_cmd:
            rv_config = model_router.router_config.review_provider
            config_lines = [
                f"📋 当前模型配置:\n",
                f"  ?Agent:  {provider_config.model}",
                f"  ?Agent:  {model_router.router_config.sub_model}",
            ]
            rv_model = model_router.router_config.review_model or provider_config.model
            rv_info = f"{rv_model} (独立 Provider)" if rv_config else rv_model
            config_lines.append(f"  审查模型:  {rv_info}")
            config_lines.append(f"\n用法:")
            config_lines.append(f"  /model main <模型?     切换?Agent 模型")
            config_lines.append(f"  /model sub <模型?      切换?Agent 模型")
            config_lines.append(f"  /model review <模型?   切换审查模型")
            config_lines.append(f"  /model add <? <url> <key>  新增自定?Provider")
            event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(config_lines))

        elif sub_cmd == "main" and len(parts) >= 2:
            new_model = parts[1]
            event_bus.publish_nowait("system", EventType.MESSAGE, f"?正在测试模型 {new_model} 连通?..")
            test_config = ProviderConfig(
                provider_type=provider_config.provider_type,
                base_url=provider_config.base_url,
                api_key=provider_config.api_key,
                model=new_model,
            )
            ok, err = await _test_llm_connection(test_config)
            if not ok:
                event_bus.publish_nowait("system", EventType.ERROR, f"?模型 {new_model} 连通性测试失? {err}")
                return
            provider_config.model = new_model
            new_llm = create_llm(provider_config)
            token_tracker.model = new_model
            model_router.router_config.main_model = new_model
            _save_settings(provider_config,
                           sub_model=model_router.router_config.sub_model,
                           review_model=model_router.router_config.review_model,
                           review_config=model_router.router_config.review_provider)
            event_bus.publish_nowait("system", EventType.MESSAGE, f"??Agent 模型已切换为: {new_model}")
            return new_llm

        elif sub_cmd == "sub" and len(parts) >= 2:
            new_sub = parts[1]
            model_router.router_config.sub_model = new_sub
            _save_settings(provider_config,
                           sub_model=new_sub,
                           review_model=model_router.router_config.review_model,
                           review_config=model_router.router_config.review_provider)
            event_bus.publish_nowait("system", EventType.MESSAGE, f"??Agent 模型已切换为: {new_sub}")

        elif sub_cmd == "review" and len(parts) >= 2:
            new_review = parts[1]
            model_router.router_config.review_model = new_review
            model_router.router_config.review_provider = None
            _save_settings(provider_config,
                           sub_model=model_router.router_config.sub_model,
                           review_model=new_review,
                           review_config=None)
            _set_setting_flag("review_model_prompted", True)
            event_bus.publish_nowait("system", EventType.MESSAGE, f"?审查模型已切换为: {new_review}")

        elif sub_cmd == "add" and len(parts) >= 4:
            provider_name = parts[1]
            custom_url = parts[2]
            custom_key = parts[3]
            custom_model = input(f"  输入 {provider_name} 的模型名? ").strip()
            if not custom_model:
                event_bus.publish_nowait("system", EventType.MESSAGE, "⚠️ 模型名称不能为空")
                return
            event_bus.publish_nowait("system", EventType.MESSAGE, f"?正在测试 Provider [{provider_name}] 连通?..")
            test_config = ProviderConfig(
                provider_type=ProviderType.OPENAI_COMPATIBLE,
                base_url=custom_url,
                api_key=custom_key,
                model=custom_model,
            )
            ok, err = await _test_llm_connection(test_config)
            if not ok:
                event_bus.publish_nowait("system", EventType.ERROR, f"?Provider [{provider_name}] 连通性测试失? {err}")
                return
            provider_config.model = custom_model
            provider_config.base_url = custom_url
            provider_config.api_key = custom_key
            new_llm = create_llm(provider_config)
            token_tracker.model = custom_model
            model_router.router_config.main_model = custom_model
            _save_settings(provider_config,
                           sub_model=model_router.router_config.sub_model,
                           review_model=model_router.router_config.review_model,
                           review_config=model_router.router_config.review_provider,
                           custom_name=provider_name)
            event_bus.publish_nowait("system", EventType.MESSAGE,
                                     f"?已新?Provider [{provider_name}] 并切换为?Agent 模型\n"
                                     f"   URL: {custom_url}\n   模型: {custom_model}")
            return new_llm

        else:
            event_bus.publish_nowait("system", EventType.MESSAGE,
                                     "未知子命令，输入 /model 查看用法")

    elif base == "/search":
        if not rest:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /search <关键?")
            return
        results = conversations.search_messages(rest)
        if not results:
            event_bus.publish_nowait("system", EventType.MESSAGE, "未找到匹配消息")
            return
        lines = [f"找到 {len(results)} 条匹配消?"]
        for r in results[:10]:
            preview = r.content[:100].replace("\n", " ")
            lines.append(f"  [{r.session_id[:8]}] {r.role}: {preview}")
        event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))

    elif base == "/session":
        if not rest:
            current = conversations.current_session_id
            info = conversations.get_session_info(current) if current else None
            if info:
                event_bus.publish_nowait("system", EventType.MESSAGE,
                                         f"当前会话: {info.title} ({info.session_id[:8]})")
            else:
                event_bus.publish_nowait("system", EventType.MESSAGE, "无当前会话")
            return
        sid = conversations.resolve_session_id(rest)
        if sid:
            conversations.load_session(sid)
            info = conversations.get_session_info(sid)
            event_bus.publish_nowait("system", EventType.MESSAGE,
                                     f"已切换到会话: {info.title if info else sid[:8]}")
        else:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"未找到会? {rest}（多个匹配或不存在）")

    elif base == "/session_rename":
        parts = rest.split(maxsplit=1)
        if len(parts) < 2:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /session_rename <session_id> <新名?")
            return
        sid = conversations.resolve_session_id(parts[0])
        if not sid:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"未找到会? {parts[0]}（多个匹配或不存在）")
            return
        ok = conversations.rename_session(sid, parts[1])
        if ok:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"会话 {sid[:8]} 已重命名? {parts[1]}")
        else:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"重命名失? {sid[:8]}")

    elif base == "/session_delete":
        if not rest:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /session_delete <session_id>")
            return
        sid = conversations.resolve_session_id(rest)
        if not sid:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"未找到会? {rest}（多个匹配或不存在）")
            return
        ok = conversations.delete_session(sid)
        if ok:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"已删除会?{sid[:8]}")
        else:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"删除失败: {sid[:8]}")

    elif base == "/export":
        export_parts = rest.split(maxsplit=1) if rest else []
        fmt = export_parts[1] if len(export_parts) > 1 else "text"
        raw_sid = export_parts[0] if export_parts else ""
        if not raw_sid:
            sid = conversations.current_session_id
        else:
            sid = conversations.resolve_session_id(raw_sid)
        if not sid:
            event_bus.publish_nowait("system", EventType.MESSAGE, "没有当前会话或会话不存在")
            return
        try:
            result = conversations.export_session(sid, format=fmt)
        except Exception as e:
            event_bus.publish_nowait("system", EventType.ERROR, f"导出失败: {e}")
            return
        if fmt == "json":
            filepath = f"session_{sid[:8]}.json"
            Path(filepath).write_text(result, encoding="utf-8")
            event_bus.publish_nowait("system", EventType.MESSAGE, f"已导出到 {filepath}")
        else:
            if len(result) > 2000:
                event_bus.publish_nowait("system", EventType.MESSAGE, result[:2000] + f"\n... (?{len(result)} 字符)")
            else:
                event_bus.publish_nowait("system", EventType.MESSAGE, result)

    elif base == "/spawn":
        parts = rest.split(maxsplit=1)
        if len(parts) < 2:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /spawn <角色> <任务描述>")
            event_bus.publish_nowait("system", EventType.MESSAGE, "角色: general, explore, plan, implementer, review, verifier")
            return
        role_name = parts[0].lower()
        task_desc = parts[1]
        try:
            role_type = RoleType(role_name)
        except ValueError:
            valid = [r.value for r in RoleType]
            event_bus.publish_nowait("system", EventType.MESSAGE, f"无效的角?'{role_name}'，可? {valid}")
            return
        child_llm = model_router.get_llm_for_role(role_name)
        from goat.agent.subagent_runtime import run_subagent
        child_cancel = CancellationToken()
        agent = await manager.spawn(
            parent_id="main_tui",
            role_type=role_type,
            task_description=task_desc,
            parent_cancel_token=cancel_token,
            spawn_depth=1,
        )
        ctx = AgentContext(
            agent_id=agent.agent_id,
            agent_name=agent.name,
            role_type=role_type,
            role_def=ROLE_REGISTRY[role_type],
            cancel_token=child_cancel,
            message_queue=asyncio.Queue(),
            event_bus=event_bus,
            llm=child_llm,
            tools=get_tools_by_names(ROLE_REGISTRY[role_type].allowed_tools),
            skill_registry=skill_registry,
            conversation_manager=conversations,
            approval_system=approval_system,
            hook_system=hook_system,
            depth=1,
        )
        agent.status = "running"
        agent.task_handle = asyncio.create_task(run_subagent(ctx))
        event_bus.publish_nowait("system", EventType.MESSAGE,
                                 f"已创建子 Agent: {agent.name} [{agent.agent_id}] (角色: {role_name})")

    elif base == "/collect":
        ids = [i.strip() for i in rest.split(",") if i.strip()] if rest else None
        result = await manager.collect_results(ids)
        event_bus.publish_nowait("system", EventType.MESSAGE, str(result))

    elif base == "/cancel":
        if not rest:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /cancel <agent_id>")
            return
        await manager.cancel(rest.strip())
        event_bus.publish_nowait("system", EventType.MESSAGE, f"已取?Agent {rest.strip()}")

    elif base == "/eval":
        parts = rest.split(maxsplit=1)
        if len(parts) < 2:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /eval <agent_id> <消息>")
            return
        result = await manager.send_message(parts[0], parts[1])
        event_bus.publish_nowait("system", EventType.MESSAGE, str(result))

    elif base == "/task":
        if not rest:
            event_bus.publish_nowait("system", EventType.MESSAGE,
                "用法: /task <任务? [描述] [metadata:{...}]\n可用任务类型: explore, batch_run")
            return
        parts = rest.split(maxsplit=1)
        task_name = parts[0].strip()
        rest2 = parts[1] if len(parts) > 1 else ""
        import re
        meta_match = re.search(r'metadata:(\{.*\})', rest2)
        metadata = {}
        if meta_match:
            try:
                metadata = json.loads(meta_match.group(1))
            except json.JSONDecodeError:
                event_bus.publish_nowait("system", EventType.MESSAGE, "metadata JSON 解析失败")
                return
            description = rest2[:meta_match.start()].strip()
        else:
            description = rest2.strip()
        task_id, err = await task_manager.submit(task_name, description=description, metadata=metadata)
        if err:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"提交失败: {err}")
        else:
            event_bus.publish_nowait("system", EventType.MESSAGE,
                f"?任务已提? {task_id}\n   名称: {task_name} | 描述: {description[:60]}")

    elif base == "/task_cancel":
        if not rest:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /task_cancel <task_id>")
            return
        msg = await task_manager.cancel(rest.strip())
        event_bus.publish_nowait("system", EventType.MESSAGE, str(msg))

    elif base == "/task_pause":
        if not rest:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /task_pause <task_id>")
            return
        msg = await task_manager.pause(rest.strip())
        event_bus.publish_nowait("system", EventType.MESSAGE, str(msg))

    elif base == "/task_resume":
        if not rest:
            event_bus.publish_nowait("system", EventType.MESSAGE, "用法: /task_resume <task_id>")
            return
        msg = await task_manager.resume(rest.strip())
        event_bus.publish_nowait("system", EventType.MESSAGE, str(msg))

    elif base == "/task_recover":
        recovered = await task_manager.recover()
        if recovered:
            lines = [f"已恢?{len(recovered)} 个任?"]
            for r in recovered:
                lines.append(f"  {r.task_id} ?{r.name}: {r.description[:50] if hasattr(r, 'description') else ''}")
            event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))
        else:
            event_bus.publish_nowait("system", EventType.MESSAGE, "没有需要恢复的任务")

    elif base == "/resume":
        if not rest:
            checkpoint = conversations.get_last_checkpoint()
            if checkpoint:
                sessions = conversations.list_sessions(limit=5)
                lines = [
                    f"发现上次断点:",
                    f"  会话: {checkpoint['title']}",
                    f"  消息? {checkpoint['message_count']}",
                    f"  Token: {checkpoint['token_count']}",
                    "使用 /resume <session_id> 指定要恢复的会话",
                ]
                if sessions:
                    lines.append("")
                    lines.append("最近的会话:")
                    for s in sessions:
                        lines.append(f"  {s.session_id[:8]} ?{s.title} ({s.message_count} 条消?")
                event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))
            else:
                sessions = conversations.list_sessions(limit=5)
                if sessions:
                    lines = ["没有可恢复的断点，最近的会话:"]
                    for s in sessions:
                        lines.append(f"  {s.session_id[:8]} ?{s.title} ({s.message_count} 条消?")
                    event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))
                else:
                    event_bus.publish_nowait("system", EventType.MESSAGE, "没有会话可恢复")
            return
        sid = conversations.resolve_session_id(rest.strip())
        if not sid:
            sessions = conversations.list_sessions(limit=5)
            lines = [f"未找到会? {rest.strip()}（多个匹配或不存在）"]
            if sessions:
                lines.append("最近的会话:")
                for s in sessions:
                    lines.append(f"  {s.session_id[:8]} ?{s.title} ({s.message_count} 条消?")
            event_bus.publish_nowait("system", EventType.MESSAGE, "\n".join(lines))
            return
        if conversations.load_session(sid):
            info = conversations.get_session_info(sid)
            conversations.clear_checkpoint()
            if info:
                event_bus.publish_nowait("system", EventType.MESSAGE,
                    f"?已恢复会? {info.title} ({info.message_count} 条消? {info.token_count} tokens)")
            else:
                event_bus.publish_nowait("system", EventType.MESSAGE, f"?已恢复会?{sid[:8]}")
        else:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"会话不存? {sid[:8]}")

    elif base == "/fork":
        parts = rest.split(None, 1) if rest else []
        if not parts:
            event_bus.publish_nowait("system", EventType.MESSAGE,
                "用法: /fork <session_id> [turn_number]\n  turn_number: 分叉到指定轮次（1 开始），不指定则复制全会话")
            return
        sid = conversations.resolve_session_id(parts[0].strip())
        if not sid:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"未找到会? {parts[0].strip()}（多个匹配或不存在）")
            return
        turn_number = None
        if len(parts) > 1:
            try:
                turn_number = int(parts[1].strip())
                if turn_number < 1:
                    event_bus.publish_nowait("system", EventType.MESSAGE, "turn_number 必须 >= 1")
                    return
            except ValueError:
                event_bus.publish_nowait("system", EventType.MESSAGE, f"turn_number 必须是数? {parts[1]}")
                return
        source = conversations.get_session_info(sid)
        if source is None:
            event_bus.publish_nowait("system", EventType.MESSAGE, f"源会话不存在: {sid}")
            return
        title = f"Fork: {source.title[:30]}"
        new_id = conversations.fork_session(sid, title=title, turn_number=turn_number)
        info = conversations.get_session_info(new_id)
        msg_count = info.message_count if info else "?"
        event_bus.publish_nowait("system", EventType.MESSAGE,
            f"?已分叉新会话: {new_id[:8]} ?{title}\n  消息? {msg_count}\n  源会? {sid[:8]} (轮次: {turn_number or '全部'})")

    elif base == "/memory":
        from goat.memory import MemoryManager
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
                event_bus.publish_nowait("system", EventType.MESSAGE, f"已记? {key}")
            else:
                content = mm.get(key)
                if content is None:
                    event_bus.publish_nowait("system", EventType.MESSAGE, f"记忆不存? {key}")
                else:
                    event_bus.publish_nowait("system", EventType.MESSAGE, f"[{key}]\n{content}")

    else:
        event_bus.publish_nowait("system", EventType.MESSAGE, f"未知命令: {cmd}，输?/help 查看帮助")

    return None


async def _check_tui_review_model(
    event_bus: EventBus,
    review_llm: ChatOpenAI,
    llm: ChatOpenAI,
    settings: dict | None,
) -> None:
    """TUI 侧检查审查模型是否独立，未独立且在首次时推送通知消息。"""
    if settings and settings.get("review_model_prompted"):
        return
    if review_llm is not llm:
        return
    event_bus.publish_nowait("system", EventType.NOTIFICATION,
                             "💡 Flow 模式建议使用独立审查模型以获得更客观的审查效果。可通过 /model review <模型名> 命令配置。",
                             agent_name="system")


async def _do_flow_chat(
    message: str,
    pipeline: FlowPipeline,
    event_bus: EventBus,
    conversations: ConversationManager,
    token_tracker: TokenTracker,
    cancel_token: CancellationToken,
):
    if cancel_token.is_cancelled():
        event_bus.publish_nowait("system", EventType.MESSAGE, "任务已取消", agent_name="system")
        return

    user_msg = HumanMessage(content=message)
    await conversations.add_message(user_msg)
    await conversations.compress_context()

    event_bus.publish_nowait("system", EventType.MESSAGE,
                             f"🔄 Flow 模式: 检测到复杂任务，进?实现→审查→修复 闭环",
                             agent_name="system")

    report = await pipeline.run(message)

    input_text = message
    output_text = report.summary
    token_tracker.record_turn(input_text, output_text)

    event_bus.publish_nowait("system", EventType.LLM_RESPONSE,
                             report.summary, agent_name="flow")
    event_bus.publish_nowait("system", EventType.COMPLETED, "completed", agent_name="system")


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
    hook_system: HookLifecycleSystem | None = None,
    workspace: Path | None = None,
    session_injected_context: str = "",
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
        hook_system=hook_system,
        depth=0,
    )
    subagent_tools = _build_subagent_tools(ctx)
    seen = {t.name for t in tools}
    for t in subagent_tools:
        if t.name not in seen:
            tools.append(t)
            seen.add(t.name)
    all_tools = tools

    skills = [f"{s.name} ?{s.description}" for s in skill_registry.list_all()]
    system_prompt = prompt_engine.render_main_system(
        "general", skills=skills, cwd=str(workspace or Path.cwd()),
        model=provider_config.model,
        provider=PROVIDER_DISPLAY_NAMES.get(
            provider_config.provider_type,
            provider_config.provider_type.value,
        ),
    )

    if session_injected_context:
        system_prompt += "\n\n" + session_injected_context

    user_msg = HumanMessage(content=message)
    await conversations.add_message(user_msg)
    await conversations.compress_context()

    llm_with_tools = llm.bind_tools(all_tools)

    event_bus.publish_nowait(
        "llm", EventType.LLM_STREAM, "", agent_name="assistant",
    )

    continuation_count = 0

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
            raw_content = str(response.content) if response.content else ""

            # ── 检测中途状态更新（非最终答案），自动催促 LLM 继续 ──
            if not looks_like_final_answer(raw_content):
                continuation_count += 1
                if continuation_count <= 3:
                    await conversations.add_message(response)
                    await conversations.add_message(HumanMessage(
                        content=continuation_prompt(continuation_count)
                    ))
                    event_bus.publish_nowait(
                        "system", EventType.MESSAGE,
                        f"任务未完成，自动继续（{continuation_count}/3）...",
                        agent_name="system",
                    )
                    continue
                else:
                    event_bus.publish_nowait(
                        "system", EventType.MESSAGE,
                        "连续3次无工具调用，退出等待用户指令",
                        agent_name="system",
                    )
                    await conversations.add_message(response)
                    content = raw_content if raw_content else "(无内容)"
                    event_bus.publish_nowait(
                        "llm", EventType.LLM_RESPONSE, content, agent_name="assistant",
                    )
                    event_bus.publish_nowait(
                        "system", EventType.COMPLETED, "completed", agent_name="system",
                    )
                    return

            continuation_count = 0
            content = raw_content if raw_content else "(无内容)"
            event_bus.publish_nowait(
                "llm", EventType.LLM_RESPONSE,
                content,
                agent_name="assistant",
            )
            await conversations.add_message(response)
            if hook_system:
                try:
                    await hook_system.on_stop("completed")
                except Exception:
                    pass
            event_bus.publish_nowait(
                "system", EventType.COMPLETED, "completed", agent_name="system",
            )
            return

        await conversations.add_message(response)

        continuation_count = 0  # 有工具调用，重置计数

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
                json.dumps({
                    "id": tc_id,
                    "tool_name": tc_name,
                    "args": tc_args,
                    "description": describe_tool_action(tc_name, tc_args),
                }, ensure_ascii=False),
                agent_name="assistant",
            )

            if hook_system:
                hook_cmd = str(tc_args.get("command", tc_args.get("filepath", "")))
                hook_target = str(tc_args.get("filepath", tc_args.get("directory", "")))
                hook_output = await hook_system.on_pre_tool_use(
                    tc_name, tc_args, command=hook_cmd, target_path=hook_target,
                )
                is_yolo = (approval_system is not None and
                           approval_system.context.mode == PermissionMode.YOLO)
                if not is_yolo and hook_output.decision == HookDecision.BLOCK:
                    msg = f"工具 '{tc_name}' 被钩子系统阻? {hook_output.reason}"
                    event_bus.publish_nowait(
                        "tool", EventType.ERROR, msg, agent_name="system",
                    )
                    tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                    await conversations.add_message(tool_msg)
                    continue

            if approval_system is not None:
                approval_result = await approval_system.evaluate_tool_call(
                    name=tc_name,
                    arguments=tc_args,
                    command=str(tc_args.get("command", tc_args.get("filepath", ""))),
                    target_path=str(tc_args.get("filepath", tc_args.get("directory", ""))),
                )
                if approval_result.decision == Decision.BLOCK:
                    msg = f"工具 '{tc_name}' 被审批系统拒? {approval_result.message}"
                    event_bus.publish_nowait(
                        "tool", EventType.ERROR, msg, agent_name="system",
                    )
                    tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                    await conversations.add_message(tool_msg)
                    continue
                elif approval_result.decision == Decision.ASK:
                    hook_auto_approved = False
                    if hook_system:
                        try:
                            perm_hook = await hook_system.on_permission_request(tc_name, tc_args)
                            if perm_hook and perm_hook.approve:
                                hook_auto_approved = True
                        except Exception:
                            pass
                    if hook_auto_approved:
                        event_bus.publish_nowait(
                            "tool", EventType.MESSAGE,
                            f"?钩子自动批准: {tc_name}",
                            agent_name="system",
                        )
                    else:
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
                result = f"工具不可? {tc_name}"
                if hook_system:
                    await hook_system.on_post_tool_use(tc_name, tc_args, result)
            else:
                try:
                    if tc_name == "execute_command":
                        cmd = tc_args.get("command", "")
                        wd = tc_args.get("working_dir", ".")
                        tmo = tc_args.get("timeout", 60)
                        event_bus.publish_nowait(
                            "tool", EventType.MESSAGE,
                            f"?执行命令: {cmd[:80]}",
                            agent_name="system",
                        )
                        stream_lines = []
                        def on_stdout(line: str):
                            stream_lines.append(line)
                            event_bus.publish_nowait(
                                "tool", EventType.TOOL_STDOUT, line, agent_name="system",
                            )
                        def on_stderr(line: str):
                            stream_lines.append(line)
                            event_bus.publish_nowait(
                                "tool", EventType.TOOL_STDERR, line, agent_name="system",
                            )
                        returncode, stdout_text, stderr_text = await execute_command_async(
                            cmd, working_dir=wd, timeout=tmo,
                            on_stdout=on_stdout, on_stderr=on_stderr,
                        )
                        output = stdout_text
                        if stderr_text:
                            output += "\n[stderr]\n" + stderr_text
                        if not output:
                            output = f"命令执行成功 (exit code: {returncode})"
                        if len(output) > 50000:
                            output = output[:50000] + f"\n\n[输出截断，共 {len(output)} 字符]"
                        result = output
                    elif asyncio.iscoroutinefunction(tool.ainvoke):
                        result = await tool.ainvoke(tc_args)
                    else:
                        result = tool.invoke(tc_args)
                    if hook_system:
                        post_results = await hook_system.on_post_tool_use(tc_name, tc_args, str(result))
                        if post_results:
                            for pr in post_results:
                                if isinstance(pr, dict) and pr.get("additionalContext"):
                                    result = str(result) + "\n\n[Hook Context] " + pr["additionalContext"]
                except Exception as e:
                    result = f"工具执行失败: {e}\n{traceback.format_exc()}"
                    if hook_system:
                        await hook_system.on_post_tool_use_failure(tc_name, tc_args, str(e))

            result_str = str(result)
            tool_result_payload = json.dumps({
                "tool_name": tc_name,
                "result": result_str[:10000],
                "tool_call_id": tc_id,
            })
            event_bus.publish_nowait(
                "tool", EventType.TOOL_RESULT,
                tool_result_payload,
                agent_name="assistant",
            )

            tool_msg = ToolMessage(content=result_str, tool_call_id=tc_id)
            await conversations.add_message(tool_msg)

        # ── FLOW mode: runtime review after mutation tools ──
        if (approval_system is not None
                and approval_system.context.mode == PermissionMode.FLOW
                and has_mutation_tools(response.tool_calls)):
            diff = get_git_diff()
            if diff:
                event_bus.publish_nowait("system", EventType.MESSAGE,
                                         "🔍 Flow 审查变更...", agent_name="system")
                findings = await run_mid_flow_review(message, diff, llm)
                if findings:
                    findings_text = _format_findings_text(findings)
                    review_msg = HumanMessage(
                        content=f"## 🔍 Code Review Findings\n\n{findings_text}\n\nPlease fix these issues."
                    )
                    await conversations.add_message(review_msg)
                    event_bus.publish_nowait("system", EventType.MESSAGE,
                                             f"📋 审查发现 {len(findings)} 个问题，继续修正...",
                                             agent_name="system")
                else:
                    event_bus.publish_nowait("system", EventType.MESSAGE,
                                             "?审查通过", agent_name="system")

    event_bus.publish_nowait(
        "system", EventType.MESSAGE,
        f"达到最大轮?({MAX_AGENT_TURNS})",
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


async def _test_llm_connection(config: ProviderConfig, timeout: int = 10) -> tuple[bool, str]:
    try:
        test_llm = create_llm(config)
        test_llm.request_timeout = timeout
        await test_llm.ainvoke([HumanMessage(content="hi")], max_tokens=1)
        return True, ""
    except Exception as e:
        err_str = str(e)
        if "429" in err_str or "insufficient_quota" in err_str:
            return False, f"API 配额不足 (429)，请检查账户余? {err_str[:120]}"
        if "401" in err_str or "Unauthorized" in err_str or "invalid_api_key" in err_str:
            return False, f"API Key 无效 (401)，请检查密? {err_str[:120]}"
        if "404" in err_str or "model_not_found" in err_str:
            return False, f"模型不存?(404)，请检查模型名: {err_str[:120]}"
        if "Connection" in err_str or "timeout" in err_str.lower():
            return False, f"连接失败，请检?Base URL: {err_str[:120]}"
        return False, f"连接测试失败: {err_str[:120]}"


def _build_config_from_settings(settings: dict) -> ProviderConfig:
    provider_str = settings.get("provider", "openai_compatible")
    provider_type = parse_provider(provider_str) or ProviderType.OPENAI_COMPATIBLE

    base_url = os.environ.get("BASE_URL") or settings.get("base_url", "")
    api_key = os.environ.get("API_KEY") or settings.get("api_key", "")
    model = os.environ.get("MODEL") or settings.get("model", "")

    return ProviderConfig(
        provider_type=provider_type,
        base_url=base_url,
        api_key=api_key,
        model=model,
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
                   review_model: str | None = None,
                   review_config: ProviderConfig | None = None,
                   custom_name: str | None = None):
    data: dict = {
        "provider": config.provider_type.value,
        "base_url": config.base_url,
        "api_key": config.api_key,
        "model": config.model,
    }
    if sub_model is not None:
        data["sub_model"] = sub_model
    if review_model is not None:
        if review_config:
            data["review_model"] = review_model or review_config.model
            data["review_api_key"] = review_config.api_key
            data["review_base_url"] = review_config.base_url
            data["review_provider"] = review_config.provider_type.value
        else:
            data["review_model"] = review_model
            data.pop("review_api_key", None)
            data.pop("review_base_url", None)
            data.pop("review_provider", None)
    if custom_name:
        data["custom_provider"] = custom_name
    try:
        if SETTINGS_FILE.exists():
            existing = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
            for k, v in existing.items():
                if k not in data:
                    data[k] = v
        SETTINGS_FILE.write_text(
            json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8"
        )
    except OSError:
        pass


def _init_skills(registry: SkillRegistry):
    candidates = [get_skills_dir()]
    try:
        pkg_skills = Path(__file__).resolve().parent / "skills"
        if pkg_skills not in candidates:
            candidates.append(pkg_skills)
    except Exception:
        pass
    for skills_dir in candidates:
        if skills_dir.exists():
            registry.load_skills_from_directory(skills_dir)

    if not registry.get("skill-creator"):
        sc_dir = Path(__file__).resolve().parent / "skills" / "skill-creator"
        if sc_dir.exists():
            from goat.agent.skill_system import load_skill_from_directory
            sc = load_skill_from_directory(sc_dir)
            if sc:
                registry.register(sc)

    if not registry.list_all():
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


async def _init_mcp_connections(event_bus: EventBus, settings: dict):
    try:
        from goat.mcp.mcp_config import load_mcp_config, McpServerConnection
        from goat.mcp.mcp_client import connect_mcp_servers

        config = load_mcp_config()
        mcp_enabled = settings.get("mcp", {}).get("enabled", config.enabled)
        if not mcp_enabled:
            return

        connections = list(config.connections)
        mcp_settings_conns = settings.get("mcp", {}).get("connections", [])
        if mcp_settings_conns:
            for c in mcp_settings_conns:
                connections.append(McpServerConnection(
                    name=c.get("name", "unknown"),
                    command=c.get("command"),
                    args=c.get("args", []),
                    url=c.get("url"),
                    env=c.get("env", {}),
                    transport=c.get("transport"),
                    headers=c.get("headers"),
                ))

        if not connections:
            return

        event_bus.publish_nowait(
            source_id="tui",
            event_type=EventType.MESSAGE,
            payload=f"正在连接 {len(connections)} ?MCP 服务?..",
            agent_name="system",
        )

        tools = await connect_mcp_servers(connections, event_bus)

        event_bus.publish_nowait(
            source_id="tui",
            event_type=EventType.NOTIFICATION,
            payload=f"MCP: {len(tools)} 个外部工具已就绪",
            agent_name="system",
        )
    except Exception as e:
        logger.error(f"MCP initialization failed: {e}")
        event_bus.publish_nowait(
            source_id="tui",
            event_type=EventType.ERROR,
            payload=f"MCP 初始化失? {e}",
            agent_name="system",
        )


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Goat TUI - 山羊主题终端 AI 编程助手")
    parser.add_argument("--workspace", "-w", default=None,
                        help="工作空间目录（默认 当前目录）")
    parser.add_argument("--provider", default=None,
                        help=f"LLM provider（默认从 setting.json 读取）")
    parser.add_argument("--model", default=None,
                        help="模型名称（默认从 setting.json 读取）")
    parser.add_argument("--inplace", action="store_true",
                        help="在当前终端运行，不弹出新窗口（Windows 默认弹新窗口）")
    args = parser.parse_args()

    if sys.platform == "win32" and not args.inplace:
        import subprocess
        script = Path(__file__).resolve()
        opts = ""
        if args.workspace:
            opts += f" -w \"{args.workspace}\""
        if args.provider:
            opts += f" --provider \"{args.provider}\""
        if args.model:
            opts += f" --model \"{args.model}\""
        opts += " --inplace"
        cmd = (
            f'start "Goat TUI" cmd /c '
            f'"{sys.executable} "{script}"{opts} & pause"'
        )
        subprocess.Popen(cmd, shell=True)
        return

    if sys.platform == "win32":
        try:
            import ctypes
            kernel32 = ctypes.windll.kernel32
            kernel32.SetConsoleCP(65001)
            kernel32.SetConsoleOutputCP(65001)
        except Exception:
            pass

    try:
        asyncio.run(run_tui(workspace=args.workspace))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()





