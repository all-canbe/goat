#!/usr/bin/env python3
"""
Goat ？gent 工具，支持子 Agent 并行调度与对话管？用法:
    # 运行 CLI
    goat

    # 运行 TUI
    goat_tui

启动后按提示输入 base_url、api_key、model 完成初始化？��后进入交互命令行，支持以下命令:

    /spawn <角色> <任务>  ？��动创建？gent
    /list                 ？��出所有子 Agent
    /collect [ids]        ？��集？gent 结果
    /cancel <id>          ？��消？gent
    /eval <id> <msg>      ？��子 Agent 发送消？   /sessions             ？��出对话历史
    /session [id]         ？��换会话
    /new [title]          ？��建新会？   /search <关键？      ？��索历史消息
    /export [id] [format] ？��出会话
    /skills [list]        ？��出可用技？   /skills find <描述>    ？��索？kill
    /skills install <url> [--global] ？��装 skill
    /roles                ？��出可用角色类型
    /status               ？��示系统状？   /help                 ？��示帮助
    /quit                 ？��？"""


from __future__ import annotations

import asyncio
import json
import re
import signal
import sys
import traceback
from pathlib import Path

sys.stdout.reconfigure(encoding='utf-8')

from langchain_core.messages import (
    SystemMessage, HumanMessage, ToolMessage,
)
from langchain_openai import ChatOpenAI

from goat.core.cancellation import CancellationToken
from goat.agent.subagent_manager import SubAgentManager, SubAgentStatus
from goat.agent.subagent_roles import (
    RoleType, ROLE_REGISTRY, get_role, list_roles,
)
from goat.agent.subagent_runtime import (
    AgentContext, _run_agent_loop, _build_subagent_tools, MAX_AGENT_TURNS,
)
from goat.agent.skill_system import (
    SkillRegistry, Skill,
    load_skill_from_directory,
    discover_skill_directories,
)
from goat.tools.tools import get_tools_by_names, BUILTIN_TOOLS
from goat.tools.steps_tracker import StepsTracker
from goat.tools.async_executor import execute_command_async
from goat.conversation.conversation_manager import ConversationManager, SessionInfo
from goat.core.event_bus import EventBus, EventType
from goat.core.workspace import get_goat_home, get_skills_dir, resolve_workspace
from goat.tasks.durable_task_manager import (
    DurableTaskManager, TaskDef, TaskContext, TaskType, TaskStatus,
)
from goat.conversation.prompt_engine import engine as prompt_engine
from goat.conversation.prompt_templates import (
    PLAN_MODE_DESCRIPTION, AGENT_MODE_DESCRIPTION,
    YOLO_MODE_DESCRIPTION, FLOW_MODE_DESCRIPTION,
)
from goat.provider.provider import (
    ProviderType, ProviderConfig, create_llm, get_provider_display,
    parse_provider, get_available_providers, PROVIDER_DEFAULTS, PROVIDER_DISPLAY_NAMES,
)
from goat.core.token_tracker import TokenTracker, calculate_cost, format_cost
from goat.security.approval import ToolApprovalSystem, PermissionMode, ApprovalPolicy, Decision
from goat.conversation.context_compression import CompactionConfig
from goat.hooks.lifecycle import HookLifecycleSystem, HookDecision, HookConfigLoader
from goat.agent.pipeline import (
    FlowPipeline, FlowReport, is_complex_task,
    has_mutation_tools, get_git_diff, run_mid_flow_review, _format_findings_text,
)


GOAT_HOME = get_goat_home()
SETTINGS_FILE = GOAT_HOME / "setting.json"


BANNER = r"""
╔═════════════════════════════════════════╗
🐐 Goat - 山羊主题 Agent 工具
Agent 并行调度 | 多模式审批 | 对话管理
╚═════════════════════════════════════════╝
"""

HELP_TEXT = """
模式说明:
  默认 [Agent 模式] 直接输入内容即与 AI 对话
  输入 /mode 切换[命令模式] 提示符变化

"""


class CLI:
    def __init__(self, workspace: str | None = None):
        self.workspace = resolve_workspace(workspace)
        self.llm: ChatOpenAI | None = None
        self.review_llm: ChatOpenAI | None = None
        self.manager: SubAgentManager | None = None
        self.conversations: ConversationManager | None = None
        self.task_manager: DurableTaskManager | None = None
        self.event_bus: EventBus | None = None
        self.skill_registry: SkillRegistry = SkillRegistry()
        self.cancel_token: CancellationToken = CancellationToken()
        self.running = True
        self.main_agent_id = "main"
        self.agent_mode = True
        self.provider_config: ProviderConfig | None = None
        self.token_tracker: TokenTracker | None = None
        self.approval_system: ToolApprovalSystem | None = None
        self.hook_system: HookLifecycleSystem | None = None
        self.pipeline: FlowPipeline | None = None
        self._event_listener_task: asyncio.Task | None = None

    def _load_settings(self) -> dict | None:
        try:
            if SETTINGS_FILE.exists():
                data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
                if data.get("api_key"):
                    return data
        except (json.JSONDecodeError, OSError):
            pass
        return None

    def _save_settings(self, base_url: str, api_key: str, model: str,
                       max_concurrent: int, max_depth: int,
                       provider: str = "openai_compatible",
                       sub_model: str | None = None,
                       review_model: str | None = None,
                       review_api_key: str | None = None,
                       review_base_url: str | None = None,
                       review_provider: str | None = None) -> None:
        data = {
            "provider": provider,
            "base_url": base_url,
            "api_key": api_key,
            "model": model,
            "max_concurrent": max_concurrent,
            "max_depth": max_depth,
        }
        if sub_model is not None:
            data["sub_model"] = sub_model
        if review_model is not None:
            data["review_model"] = review_model
            if review_api_key:
                data["review_api_key"] = review_api_key
                data["review_base_url"] = review_base_url or base_url
                data["review_provider"] = review_provider or provider
        try:
            if SETTINGS_FILE.exists():
                existing = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
                for k, v in existing.items():
                    if k not in data:
                        data[k] = v
            SETTINGS_FILE.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
        except OSError as e:
            print(f"  ⚠️ 配置保存失败: {e}")

    def _prompt_provider_selection(self) -> ProviderConfig:
        providers = get_available_providers()
        print("\n  选择 LLM Provider:")
        for i, p in enumerate(providers, 1):
            print(f"    {i}. {p['display']} (默认模型: {p['default_model']})")
        print(f"    {len(providers) + 1}. 手动输入")

        choice = input(f"\n  请选择 (1-{len(providers) + 1}, 默认 1): ").strip()
        if not choice:
            choice = "1"

        try:
            idx = int(choice) - 1
            if 0 <= idx < len(providers):
                selected = providers[idx]
                provider_type = ProviderType(selected["key"])
            else:
                raise ValueError
        except (ValueError, IndexError):
            provider_type = ProviderType.OPENAI_COMPATIBLE

        defaults = next(
            (p for p in providers if p["key"] == provider_type.value),
            providers[0],
        )

        base_url = input(f"  Base URL (默认 {defaults['default_base_url']}): ").strip()
        if not base_url:
            base_url = defaults["default_base_url"]

        api_key = input("  API Key: ").strip()
        while not api_key:
            print("  API Key 不能为空")
            api_key = input("  API Key: ").strip()

        model = input(f"  模型名称 (默认 {defaults['default_model']}): ").strip()
        if not model:
            model = defaults["default_model"]

        return ProviderConfig(
            provider_type=provider_type,
            base_url=base_url,
            api_key=api_key,
            model=model,
        )

    async def initialize(self) -> None:
        print(BANNER)

        settings = self._load_settings()
        if settings:
            provider_str = settings.get("provider", "openai_compatible")
            provider_type = parse_provider(provider_str) or ProviderType.OPENAI_COMPATIBLE
            base_url = settings.get("base_url", "")
            api_key = settings["api_key"]
            model = settings.get("model", "")
            max_concurrent = min(settings.get("max_concurrent", 10), 20)
            max_depth = settings.get("max_depth", 3)

            self.provider_config = ProviderConfig(
                provider_type=provider_type,
                base_url=base_url,
                api_key=api_key,
                model=model,
            )
            display = get_provider_display(self.provider_config)
            print(f"  读取已保存的配置: {display}\n")
        else:
            print("请配？LM 连接参数:\n")
            self.provider_config = self._prompt_provider_selection()

            max_concurrent = input("  最大并发子 Agent ？默认 10): ").strip()
            try:
                max_concurrent = int(max_concurrent) if max_concurrent else 10
            except ValueError:
                max_concurrent = 10
            max_concurrent = min(max_concurrent, 20)

            max_depth = input("  最大嵌套深？默认 3): ").strip()
            try:
                max_depth = int(max_depth) if max_depth else 3
            except ValueError:
                max_depth = 3

        print(f"\n  正在初始？.")
        self.llm = create_llm(self.provider_config)
        self.manager = SubAgentManager(
            max_concurrent=max_concurrent,
            max_spawn_depth=max_depth,
            state_file=str(GOAT_HOME / "subagents.json"),
        )

        self.conversations = ConversationManager(
            db_path=str(GOAT_HOME / "conversations.db"),
            max_tokens=128000,
            compression_config=CompactionConfig(
                context_window=128000,
                compaction_threshold_ratio=0.7,
                micro_compact_tool_count=10,
                micro_compact_min_tokens=5000,
                collapse_min_tokens=15000,
                hot_tail_size=3,
                prefer_cache_stability=False,
            ),
        )
        await self.conversations.create_session(model=self.provider_config.model, title="默认会话")

        self.event_bus = EventBus()
        from goat.tools.retry import init_retry
        init_retry(self.event_bus)

        self.task_manager = DurableTaskManager(
            db_path=str(GOAT_HOME / "tasks.db"),
            max_workers=4,
        )
        self._init_tasks()
        self.task_manager.start()
        recovered = await self.task_manager.recover()
        if recovered:
            print(f"  🔄 恢复 {len(recovered)} 个中断的任务")

        self._init_skills()
        self.review_llm = self.llm
        settings = self._load_settings()
        if settings and settings.get("review_api_key"):
            try:
                self.review_llm = create_llm(ProviderConfig(
                    provider_type=parse_provider(settings.get("review_provider", "")) or self.provider_config.provider_type,
                    base_url=settings.get("review_base_url", self.provider_config.base_url),
                    api_key=settings["review_api_key"],
                    model=settings.get("review_model", self.provider_config.model),
                ))
            except Exception as e:
                print(f"  ⚠️ 审查模型加载失败，使用主模型: {e}")
                self.review_llm = self.llm
        self.pipeline = FlowPipeline(
            impl_llm=self.llm,
            review_llm=self.review_llm,
            manager=self.manager,
            skill_registry=self.skill_registry,
            event_bus=self.event_bus,
            cancel_token=self.cancel_token,
            approval_system=self.approval_system,
            hook_system=self.hook_system,
        )
        self.token_tracker = TokenTracker(model=self.provider_config.model)
        self.approval_system = ToolApprovalSystem()
        self.approval_system.auto_configure()
        self.approval_system.set_mode(PermissionMode.DEFAULT)

        self.hook_system = HookLifecycleSystem()
        if settings and "hooks" in settings:
            try:
                raw = json.dumps({"hooks": settings["hooks"]}, ensure_ascii=False)
                hooks = HookConfigLoader.from_json(raw)
                for h in hooks:
                    self.hook_system.register(h)
                print(f"  🔌 已加载 {len(hooks)} 个钩子配置")
            except Exception as e:
                print(f"  ⚠️ 钩子配置加载失败: {e}")
        self._session_injected_context = ""
        try:
            session_results = await self.hook_system.on_session_start()
            if session_results:
                parts = []
                for r in session_results:
                    if isinstance(r, dict) and r.get("injected_prompt"):
                        parts.append(r["injected_prompt"])
                if parts:
                    self._session_injected_context = "\n".join(parts)
        except Exception:
            pass

        self._save_settings(
            self.provider_config.base_url,
            self.provider_config.api_key,
            self.provider_config.model,
            max_concurrent,
            max_depth,
            provider=self.provider_config.provider_type.value,
        )
        print(f"  ？��始化完？ {get_provider_display(self.provider_config)} | 并发上限: {max_concurrent} | 深度上限: {max_depth}\n")

        checkpoint = self.conversations.get_last_checkpoint()
        if checkpoint:
            print(f"  📌 发现上次退出时的会话断？")
            print(f"     会话: {checkpoint['title']}")
            print(f"     消息: {checkpoint['message_count']} ？ Token: {checkpoint['token_count']}")
            print(f"     输入 /resume 恢复，或直接开始新对话")

    def _init_tasks(self) -> None:
        """注册内置持久化任务类型"""

        async def explore_codebase(ctx: TaskContext) -> str:
            from goat.tools.tools import BUILTIN_TOOLS
            list_files = BUILTIN_TOOLS["list_files"]
            read_file = BUILTIN_TOOLS["read_file"]

            ctx.current_step = "浏览目录结构"
            result = list_files.invoke({"directory": ctx.metadata.get("directory", ".")})
            lines = [f"## 目录结构\n{result}"]

            ctx.current_step = "读取关键文件"
            targets = ctx.metadata.get("targets", [])
            for i, target in enumerate(targets):
                if ctx.cancel_token.is_set():
                    return "任务被取消"
                ctx.progress = (i + 1) / len(targets)
                content = read_file.invoke({"filepath": target})
                lines.append(f"\n## {target}\n{content}")

            return "\n---\n".join(lines)

        async def batch_process(ctx: TaskContext) -> str:
            cmds = ctx.metadata.get("commands", [])
            from goat.tools.tools import BUILTIN_TOOLS
            execute = BUILTIN_TOOLS["execute_command"]

            results = []
            for i, cmd in enumerate(cmds):
                if ctx.cancel_token.is_set():
                    return f"任务被取消，已完？{i}/{len(cmds)}"
                ctx.current_step = f"执行: {cmd[:60]}"
                ctx.progress = i / len(cmds)
                r = execute.invoke({"command": cmd})
                results.append(f"### [{i+1}/{len(cmds)}] {cmd}\n{r}")

            return "\n\n".join(results)

        self.task_manager.register_task_type(TaskDef(
            name="explore",
            description="探索代码库并生成结构化报告",
            fn=explore_codebase,
            task_type=TaskType.BATCH,
            metadata={"directory": ".", "targets": []},
        ))
        self.task_manager.register_task_type(TaskDef(
            name="batch_run",
            description="批量执行命令",
            fn=batch_process,
            task_type=TaskType.BATCH,
            priority=-1,
            metadata={"commands": []},
        ))

    def _init_skills(self) -> None:
        candidates = [get_skills_dir()]
        try:
            pkg_skills = Path(__file__).resolve().parent / "skills"
            if pkg_skills not in candidates:
                candidates.append(pkg_skills)
        except Exception:
            pass

        for skills_dir in candidates:
            if skills_dir.exists():
                loaded = self.skill_registry.load_skills_from_directory(skills_dir)
                if loaded:
                    print(f"  📦 Loaded {len(loaded)} skills: {', '.join(s.name for s in loaded)}")

        if not self.skill_registry.get("skill-creator"):
            sc_dir = Path(__file__).resolve().parent / "skills" / "skill-creator"
            if sc_dir.exists():
                from goat.agent.skill_system import load_skill_from_directory
                sc = load_skill_from_directory(sc_dir)
                if sc:
                    self.skill_registry.register(sc)
                    print(f"  📦 已注册初始技？skill-creator")

        if not self.skill_registry.list_all():
            from goat.tools.tools import BUILTIN_TOOLS as bt

            print("  ⚠️ skills/ 目录未找到 SKILL.md, 使用内置技能")
            self.skill_registry.register(Skill(
                name="code_explorer",
                description="代码库探索与分析",
                tools=[bt["list_files"], bt["read_file"], bt["search_code"]],
                metadata={"role": "explore"},
            ))
            self.skill_registry.register(Skill(
                name="code_writer",
                description="代码编写与修改",
                tools=[bt["read_file"], bt["write_file"], bt["search_code"], bt["execute_command"]],
                metadata={"role": "implementer"},
            ))
            self.skill_registry.register(Skill(
                name="code_reviewer",
                description="代码审查",
                tools=[bt["read_file"], bt["search_code"], bt["execute_command"]],
                metadata={"role": "review"},
            ))
            self.skill_registry.register(Skill(
                name="test_runner",
                description="测试执行与验证",
                tools=[bt["read_file"], bt["execute_command"], bt["search_code"]],
                metadata={"role": "verifier"},
            ))
            self.skill_registry.register(Skill(
                name="task_planner",
                description="任务分解与规划",
                tools=[bt["list_files"], bt["read_file"], bt["write_file"]],
                metadata={"role": "plan"},
            ))

    async def run(self) -> None:
        await self.initialize()

        self._event_listener_task = asyncio.create_task(self._event_listener())

        print("输入 /help 查看命令？uit 退出\n")
        print("当前？Agent 模式]，直接输入内容即？I 对话\n")

        while self.running:
            try:
                prefix = "🤖 > " if self.agent_mode else "🔧 > "
                user_input = await asyncio.get_event_loop().run_in_executor(
                    None, lambda: input(prefix).strip()
                )
            except (EOFError, KeyboardInterrupt):
                print("\n正在退？.")
                break

            if not user_input:
                continue

            if user_input.startswith("/"):
                find_query: str | None = None
                parts = user_input.strip().split(maxsplit=1)
                if len(parts) >= 2 and parts[0] == "/skills":
                    after_skills = parts[1].strip()
                    if after_skills.lower().startswith("find"):
                        find_query = after_skills[4:].lstrip()
                if find_query:
                    prompt = (
                        f"请使？ind-skills skill 搜索与「{find_query}」相关的可用 skill。\n"
                        f"如果 find-skills skill 不可用，请通过 WebSearch 搜索 npx skills 仓库。\n"
                        f"请清晰地列出找到的每？kill 的：名称、描述、安？RL。\n"
                        f"搜索完毕后我会告知你安装选项。"
                    )
                    await self._do_chat(prompt)

                    # ── 交互式安装选择 ──
                    print(f"\n{'─'*50}")
                    print("📋 以上是搜索结果。输？kill URL 进行安装（从上方列表复制），或输？ 取消:")
                    choice = await asyncio.get_event_loop().run_in_executor(
                        None, lambda: input("  请输？").strip()
                    )
                    if choice and choice != "0":
                        scope = await asyncio.get_event_loop().run_in_executor(
                            None, lambda: input("  安装？(1) 本项？(2) 全局? [默认 1]: ").strip()
                        )
                        install_args = choice
                        if scope == "2":
                            install_args += " --global"
                        await self._install_skill(install_args)
                    else:
                        print("  已取消安装")
                else:
                    await self._handle_command(user_input)
            else:
                await self._do_chat(user_input)

        await self._cleanup()

    async def _event_listener(self) -> None:
        queue = await self.event_bus.stream("cli_main")
        while self.running:
            try:
                event = await asyncio.wait_for(queue.get(), timeout=0.5)
            except asyncio.TimeoutError:
                continue

            if event.event_type == EventType.LLM_STREAM:
                print(event.payload, end="", flush=True)
            else:
                print(f"  {event.short()}")

            queue.task_done()

    async def _handle_command(self, raw: str) -> None:
        parts = raw.split(maxsplit=1)
        cmd = parts[0].lower()
        args = parts[1] if len(parts) > 1 else ""

        match cmd:
            case "/spawn":
                spawn_parts = args.split(maxsplit=1)
                if len(spawn_parts) < 2:
                    print("用法: /spawn <角色> <任务描述>")
                    print("角色: general, explore, plan, implementer, review, verifier")
                    return
                await self._do_spawn(spawn_parts[0], spawn_parts[1])
            case "/list":
                print(self.manager.format_agent_list())
            case "/collect":
                ids = [i.strip() for i in args.split(",") if i.strip()] if args else None
                result = await self.manager.collect_results(ids)
                print(result)
            case "/cancel":
                if not args:
                    print("用法: /cancel <agent_id>")
                    return
                await self.manager.cancel(args.strip())
                print(f"已取？gent {args.strip()}")
            case "/eval":
                eval_parts = args.split(maxsplit=1)
                if len(eval_parts) < 2:
                    print("用法: /eval <agent_id> <消息>")
                    return
                result = await self.manager.send_message(eval_parts[0], eval_parts[1])
                print(result)
            case "/skills":
                sub = args.split(maxsplit=1)
                subcmd = sub[0].lower() if sub else ""
                sub_rest = sub[1] if len(sub) > 1 else ""
                if subcmd == "install":
                    await self._install_skill(sub_rest)
                elif subcmd.startswith("find"):
                    query = subcmd[4:].strip() or sub_rest
                    if not query:
                        print("用法: /skills find <描述>")
                    else:
                        print(f"🔍 正在搜索 skill: {query}，请等待 Agent 响应...")
                else:
                    self._list_skills()
            case "/roles":
                self._list_roles()
            case "/provider":
                await self._switch_provider(args)
            case "/model":
                self._switch_model(args)
            case "/cost":
                self._show_cost()
            case "/resume":
                await self._resume_session(args)
            case "/fork":
                await self._fork_session(args)
            case "/status":
                self._show_status()
            case "/help":
                print(HELP_TEXT)
            case "/mode":
                self.agent_mode = not self.agent_mode
                mode_name = "Agent" if self.agent_mode else "命令"
                print(f"已切换到 [{mode_name} 模式]")
                if self.agent_mode:
                    print("直接输入内容即可？I 对话，输？ 开头为命令")
                else:
                    print("提示符已变为 🔧，直接输入内容可？I 对话，输？ 开头为命令")
            case "/plan":
                self.approval_system.set_mode(PermissionMode.PLAN)
                print("已切换到 [Plan 模式] 🔍")
                print("  只读模式 — 探索代码并制定计划")
                print("  计划完成后，系统会询问你是否执行")
                print("  输入 /agent ？olo 可直接切换到执行模式")
            case "/agent":
                self.approval_system.set_mode(PermissionMode.DEFAULT)
                print("已切换到 [Agent 模式] 🤖")
                print("  默认交互模式 ？��次操作都会询问确认")
            case "/yolo":
                self.approval_system.set_mode(PermissionMode.YOLO)
                print("已切换到 [YOLO 模式] 😈")
                print("  全部自动批准（安全守卫仍生效），谨慎操作")
            case "/flow":
                self.approval_system.set_mode(PermissionMode.FLOW)
                print("已切换到 [Flow 模式] 🔄")
                print("  流程模式 ？��杂任务自动进入 实现→审查→修复 闭环")
                print("  简单问题直接回答，不消耗审？oken")
            case "/sessions":
                self._list_sessions()
            case "/session":
                self._switch_session(args)
            case "/new":
                await self._new_session(args)
            case "/session_rename":
                rename_parts = args.split(maxsplit=1)
                if len(rename_parts) < 2:
                    print("用法: /session_rename <session_id> <新名？")
                    return
                sid = self.conversations.resolve_session_id(rename_parts[0])
                if not sid:
                    print(f"未找到会？{rename_parts[0]}（多个匹配或不存在）")
                    return
                if self.conversations.rename_session(sid, rename_parts[1]):
                    print(f"会话 {sid[:8]} 已重命名？{rename_parts[1]}")
                else:
                    print(f"重命名失？{rename_parts[0]}")
            case "/session_delete":
                if not args:
                    print("用法: /session_delete <session_id>")
                    return
                sid = self.conversations.resolve_session_id(args.strip())
                if not sid:
                    print(f"未找到会？{args.strip()}（多个匹配或不存在）")
                    return
                if self.conversations.delete_session(sid):
                    print(f"已删除会？{sid[:8]}")
                else:
                    print(f"删除会话失败: {sid[:8]}")
            case "/export":
                export_parts = args.split(maxsplit=1)
                fmt = export_parts[1] if len(export_parts) > 1 else "text"
                raw_sid = export_parts[0] if export_parts else ""
                if not raw_sid:
                    sid = self.conversations.current_session_id
                else:
                    sid = self.conversations.resolve_session_id(raw_sid)
                if not sid:
                    print("没有当前会话或会话不存在")
                    return
                result = self.conversations.export_session(sid, format=fmt)
                if fmt == "json":
                    filepath = f"session_{sid[:8]}.json"
                    Path(filepath).write_text(result, encoding="utf-8")
                    print(f"已导出到 {filepath}")
                else:
                    print(result[:2000])
                    if len(result) > 2000:
                        print(f"... (？{len(result)} 字符)")
            case "/search":
                if not args:
                    print("用法: /search <关键？")
                    return
                results = self.conversations.search_messages(args.strip())
                if not results:
                    print("未找到匹配消息")
                    return
                print(f"\n找到 {len(results)} 条匹配消？")
                for r in results[:10]:
                    preview = r.content[:100].replace("\n", " ")
                    print(f"  [{r.session_id[:8]}] {r.role}: {preview}")
            case "/task":
                await self._submit_task(args)
            case "/task_list":
                self._list_tasks(args)
            case "/task_cancel":
                if not args:
                    print("用法: /task_cancel <task_id>")
                    return
                msg = await self.task_manager.cancel(args.strip())
                print(msg)
            case "/task_pause":
                if not args:
                    print("用法: /task_pause <task_id>")
                    return
                msg = await self.task_manager.pause(args.strip())
                print(msg)
            case "/task_resume":
                if not args:
                    print("用法: /task_resume <task_id>")
                    return
                msg = await self.task_manager.resume(args.strip())
                print(msg)
            case "/task_recover":
                recovered = await self.task_manager.recover()
                if recovered:
                    print(f"已恢？{len(recovered)} 个任？")
                    for r in recovered:
                        print(f"  {r.task_id} ？{r.name}: {r.description[:50]}")
                else:
                    print("没有需要恢复的任务")
            case "/quit":
                self.running = False
                print("再见! 👋")
            case _:
                print(f"未知命令: {cmd}，输？help 查看帮助")

    def _get_mode_instructions(self) -> tuple[str, str]:
        if self.approval_system is None:
            return "Agent", AGENT_MODE_DESCRIPTION
        mode = self.approval_system.context.mode
        if mode == PermissionMode.PLAN:
            return "Plan (只读规划)", PLAN_MODE_DESCRIPTION
        elif mode == PermissionMode.YOLO:
            return "YOLO (自动)", YOLO_MODE_DESCRIPTION
        elif mode == PermissionMode.FLOW:
            return "Flow (流程)", FLOW_MODE_DESCRIPTION
        return "Agent (执行)", AGENT_MODE_DESCRIPTION

    async def _handle_plan_approval(self, plan_path: Path | None = None) -> None:
        sep = "─" * 50
        print(f"\n{sep}", flush=True)
        print(f"  📋 Plan 已完成，请选择:", flush=True)
        print(f"{sep}", flush=True)
        if plan_path:
            print(f"  📄 计划文件: {plan_path}")
        print(f"  (a) Agent 模式执行 ？��换？GENT 模式执行（每次确认）")
        print(f"  (y) YOLO 模式执行   ？��换？OLO 模式自动执行")
        print(f"  (n) 继续修改        ？��持 Plan 模式继续完善")
        print()
        choice = await asyncio.get_event_loop().run_in_executor(
            None, lambda: input("  请选择 [a/y/n]: ").strip().lower()
        )

        if choice in ("a", "agent"):
            self.approval_system.set_mode(PermissionMode.DEFAULT)
            print(f"\n  ？��切换到 Agent 模式，开始执行计？.", flush=True)
            await self._do_chat("请按照上述计划开始执行，一步步完成。")
        elif choice in ("y", "yolo"):
            self.approval_system.set_mode(PermissionMode.YOLO)
            print(f"\n  🤖 已切换到 YOLO 模式，开始自动执行计？.", flush=True)
            await self._do_chat("请按照上述计划开始执行，一步步完成。")
        else:
            print(f"\n  📝 继续完善计划...", flush=True)

    async def _do_chat(self, message: str) -> None:
        is_flow_mode = (self.approval_system is not None and
                        self.approval_system.context.mode == PermissionMode.FLOW)

        if is_flow_mode and is_complex_task(message):
            await self._do_flow(message)
            return

        """子Agent 对话: 流式输出 + 对话管理 + 可自定 spawn 子Agent"""
        tools = get_tools_by_names(
            ROLE_REGISTRY[RoleType.GENERAL].allowed_tools,
        )

        steps_tracker = StepsTracker()
        ctx = AgentContext(
            agent_id=self.main_agent_id,
            agent_name="main",
            role_type=RoleType.GENERAL,
            role_def=ROLE_REGISTRY[RoleType.GENERAL],
            cancel_token=self.cancel_token,
            message_queue=asyncio.Queue(),
            event_bus=self.event_bus,
            llm=self.llm,
            tools=tools,
            skill_registry=self.skill_registry,
            subagent_manager=self.manager,
            conversation_manager=self.conversations,
            approval_system=self.approval_system,
            hook_system=self.hook_system,
            depth=0,
            steps_tracker=steps_tracker,
        )
        subagent_tools = _build_subagent_tools(ctx)
        seen = {t.name for t in tools}
        for t in subagent_tools:
            if t.name not in seen:
                tools.append(t)
                seen.add(t.name)
        all_tools = tools

        skills = [f"{s.name} ？{s.description}" for s in self.skill_registry.list_all()]
        mode_label, mode_desc = self._get_mode_instructions()
        system_prompt = prompt_engine.render_main_system(
            "general", skills=skills, cwd=str(self.workspace),
            model=self.provider_config.model,
            provider=PROVIDER_DISPLAY_NAMES.get(
                self.provider_config.provider_type,
                self.provider_config.provider_type.value,
            ),
            mode=mode_label,
            mode_description=mode_desc,
        )

        if getattr(self, "_session_injected_context", ""):
            system_prompt += "\n\n" + self._session_injected_context

        user_msg = HumanMessage(content=message)
        await self.conversations.add_message(user_msg)

        await self.conversations.compress_context()

        llm_with_tools = self.llm.bind_tools(all_tools)

        print(f"\n🤖 ？gent 正在处理: {message[:80]}...\n")

        for turn in range(MAX_AGENT_TURNS):
            if self.cancel_token.is_cancelled():
                print("⚠️ 任务被取消")
                return

            messages = [
                SystemMessage(content=system_prompt),
                *self.conversations.get_messages(),
            ]

            print("🤖 ", end="", flush=True)
            collected_chunks = []

            try:
                async for chunk in llm_with_tools.astream(messages):
                    if self.cancel_token.is_cancelled():
                        break
                    collected_chunks.append(chunk)
                    if chunk.content:
                        print(chunk.content, end="", flush=True)
            except Exception as e:
                print(f"\n？LM 调用失败: {e}")
                return

            print(flush=True)

            if not collected_chunks:
                print("⚠️ 未获取到响应")
                return

            response = collected_chunks[0]
            for c in collected_chunks[1:]:
                response += c

            input_text = "\n".join(m.content or "" for m in messages)
            output_text = str(response.content) if response.content else ""
            if self.token_tracker:
                self.token_tracker.record_turn(input_text, output_text)

            if not response.tool_calls:
                content = str(response.content) if response.content else "(无内容)"
                print(flush=True)

                is_plan_mode = (
                    self.approval_system is not None
                    and self.approval_system.context.mode == PermissionMode.PLAN
                )
                is_flow_mode = (
                    self.approval_system is not None
                    and self.approval_system.context.mode == PermissionMode.FLOW
                )

                plan_path = None
                if is_plan_mode:
                    plan_path = self._save_plan(content)

                skip_verification = is_plan_mode or is_flow_mode
                if not skip_verification:
                    verification_result = await self._verify_task_completion(messages)
                    if not verification_result["passed"]:
                        await self.conversations.add_message(HumanMessage(
                            content=f"验证未通过：{verification_result['reason']}\n请修复后继续。"
                        ))
                        print(f"    🔄 验证未通过，继续修正。", flush=True)
                        continue

                await self.conversations.add_message(response)

                if is_plan_mode:
                    await self._handle_plan_approval(plan_path)

                if self.hook_system:
                    try:
                        await self.hook_system.on_stop("completed")
                    except Exception:
                        pass
                return

            await self.conversations.add_message(response)

            tool_name_map = {t.name: t for t in all_tools}
            for tc in response.tool_calls:
                if self.cancel_token.is_cancelled():
                    print("⚠️ 任务被取消")
                    return

                tc_name = tc.get("name", tc.get("function", {}).get("name", "unknown"))
                tc_args = tc.get("args", tc.get("function", {}).get("arguments", {}))
                if isinstance(tc_args, str):
                    try:
                        tc_args = json.loads(tc_args)
                    except json.JSONDecodeError:
                        tc_args = {}
                tc_id = tc.get("id", "")

                print(f"  🔧 调用工具: {tc_name}", flush=True)

                if self.approval_system is not None:
                    approval_result = await self.approval_system.evaluate_tool_call(
                        name=tc_name,
                        arguments=tc_args,
                        command=str(tc_args.get("command", tc_args.get("filepath", ""))),
                        target_path=str(tc_args.get("filepath", tc_args.get("directory", ""))),
                    )
                    if approval_result.decision == Decision.BLOCK:
                        msg = f"工具 '{tc_name}' 被审批系统拒？{approval_result.message}"
                        print(f"    ？{msg}", flush=True)
                        tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                        await self.conversations.add_message(tool_msg)
                        continue
                    elif approval_result.decision == Decision.ASK:
                        hook_auto_approved = False
                        if self.hook_system:
                            try:
                                perm_hook = await self.hook_system.on_permission_request(tc_name, tc_args)
                                if perm_hook and perm_hook.approve:
                                    hook_auto_approved = True
                            except Exception:
                                pass
                        if hook_auto_approved:
                            print(f"    ？��子自动批准: {tc_name}", flush=True)
                        else:
                            detail = _format_tool_detail(tc_name, tc_args)
                            print(f"    ？{approval_result.message}", flush=True)
                            if detail:
                                print(detail, flush=True)
                            confirm = await asyncio.get_event_loop().run_in_executor(
                                None, lambda: input("    确认执行? [y/N]: ").strip().lower()
                            )
                            if confirm not in ("y", "yes"):
                                msg = f"用户拒绝工具 '{tc_name}'"
                                print(f"    ？{msg}", flush=True)
                                tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                            await self.conversations.add_message(tool_msg)
                            continue

                if self.hook_system:
                    hook_cmd = str(tc_args.get("command", tc_args.get("filepath", "")))
                    hook_target = str(tc_args.get("file_path", tc_args.get("filepath", tc_args.get("directory", ""))))
                    hook_output = await self.hook_system.on_pre_tool_use(
                        tc_name, tc_args, command=hook_cmd, target_path=hook_target,
                    )
                    is_yolo = (self.approval_system is not None and
                               self.approval_system.context.mode == PermissionMode.YOLO)
                    if not is_yolo and hook_output.decision == HookDecision.BLOCK:
                        msg = f"工具 '{tc_name}' 被钩子系统阻？{hook_output.reason}"
                        print(f"    ？{msg}", flush=True)
                        tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                        await self.conversations.add_message(tool_msg)
                        continue

                tool = tool_name_map.get(tc_name)
                if tool is None:
                    result = f"工具不可？{tc_name}"
                    if self.hook_system:
                        await self.hook_system.on_post_tool_use(tc_name, tc_args, result)
                else:
                    try:
                        if tc_name == "execute_command":
                            cmd = tc_args.get("command", "")
                            wd = tc_args.get("working_dir", ".")
                            tmo = tc_args.get("timeout", 60)
                            print(f"    ？��行？. ({cmd[:80]})", flush=True)
                            print(f"    {'─' * 60}", flush=True)
                            stream_lines = []
                            def on_line(line: str):
                                stream_lines.append(line)
                                print(f"      {line}", flush=True)
                            returncode, stdout_text, stderr_text = await execute_command_async(
                                cmd, working_dir=wd, timeout=tmo,
                                on_stdout=on_line, on_stderr=on_line,
                            )
                            print(f"    {'─' * 60}", flush=True)
                            print(f"    ？��成 (exit code: {returncode})", flush=True)
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
                        if self.hook_system:
                            post_results = await self.hook_system.on_post_tool_use(tc_name, tc_args, str(result))
                            if post_results:
                                for pr in post_results:
                                    if isinstance(pr, dict) and pr.get("additionalContext"):
                                        result_str_ctx = str(result) + "\n\n[Hook Context] " + pr["additionalContext"]
                                        result = result_str_ctx
                    except Exception as e:
                        result = f"工具执行失败: {e}\n{traceback.format_exc()}"
                        if self.hook_system:
                            await self.hook_system.on_post_tool_use_failure(tc_name, tc_args, str(e))

                result_str = str(result)
                if tc_name == "execute_command" and len(result_str) > 500:
                    preview = result_str[:500]
                    print(f"    结果摘要: {preview}... (？{len(result_str)} 字符)", flush=True)
                elif tc_name != "execute_command":
                    if len(result_str) > 500:
                        preview = result_str[:500]
                        print(f"    结果: {preview}... (？{len(result_str)} 字符)", flush=True)
                    else:
                        print(f"    结果: {result_str}", flush=True)

                tool_msg = ToolMessage(content=result_str, tool_call_id=tc_id)
                await self.conversations.add_message(tool_msg)

            # ── FLOW mode: runtime review after mutation tools ──
            if (self.approval_system is not None
                    and self.approval_system.context.mode == PermissionMode.FLOW
                    and has_mutation_tools(response.tool_calls)):
                diff = get_git_diff()
                if diff:
                    print("    🔍 Flow 审查变更...", flush=True)
                    findings = await run_mid_flow_review(message, diff, self.llm)
                    if findings:
                        findings_text = _format_findings_text(findings)
                        review_msg = HumanMessage(
                            content=f"## 🔍 Code Review Findings\n\n{findings_text}\n\nPlease fix these issues."
                        )
                        await self.conversations.add_message(review_msg)
                        print(f"    📋 审查发现 {len(findings)} 个问题，继续修正...", flush=True)
                    else:
                        print("    ？��查通过", flush=True)

        print(f"\n⚠️ 达到最大轮？{MAX_AGENT_TURNS})", flush=True)

        if (self.approval_system is not None
                and self.approval_system.context.mode == PermissionMode.PLAN):
            plan_content = "\n".join(
                m.content for m in messages
                if isinstance(m, (HumanMessage,)) or (hasattr(m, 'content') and isinstance(m.content, str) and not isinstance(m, ToolMessage))
            )
            plan_path = self._save_plan(plan_content) if plan_content.strip() else None
            await self._handle_plan_approval(plan_path)

    # ── P0: Stop Hook ？��务完成验证 ──

    _VERIFY_MUTATION = frozenset({
        "write_file", "delete_file", "move_file", "copy_file",
        "file_edit", "apply_diff", "edit", "write",
        "git_commit", "git_push",
    })

    _VERIFY_MUTATION_ERROR_PATTERNS = re.compile(
        r"(permission\s*denied|no\s*such\s*file|disk\s*full|write\s*error|无法写入|写入失败)",
        re.IGNORECASE,
    )

    async def _verify_task_completion(self, messages: list) -> dict:
        mutation_calls = []
        mutation_results = []
        collecting_result = False

        for msg in reversed(messages):
            if isinstance(msg, ToolMessage) and msg.content and collecting_result:
                mutation_results.append(msg.content[:500])
                collecting_result = False
            elif hasattr(msg, "tool_calls") and msg.tool_calls:
                for tc in msg.tool_calls:
                    name = tc.get("name", tc.get("function", {}).get("name", ""))
                    args = tc.get("args", tc.get("function", {}).get("arguments", {}))
                    if isinstance(args, str):
                        try:
                            args = json.loads(args)
                        except (json.JSONDecodeError, TypeError):
                            args = {}
                    if name in self._VERIFY_MUTATION:
                        mutation_calls.append((name, args))
                        collecting_result = True
            if len(mutation_calls) >= 3:
                break

        for result_text in mutation_results:
            if self._VERIFY_MUTATION_ERROR_PATTERNS.search(result_text):
                return {"passed": False, "reason": "写入操作执行失败，需要修改"}

        for name, args in mutation_calls:
            file_path = args.get("file_path") or args.get("filepath") or ""
            if file_path:
                path = Path(file_path)
                if path.suffix == ".py":
                    try:
                        proc = await asyncio.create_subprocess_exec(
                            sys.executable, "-c",
                            f"import ast; ast.parse(open(r'{path}').read())",
                            stdout=asyncio.subprocess.PIPE,
                            stderr=asyncio.subprocess.PIPE,
                        )
                        _, stderr = await proc.communicate()
                        if stderr:
                            return {"passed": False, "reason": f"Python 语法错误: {stderr.decode()[:200]}"}
                    except Exception as e:
                        return {"passed": False, "reason": f"文件验证失败: {e}"}

        return {"passed": True, "reason": ""}

    def _save_plan(self, content: str) -> Path:
        from datetime import datetime

        plans_dir = self.workspace / ".plans"
        plans_dir.mkdir(parents=True, exist_ok=True)

        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        plan_file = plans_dir / f"plan_{timestamp}.md"

        header = (
            f"# Plan ？{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n\n"
        )
        plan_file.write_text(header + content, encoding="utf-8")
        print(f"  📄 计划已保？{plan_file}", flush=True)
        return plan_file

    async def _do_spawn(self, role_str: str, task: str) -> None:
        try:
            role_type = RoleType(role_str.lower())
        except ValueError:
            valid = [r.value for r in RoleType]
            print(f"无效的角？{role_str}'，可？{valid}")
            return

        role_def = get_role(role_type)
        tools = get_tools_by_names(role_def.allowed_tools)

        ctx = AgentContext(
            agent_id=self.main_agent_id,
            agent_name="main",
            role_type=role_type,
            role_def=role_def,
            cancel_token=self.cancel_token,
            message_queue=asyncio.Queue(),
            event_bus=self.event_bus,
            llm=self.llm,
            tools=tools,
            skill_registry=self.skill_registry,
            subagent_manager=self.manager,
            conversation_manager=self.conversations,
            approval_system=self.approval_system,
            depth=0,
        )

        agent = await self.manager.spawn(
            parent_id=self.main_agent_id,
            role_type=role_type,
            task_description=task,
            parent_cancel_token=self.cancel_token,
            spawn_depth=1,
        )

        subagent_tools = _build_subagent_tools(ctx)
        if role_def.can_spawn:
            seen = {t.name for t in tools}
            for t in subagent_tools:
                if t.name not in seen:
                    tools.append(t)
                    seen.add(t.name)

        agent.status = SubAgentStatus.RUNNING
        agent.task_handle = asyncio.create_task(
            _run_agent_loop(
                AgentContext(
                    agent_id=agent.agent_id,
                    agent_name=agent.name,
                    role_type=role_type,
                    role_def=role_def,
                    cancel_token=agent.cancel_token,
                    message_queue=agent.message_queue,
                    event_bus=self.event_bus,
                    llm=self.llm,
                    tools=tools,
                    skill_registry=self.skill_registry,
                    subagent_manager=self.manager,
                    conversation_manager=self.conversations,
                    approval_system=self.approval_system,
                    depth=1,
                ),
            )
        )

        print(f"？？Agent 已启？{agent.name} [{agent.agent_id}]")
        print(f"   角色: {role_type.value} | 任务: {task[:80]}")

    async def _do_flow(self, task: str) -> None:
        if not task:
            print("用法: /flow <任务描述>")
            return
        if not self.pipeline:
            print("  ⚠️ FlowPipeline 未初始化")
            return

        impl_model = self.provider_config.model
        review_model = self.review_llm.model if hasattr(self.review_llm, 'model') else impl_model

        print(f"\n{'─'*50}")
        print(f"  📋 /flow ？mplement ？eview ？ix")
        print(f"  🛠？实现: {impl_model}  |  🔍 审查: {review_model} (只读)")
        print(f"  📝 任务: {task[:100]}")
        print(f"  📐 最大迭代 3 次")
        print(f"{'─'*50}")

        report = await self.pipeline.run(task)

        print(f"\n{'─'*50}")
        print(f"  /flow Report")
        print(f"{'─'*50}")

        findings_count = 0
        for i, findings in enumerate(report.findings_history, 1):
            if findings:
                findings_count += len(findings)
                print(f"\n  🔍 Round {i} ？{len(findings)} issue(s):")
                for f in findings:
                    print(f"    [{f.severity.upper()}] {f.file_path}:{f.line or '?'} - {f.description}")
            else:
                print(f"\n  🔍 Round {i} ？ASS")

        print(f"\n  {'─'*48}")
        print(f"  {report.summary}")
        print(f"  总计发现问题: {findings_count}")
        print(f"  状？{'？ASS' if report.success else '？ARTIAL (max iterations)'}")
        print()

    async def _resume_session(self, args: str) -> None:
        if not args:
            checkpoint = self.conversations.get_last_checkpoint()
            if checkpoint:
                sid = checkpoint["session_id"]
                print(f"  发现上次断点:")
                print(f"    会话: {checkpoint['title']}")
                print(f"    消息？{checkpoint['message_count']}")
                print(f"    Token: {checkpoint['token_count']}")
                resp = await asyncio.get_event_loop().run_in_executor(
                    None, lambda: input("  恢复该会？[Y/n]: ").strip().lower()
                )
                if resp in ("", "y", "yes"):
                    self.conversations.load_session(sid)
                    self.conversations.clear_checkpoint()
                    info = self.conversations.get_session_info(sid)
                    print(f"  Restored session {info.title} ({info.message_count} msgs, {info.token_count} tokens)")
                    return
                print("  已跳过恢复")
            else:
                print("  没有可恢复的断点")
            return

        sid = self.conversations.resolve_session_id(args.strip())
        if not sid:
            print(f"  未找到会？{args.strip()}（多个匹配或不存在）")
            sessions = self.conversations.list_sessions(limit=5)
            if sessions:
                print("  最近的会话:")
                for s in sessions:
                    print(f"    {s.session_id[:8]} ？{s.title} ({s.message_count} 条消？")
            return
        if self.conversations.load_session(sid):
            info = self.conversations.get_session_info(sid)
            if info:
                self.conversations.clear_checkpoint()
                print(f"  Restored session {info.title} ({info.message_count} msgs, {info.token_count} tokens)")
            else:
                print(f"  Restored session {sid[:8]}")
        else:
            print(f"  会话不存？{sid[:8]}")

    async def _fork_session(self, args: str) -> None:
        parts = args.split(maxsplit=1)
        if not parts:
            print("用法: /fork <session_id> [turn_number]")
            print("  turn_number: 分叉到指定轮次（1 开始），不指定则复制全会话")
            return

        sid = self.conversations.resolve_session_id(parts[0].strip())
        if not sid:
            print(f"  未找到会？{parts[0].strip()}（多个匹配或不存在）")
            return
        turn_number = None
        if len(parts) > 1:
            try:
                turn_number = int(parts[1].strip())
                if turn_number < 1:
                    print("  turn_number 必须 >= 1")
                    return
            except ValueError:
                print(f"  turn_number 必须是数？{parts[1]}")
                return

        source = self.conversations.get_session_info(sid)
        if source is None:
            print(f"  源会话不存在: {sid}")
            return

        title_suffix = f" (轮次 {turn_number})" if turn_number else " (全部)"
        title = f"Fork: {source.title[:30]}"

        new_id = self.conversations.fork_session(sid, title=title, turn_number=turn_number)
        info = self.conversations.get_session_info(new_id)
        msg_count = info.message_count if info else "?"
        print(f"  Forked new session: {new_id[:8]} {title}")
        print(f"    消息？{msg_count}")
        print(f"    源会？{sid[:8]} (轮次: {turn_number or '全部'})")

    async def _switch_provider(self, args: str) -> None:
        if not args:
            print(f"\n  当前 Provider: {get_provider_display(self.provider_config)}")
            print(f"  Base URL: {self.provider_config.base_url}")
            print(f"\n  可用 Provider:")
            for p in get_available_providers():
                print(f"    {p['key']:20s} ？{p['display']}")
            print(f"\n  用法: /provider <类型>")
            return

        provider_type = parse_provider(args.strip())
        if provider_type is None:
            print(f"  无效？rovider: {args}")
            print(f"  可？openai_compatible, anthropic")
            return

        if provider_type == self.provider_config.provider_type:
            print(f"  当前已经？{PROVIDER_DISPLAY_NAMES[provider_type]}")
            return

        defaults = PROVIDER_DEFAULTS.get(provider_type.value, {})

        api_key = input(f"  {PROVIDER_DISPLAY_NAMES[provider_type]} API Key: ").strip()
        while not api_key:
            print("  API Key 不能为空")
            api_key = input(f"  {PROVIDER_DISPLAY_NAMES[provider_type]} API Key: ").strip()

        base_url = input(f"  Base URL (默认 {defaults.get('base_url', '')}): ").strip()
        if not base_url:
            base_url = defaults.get("base_url", "")

        model = input(f"  模型 (默认 {defaults.get('model', '')}): ").strip()
        if not model:
            model = defaults.get("model", "")

        self.provider_config = ProviderConfig(
            provider_type=provider_type,
            base_url=base_url,
            api_key=api_key,
            model=model,
        )

        self.llm = create_llm(self.provider_config)
        if self.token_tracker:
            self.token_tracker.model = self.provider_config.model
        else:
            self.token_tracker = TokenTracker(model=self.provider_config.model)
        self._save_settings(
            self.provider_config.base_url,
            self.provider_config.api_key,
            self.provider_config.model,
            self.manager.max_concurrent,
            self.manager.max_spawn_depth,
            provider=self.provider_config.provider_type.value,
        )
        print(f"  ？��切换到: {get_provider_display(self.provider_config)}\n")

    def _switch_model(self, args: str) -> None:
        if args and args.strip():
            model = args.strip()
            self.provider_config.model = model
            self.llm = create_llm(self.provider_config)
            if self.token_tracker:
                self.token_tracker.model = model
            self._save_settings(
                self.provider_config.base_url,
                self.provider_config.api_key,
                self.provider_config.model,
                self.manager.max_concurrent,
                self.manager.max_spawn_depth,
                provider=self.provider_config.provider_type.value,
            )
            print(f"  ？？Agent 模型已切换为: {model}\n")
            return

        self._show_model_menu()

    def _get_current_sub_model(self) -> str:
        settings = self._load_settings()
        if settings and settings.get("sub_model"):
            return settings["sub_model"]
        return self.provider_config.model

    def _get_current_review_model(self) -> str:
        if self.review_llm and hasattr(self.review_llm, 'model'):
            return self.review_llm.model
        settings = self._load_settings()
        if settings and settings.get("review_model"):
            return settings["review_model"]
        return self.provider_config.model

    def _show_model_menu(self) -> None:
        sub = self._get_current_sub_model()
        rv = self._get_current_review_model()
        while True:
            print(f"\n{'─'*50}")
            print(f"  📋 当前模型配置:")
            print(f"    1. ？gent:  {self.provider_config.model}")
            print(f"    2. ？gent:  {sub}")
            print(f"    3. 审查模型:  {rv}")
            print(f"    4. 新增自定？rovider")
            print(f"    0. 返回")
            choice = input(f"\n  请选择 (0-4): ").strip()
            if choice == "0":
                break
            elif choice == "1":
                self._prompt_switch_main(sub, rv)
                sub = self._get_current_sub_model()
                rv = self._get_current_review_model()
            elif choice == "2":
                self._prompt_switch_sub(sub, rv)
                sub = self._get_current_sub_model()
            elif choice == "3":
                self._prompt_switch_review(sub, rv)
                rv = self._get_current_review_model()
            elif choice == "4":
                self._prompt_add_provider()
                sub = self._get_current_sub_model()
                rv = self._get_current_review_model()
            else:
                print("  无效选择")

    def _prompt_model_selection(self, target_name: str, current: str,
                                alt1_name: str, alt1: str,
                                alt2_name: str, alt2: str) -> str | None:
        print(f"\n  当前 {target_name}: {current}")
        print(f"  请选择:")
        print(f"    1. 使用 {alt1_name}: {alt1}")
        print(f"    2. 使用 {alt2_name}: {alt2}")
        print(f"    3. 输入其他模型")
        print(f"    0. 取消")
        choice = input(f"\n  请选择 (0-3): ").strip()
        if choice == "0":
            return None
        elif choice == "1":
            return alt1
        elif choice == "2":
            return alt2
        elif choice == "3":
            return input(f"  输入 {target_name}: ").strip()
        print("  无效选择")
        return None

    def _prompt_switch_main(self, sub: str, rv: str) -> None:
        model = self._prompt_model_selection("？gent 模型",
                                             self.provider_config.model,
                                             "？gent 模型", sub,
                                             "审查模型", rv)
        if not model:
            return
        self.provider_config.model = model
        self.llm = create_llm(self.provider_config)
        if self.token_tracker:
            self.token_tracker.model = model
        self._save_settings(
            self.provider_config.base_url,
            self.provider_config.api_key,
            model,
            self.manager.max_concurrent,
            self.manager.max_spawn_depth,
            provider=self.provider_config.provider_type.value,
        )
        print(f"  ？？Agent 模型已切换为: {model}")

    def _prompt_switch_sub(self, sub: str, rv: str) -> None:
        model = self._prompt_model_selection("？gent 模型", sub,
                                             "？gent 模型", self.provider_config.model,
                                             "审查模型", rv)
        if not model:
            return
        self._save_settings(
            self.provider_config.base_url,
            self.provider_config.api_key,
            self.provider_config.model,
            self.manager.max_concurrent,
            self.manager.max_spawn_depth,
            provider=self.provider_config.provider_type.value,
            sub_model=model,
        )
        print(f"  ？？Agent 模型已切换为: {model}")

    def _prompt_switch_review(self, sub: str, rv: str) -> None:
        print(f"\n  当前审查模型: {rv}")
        print(f"  请选择:")
        print(f"    1. 复用？gent 模型: {self.provider_config.model}")
        print(f"    2. 使用？gent 模型: {sub}")
        print(f"    3. 输入其他模型")
        print(f"    4. 独立配置 (自定？PI Key/URL)")
        print(f"    0. 取消")
        choice = input(f"\n  请选择 (0-4): ").strip()
        if choice == "0":
            return
        elif choice == "1":
            model = self.provider_config.model
            self.review_llm = self.llm
            self._save_settings(
                self.provider_config.base_url,
                self.provider_config.api_key,
                self.provider_config.model,
                self.manager.max_concurrent,
                self.manager.max_spawn_depth,
                provider=self.provider_config.provider_type.value,
                review_model=model,
                review_api_key=None,
            )
            print(f"  ？��查模型已复用主 Agent: {model}")
        elif choice == "2":
            model = sub
            self.review_llm = create_llm(ProviderConfig(
                provider_type=self.provider_config.provider_type,
                base_url=self.provider_config.base_url,
                api_key=self.provider_config.api_key,
                model=model,
            ))
            self._save_settings(
                self.provider_config.base_url,
                self.provider_config.api_key,
                self.provider_config.model,
                self.manager.max_concurrent,
                self.manager.max_spawn_depth,
                provider=self.provider_config.provider_type.value,
                review_model=model,
                review_api_key=None,
            )
            print(f"  ？��查模型已切换为: {model}")
        elif choice == "3":
            model = input(f"  输入审查模型？").strip()
            if not model:
                return
            self.review_llm = create_llm(ProviderConfig(
                provider_type=self.provider_config.provider_type,
                base_url=self.provider_config.base_url,
                api_key=self.provider_config.api_key,
                model=model,
            ))
            self._save_settings(
                self.provider_config.base_url,
                self.provider_config.api_key,
                self.provider_config.model,
                self.manager.max_concurrent,
                self.manager.max_spawn_depth,
                provider=self.provider_config.provider_type.value,
                review_model=model,
                review_api_key=None,
            )
            print(f"  ？��查模型已切换为: {model}")
        elif choice == "4":
            providers = get_available_providers()
            print("\n  选择审查 Provider:")
            for i, p in enumerate(providers, 1):
                print(f"    {i}. {p['display']}")
            p_choice = input(f"\n  请选择 (1-{len(providers)}): ").strip()
            try:
                idx = int(p_choice) - 1
                selected = providers[idx] if 0 <= idx < len(providers) else providers[0]
                rv_provider = ProviderType(selected["key"])
            except (ValueError, IndexError):
                rv_provider = self.provider_config.provider_type
                selected = next((p for p in providers if p["key"] == rv_provider.value), providers[0])
            rv_key = input(f"  审查 API Key: ").strip()
            if not rv_key:
                return
            rv_url = input(f"  审查 Base URL (默认 {selected['default_base_url']}): ").strip()
            if not rv_url:
                rv_url = selected["default_base_url"]
            rv_model = input(f"  审查模型 (默认 {selected['default_model']}): ").strip()
            if not rv_model:
                rv_model = selected["default_model"]
            self.review_llm = create_llm(ProviderConfig(
                provider_type=rv_provider,
                base_url=rv_url,
                api_key=rv_key,
                model=rv_model,
            ))
            self._save_settings(
                self.provider_config.base_url,
                self.provider_config.api_key,
                self.provider_config.model,
                self.manager.max_concurrent,
                self.manager.max_spawn_depth,
                provider=self.provider_config.provider_type.value,
                review_model=rv_model,
                review_api_key=rv_key,
                review_base_url=rv_url,
                review_provider=rv_provider.value,
            )
            print(f"  ？��查模型已配置为独立 Provider: {rv_model}")

    def _prompt_add_provider(self) -> None:
        print(f"\n  新增自定？rovider (将替换主 Agent 配置)")
        providers = get_available_providers()
        print("  选择 Provider 类型:")
        for i, p in enumerate(providers, 1):
            print(f"    {i}. {p['display']}")
        p_choice = input(f"\n  请选择 (1-{len(providers)}): ").strip()
        try:
            idx = int(p_choice) - 1
            selected = providers[idx] if 0 <= idx < len(providers) else providers[0]
            new_provider = ProviderType(selected["key"])
        except (ValueError, IndexError):
            new_provider = ProviderType.OPENAI_COMPATIBLE
            selected = providers[0]
        base_url = input(f"  Base URL (默认 {selected['default_base_url']}): ").strip()
        if not base_url:
            base_url = selected["default_base_url"]
        api_key = input(f"  API Key: ").strip()
        if not api_key:
            print("  API Key 不能为空")
            return
        model = input(f"  模型名称 (默认 {selected['default_model']}): ").strip()
        if not model:
            model = selected["default_model"]
        self.provider_config = ProviderConfig(
            provider_type=new_provider,
            base_url=base_url,
            api_key=api_key,
            model=model,
        )
        self.llm = create_llm(self.provider_config)
        if self.token_tracker:
            self.token_tracker.model = model
        self._save_settings(
            base_url, api_key, model,
            self.manager.max_concurrent,
            self.manager.max_spawn_depth,
            provider=new_provider.value,
        )
        print(f"  ？��切换至？rovider: {get_provider_display(self.provider_config)}\n")

    def _list_skills(self) -> None:
        skills = self.skill_registry.list_all()
        if not skills:
            print("没有注册的技能")
            print("  /skills install <repo_url> [--skill <name>] [--global] — 安装技能")
            print("  /skills find <描述> — 搜索技能")
            return
        print("\n已注册的技？")
        for s in skills:
            detail = s.description or "无描？"

            print(f"  📦 {s.name} ？{detail}")
        print()

    async def _install_skill(self, args: str) -> None:
        parts = args.split()
        if not parts:
            print("用法: /skills install <repo_url> [--skill <name>] [--global]")
            print("示例: /skills install https://github.com/vercel-labs/skills --skill find-skills")
            print("      /skills install https://github.com/vercel-labs/skills --skill find-skills --global")
            return
        use_global = "--global" in parts
        if use_global:
            parts = [p for p in parts if p != "--global"]
        import shutil
        npx = shutil.which("npx")
        if not npx:
            print("？��找？px，请先安？ode.js (https://nodejs.org)")
            return
        cmd = f"npx skills add {' '.join(parts)}"
        print(f"📦 执行: {cmd}")
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
                self.skill_registry.load_skills_from_directory(skills_dir)
                scope = "全局" if use_global else "本项？"

                print(f"？��装成功 ({scope})\n{out}")
            else:
                print(f"？��装失败 (exit {proc.returncode})\n{err or out}")
        except asyncio.TimeoutError:
            try:
                proc.kill()
            except ProcessLookupError:
                pass
            print("？��装超时 (120s)")
        except Exception as e:
            try:
                proc.kill()
            except (ProcessLookupError, UnboundLocalError):
                pass
            print(f"？��装出错: {e}")

    def _list_roles(self) -> None:
        print("\n可用角色类型:")
        for role_def in list_roles():
            spawn = "？？spawn" if role_def.can_spawn else "？��可 spawn"
            tools_str = ", ".join(role_def.allowed_tools[:5])
            if len(role_def.allowed_tools) > 5:
                tools_str += f" ... 等{len(role_def.allowed_tools)}种"
            print(f"  {role_def.display_name}")
            print(f"    工具: {tools_str}")
            print(f"    能力: {spawn}")
        print()

    def _list_sessions(self) -> None:
        sessions = self.conversations.list_sessions()
        if not sessions:
            print("没有历史会话")
            return

        current = self.conversations.current_session_id
        print("\n会话列表:")
        for s in sessions:
            marker = " ◀ 当前" if s.session_id == current else ""
            created = s.created_at[:19].replace("T", " ")
            print(f"  {s.session_id[:8]} | {s.title[:30]:30s} | {s.message_count:3d} 条消？ {created}{marker}")
        print()

    def _switch_session(self, session_id: str) -> None:
        session_id = session_id.strip("[]")
        if not session_id:
            sessions = self.conversations.list_sessions()
            if sessions:
                for s in sessions:
                    print(f"  {s.session_id[:8]} ？{s.title} ({s.message_count} 条消？")
            return

        sid = self.conversations.resolve_session_id(session_id)
        if not sid:
            print(f"未找到会？{session_id}（多个匹配或不存在）")
            return
        if self.conversations.load_session(sid):
            self.conversations.clear_checkpoint()
            info = self.conversations.list_sessions()
            matched = [s for s in info if s.session_id == sid]
            if matched:
                s = matched[0]
                print(f"已切换到会话: {s.title} ({s.message_count} 条消息 {s.token_count} tokens)")
        else:
            print(f"会话不存在 {session_id}")

    async def _new_session(self, title: str) -> None:
        session_id = await self.conversations.create_session(
            model=self.provider_config.model,
            title=title.strip() or "新对话",
        )
        self.conversations.clear_checkpoint()
        info = self.conversations.list_sessions()
        matched = [s for s in info if s.session_id == session_id]
        if matched:
            s = matched[0]
            print(f"已创建并切换到新会话: {s.title} ({s.session_id[:8]})")

    async def _submit_task(self, raw: str) -> None:
        if not raw:
            print("用法: /task <任务？[描述] [metadata:{...}]")
            print(f"可用任务类型: explore, batch_run")
            return

        parts = raw.split(maxsplit=1)
        task_name = parts[0].strip()
        rest = parts[1] if len(parts) > 1 else ""

        import re
        meta_match = re.search(r'metadata:(\{.*\})', rest)
        metadata = {}
        if meta_match:
            try:
                metadata = json.loads(meta_match.group(1))
            except json.JSONDecodeError:
                print(f"⚠️ metadata JSON 解析失败")
                return
            description = rest[:meta_match.start()].strip()
        else:
            description = rest.strip()

        task_id, err = await self.task_manager.submit(
            task_name, description=description, metadata=metadata,
        )

        if err:
            print(f"？{err}")
        else:
            print(f"？��务已提？{task_id}")
            print(f"   名称: {task_name} | 描述: {description[:60]}")

    def _list_tasks(self, status_filter: str) -> None:
        if status_filter:
            try:
                st = TaskStatus(status_filter)
                tasks = self.task_manager.list_tasks(status=st.value)
            except ValueError:
                print(f"无效的状？{status_filter}，可？pending, running, completed, failed, cancelled, paused")
                return
        else:
            tasks = self.task_manager.list_tasks(limit=50)

        if not tasks:
            print("没有任务")
            return

        print(f"\n任务列表 ({len(tasks)} 个):")
        for t in tasks:
            icon = {
                "pending": "⏳", "running": "🔄", "completed": "✅",
                "failed": "❌", "cancelled": "🚫", "paused": "⏸️",
            }.get(t.status, "❓")
            created = t.created_at[:19].replace("T", " ")
            step = f" | {t.current_step[:30]}" if t.current_step and t.status in ("running", "pending") else ""
            print(f"  {icon} [{t.task_id}] {t.name} ({t.status}){step}")
            print(f"      描述: {t.description[:60]}{'...' if len(t.description) > 60 else ''}")
            print(f"      创建: {created} | 进度: {t.progress:.0%}")
        print()

    def _show_cost(self) -> None:
        if not self.token_tracker:
            print("\n  Token 跟踪器未初始化")
            return

        summary = self.token_tracker.summary()
        pricing = calculate_cost(self.provider_config.model, 1000, 1000)

        print(f"\nToken 用量统计:")
        print(f"  模型: {summary['model']}")
        print(f"  对话轮次: {summary['turns']}")
        print(f"  输入 Tokens: {summary['total_input_tokens']:,}")
        print(f"  输出 Tokens: {summary['total_output_tokens']:,}")
        print(f"  总计 Tokens: {summary['total_tokens']:,}")
        print(f"  估算成本: {summary['total_cost_str']}")
        print(f"  (输入 ${pricing['input_rate']}/M tokens | "
              f"输出 ${pricing['output_rate']}/M tokens)")
        print()

    def _show_status(self) -> None:
        agents = self.manager.list_agents()
        running = sum(1 for a in agents if a.status == SubAgentStatus.RUNNING)
        completed = sum(1 for a in agents if a.status == SubAgentStatus.COMPLETED)
        failed = sum(1 for a in agents if a.status == SubAgentStatus.FAILED)
        cancelled = sum(1 for a in agents if a.status == SubAgentStatus.CANCELLED)

        print(f"\n系统状态")
        mode_label = {
            PermissionMode.DEFAULT: "Agent",
            PermissionMode.PLAN: "Plan 🔍",
            PermissionMode.FLOW: "Flow 🔄",
            PermissionMode.YOLO: "YOLO 😈",
            PermissionMode.ACCEPT_EDITS: "AcceptEdits",
            PermissionMode.BYPASS: "Bypass",
            PermissionMode.AUTO: "Auto",
        }.get(self.approval_system.context.mode, self.approval_system.context.mode.value)
        print(f"  Provider: {get_provider_display(self.provider_config)}")
        print(f"  审批模式: {mode_label}")
        print(f"  Base URL: {self.provider_config.base_url}")
        print(f"  SubAgent Session: {self.manager.session_boot_id}")
        print(f"  并发上限: {self.manager.max_concurrent}")
        print(f"  深度上限: {self.manager.max_spawn_depth}")
        print(f"  ？gent 总数: {len(agents)}")
        print(f"    运行？{running}")
        print(f"    已完？{completed}")
        print(f"    失败: {failed}")
        print(f"    已取？{cancelled}")
        print()

        if self.conversations:
            token_info = self.conversations.get_token_usage()
            current = self.conversations.current_session_id or "(？"

            print(f"  对话系统:")
            print(f"    当前会话: {current[:8]}")
            print(f"    总会话数: {len(self.conversations.list_sessions(limit=9999))}")
            print(f"    当前消息？{token_info['message_count']}")
            print(f"    当前 Tokens: {token_info['total_tokens']} / {token_info['max_tokens']}")
            print()

        if self.event_bus:
            print(f"  事件总线: 运行中")
            print()

        if self.token_tracker:
            summary = self.token_tracker.summary()
            print(f"  Token 用量:")
            print(f"    总计: {summary['total_tokens']:,} tokens")
            print(f"    估算成本: {summary['total_cost_str']}")
            print(f"    轮次: {summary['turns']}")
            print()

        checkpoint = self.conversations.get_last_checkpoint()
        if checkpoint:
            print(f"  断点: {checkpoint['title']} ({checkpoint['message_count']} 条消？")
            print()

        if self.task_manager:
            running = self.task_manager.running_count
            pending = self.task_manager.pending_count
            total = len(self.task_manager.list_tasks(limit=9999))
            print(f"  后台任务管理？")
            print(f"    Workers: {self.task_manager.worker_count}")
            print(f"    运行？{running} | 排队？{pending} | 总计: {total}")
            print()

    async def _cleanup(self) -> None:
        self.cancel_token.cancel()
        if self.hook_system:
            await self.hook_system.on_session_end("cleanup")
        if self._event_listener_task is not None:
            self._event_listener_task.cancel()
        if self.task_manager:
            await self.task_manager.stop(wait=True)
            self.task_manager.close()
        if self.manager:
            await self.manager.shutdown()
        if self.conversations:
            if self.conversations.current_session_id:
                cpid = self.conversations.save_checkpoint()
                if cpid:
                    print("  💾 已保存会话断点")
            self.conversations.close()


def _format_tool_detail(name: str, args: dict) -> str:
    lines = []
    target = args.get("file_path") or args.get("filepath") or args.get("directory") or args.get("path") or ""
    if target:
        lines.append(f"    改哪里? {target}")
    if name in ("write_file", "edit_file", "write") and args.get("content"):
        content = args["content"]
        preview = content[:200].replace("\n", "\\n")
        lines.append(f"    改什？{preview}")
    elif name in ("edit_file",) and args.get("new_str"):
        preview = args["new_str"][:200].replace("\n", "\\n")
        lines.append(f"    改什？{preview}")
    elif name in ("execute_command", "Bash", "shell") and args.get("command"):
        cmd = args["command"]
        lines.append(f"    命令: {cmd[:200]}")
    elif name == "search_code" and args.get("query"):
        lines.append(f"    查询: {args['query']}")
    if not lines:
        arg_str = json.dumps(args, ensure_ascii=False)
        if len(arg_str) > 200:
            arg_str = arg_str[:200] + "..."
        lines.append(f"    参数: {arg_str}")
    return "\n".join(lines)


def _mcp_add_from_cli(payload: str):
    try:
        data = json.loads(payload)
    except json.JSONDecodeError:
        print("错误: JSON 解析失败，请检查粘贴内？")
        return

    servers = data.get("mcpServers", data)
    if not isinstance(servers, dict):
        print("错误: 需要mcpServers 字典格式")
        return

    from goat.mcp.mcp_config import McpServerConnection, save_mcp_config, load_mcp_config

    connections = []
    for name, server_def in servers.items():
        if not isinstance(server_def, dict):
            print(f"错误: 服务？{name}] 配置格式错误")
            return
        connections.append(McpServerConnection(
            name=name,
            command=server_def.get("command"),
            args=server_def.get("args", []),
            url=server_def.get("url"),
            env=server_def.get("env", {}),
            transport=server_def.get("transport"),
            headers=server_def.get("headers"),
        ))

    existing = load_mcp_config()
    merged = list(existing.connections)
    new_names = {c.name for c in connections}
    merged = [c for c in merged if c.name not in new_names]
    merged.extend(connections)

    path = save_mcp_config(merged)
    names = ", ".join(c.name for c in connections)
    print(f"MCP 服务器已接入: {names}")
    print(f"配置文件: {path}")
    print("下次启动 TUI 或运？goat mcp connect' 即可使用")


def _run_mcp_command(args: list[str]):
    if not args:
        print("用法: goat mcp <serve|serve-sse|connect|list|add>")
        print("  serve      启动 stdio MCP 服务？？laude Desktop / Cursor 等连？")
        print("  serve-sse  启动 SSE/HTTP MCP 服务？")
        print("  connect    连接配置？CP 服务器并注入工具")
        print("  list       列出已配置的 MCP 连接")
        print("  add        快捷添加 MCP 服务？？goat mcp add '<标准 mcpServers JSON>'")
        return

    subcmd = args[0]
    if subcmd == "serve":
        from goat.mcp.mcp_server import start_mcp_server
        tool_names = None
        if len(args) > 1 and args[1] == "--tools" and len(args) > 2:
            tool_names = [t.strip() for t in args[2].split(",") if t.strip()]
        asyncio.run(start_mcp_server(tool_names=tool_names))
    elif subcmd == "serve-sse":
        from goat.mcp.mcp_sse_server import start_mcp_sse_server
        host = "127.0.0.1"
        port = 8800
        i = 1
        while i < len(args):
            if args[i] == "--host" and i + 1 < len(args):
                host = args[i + 1]; i += 2
            elif args[i] == "--port" and i + 1 < len(args):
                port = int(args[i + 1]); i += 2
            else:
                i += 1
        asyncio.run(start_mcp_sse_server(host=host, port=port))
    elif subcmd == "connect":
        from goat.mcp.mcp_config import load_mcp_config
        from goat.mcp.mcp_client import connect_mcp_servers
        config = load_mcp_config()
        if not config.connections:
            print("No MCP connections configured. Please check goat/mcp_config.json")
            return
        tools = asyncio.run(connect_mcp_servers(config.connections))
        print(f"已连？{len(tools)} ？CP 工具")
        for t in tools:
            print(f"  - {t.name}: {t.description[:80]}")
    elif subcmd == "list":
        from goat.mcp.mcp_config import load_mcp_config
        config = load_mcp_config()
        if not config.connections:
            print("No MCP connections configured.")

            return
        print("已配置的 MCP 连接:")
        for conn in config.connections:
            transport = "stdio" if conn.command else "sse" if conn.url else "unknown"
            print(f"  - {conn.name} ({transport})")
    elif subcmd == "add":
        if len(args) < 2:
            print("用法: goat mcp add '<标准 mcpServers JSON>'")
            print('  ？goat mcp add \'{"My Server": {"command": "npx", "args": ["-y", "package"]}}\'')
            return
        _mcp_add_from_cli(args[1])
    else:
        print(f"未知子命？{subcmd}")
        print("可用: serve, serve-sse, connect, list, add")


def web_command(workspace: str | None = None):
    """启动 Goat Web 服务."""
    from goat.api.server import start_server
    start_server(open_browser=False, workspace=workspace)


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "web":
        import argparse
        p = argparse.ArgumentParser()
        p.add_argument("--workspace", "-w", default=None)
        args, _ = p.parse_known_args(sys.argv[2:])
        web_command(workspace=args.workspace)
        return

    if len(sys.argv) > 1 and sys.argv[1] == "mcp":
        _run_mcp_command(sys.argv[2:])
        return

    import argparse
    parser = argparse.ArgumentParser(description="Goat CLI - 山羊主题 Agent 工具")
    parser.add_argument("--workspace", "-w", default=None,
                        help="Workspace directory (default: current dir)")
    args = parser.parse_args()

    cli = CLI(workspace=args.workspace)

    def sig_handler(signum, frame):
        cli.running = False
        cli.cancel_token.cancel()

    signal.signal(signal.SIGINT, sig_handler)
    signal.signal(signal.SIGTERM, sig_handler)

    try:
        asyncio.run(cli.run())
    except KeyboardInterrupt:
        print("\nInterrupted")
    except Exception as e:
        print(f"\nError: {e}")
        traceback.print_exc()


if __name__ == "__main__":
    main()