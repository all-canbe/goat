#!/usr/bin/env python3
"""
SubAgent Demo — LangChain + 子 Agent 并行调度系统 + SQLite 对话管理

用法:
    python main.py

启动后按提示输入 base_url、api_key、model 完成初始化。
然后进入交互命令行，支持以下命令:

    /spawn <角色> <任务>  — 手动创建子 Agent
    /list                 — 列出所有子 Agent
    /collect [ids]        — 收集子 Agent 结果
    /cancel <id>          — 取消子 Agent
    /eval <id> <msg>      — 向子 Agent 发送消息
    /sessions             — 列出对话历史
    /session [id]         — 切换会话
    /new [title]          — 创建新会话
    /search <关键词>       — 搜索历史消息
    /export [id] [format] — 导出会话
    /skills               — 列出可用技能
    /roles                — 列出可用角色类型
    /status               — 显示系统状态
    /help                 — 显示帮助
    /quit                 — 退出
"""

from __future__ import annotations

import asyncio
import json
import signal
import sys
import traceback
from pathlib import Path

sys.stdout.reconfigure(encoding='utf-8')

from langchain_core.messages import (
    SystemMessage, HumanMessage, ToolMessage,
)
from langchain_openai import ChatOpenAI

from my_tui.core.cancellation import CancellationToken
from my_tui.agent.subagent_manager import SubAgentManager, SubAgentStatus
from my_tui.agent.subagent_roles import (
    RoleType, ROLE_REGISTRY, get_role, list_roles,
)
from my_tui.agent.subagent_runtime import (
    AgentContext, _run_agent_loop, _build_subagent_tools, MAX_AGENT_TURNS,
)
from my_tui.agent.skill_system import (
    SkillRegistry, Skill,
    load_skill_from_directory,
    discover_skill_directories,
)
from my_tui.tools.tools import get_tools_by_names, BUILTIN_TOOLS
from my_tui.conversation.conversation_manager import ConversationManager, SessionInfo
from my_tui.core.event_bus import EventBus, EventType
from my_tui.tasks.durable_task_manager import (
    DurableTaskManager, TaskDef, TaskContext, TaskType, TaskStatus,
)
from my_tui.conversation.prompt_engine import engine as prompt_engine
from my_tui.provider.provider import (
    ProviderType, ProviderConfig, create_llm, get_provider_display,
    parse_provider, get_available_providers, PROVIDER_DEFAULTS, PROVIDER_DISPLAY_NAMES,
)
from my_tui.core.token_tracker import TokenTracker, calculate_cost, format_cost
from my_tui.security.approval import ToolApprovalSystem, PermissionMode, ApprovalPolicy, Decision
from my_tui.conversation.context_compression import CompactionConfig


SETTINGS_FILE = Path("setting.json")


BANNER = r"""
╔══════════════════════════════════════════════════════╗
║      � SubAgent Demo — 子 Agent 并行调度系统        ║
║      LangChain + asyncio + SQLite 对话管理           ║
╚══════════════════════════════════════════════════════╝
"""

HELP_TEXT = """
模式说明:
  默认 [Agent 模式] — 直接输入内容即与 AI 对话
  输入 /mode 切换到 [命令模式] — 提示符变化，两种模式下均支持直接输入聊天或 / 命令

审批模式:
  /agent                — 默认模式，每次操作询问确认
  /plan                 — 只读调查模式，写操作/Shell 全部阻止
  /yolo                 — 全部自动批准（安全守卫仍生效）

可用命令:
  /mode                 — 切换 Agent/命令模式
  /spawn <角色> <任务>  — 手动创建子 Agent
  /list                 — 列出所有子 Agent 及其状态
  /collect [ids]        — 收集子 Agent 结果 (ids 可选，逗号分隔)
  /cancel <id>          — 取消指定子 Agent (级联取消所有子孙)
  /eval <id> <msg>      — 向运行中的子 Agent 发送消息
  /skills               — 列出已注册的技能
  /roles                — 列出可用的角色类型
  /provider [type]      — 显示/切换 LLM Provider (openai_compatible, anthropic)
  /model [name]         — 显示/切换当前模型
  /cost                 — 显示 Token 用量统计与估算成本
  /resume [id]          — 恢复最近或指定会话（保留上下文）
  /fork <id> [turn]     — 基于历史会话的指定轮次分叉新会话
  /status               — 显示系统状态（含对话统计）
  /new [title]          — 创建新会话
  /sessions             — 列出所有对话历史
  /session [id]         — 切换会话 (不带 id 则列出)
  /session_rename <id> <name> — 重命名会话
  /session_delete <id>  — 删除会话
  /search <关键词>       — 搜索历史消息
  /export [session_id] [format] — 导出会话 (format: text|json)
  /task <name> [描述]   — 提交持久化后台任务
  /task_list [status]   — 列出任务 (可选按状态筛选)
  /task_cancel <id>     — 取消任务
  /task_pause <id>      — 暂停运行中的任务
  /task_resume <id>     — 恢复暂停的任务
  /task_recover         — 恢复中断的任务
  /help                 — 显示此帮助
  /quit                 — 退出程序

角色类型:
  general     — 通用助手 (可 spawn 子 Agent)
  explore     — 代码探索专家 (只读工具)
  plan        — 任务规划专家 (只读 + write)
  implementer — 代码实现专家 (读写 + 命令)
  review      — 代码审查专家 (只读工具)
  verifier    — 测试验证专家 (只读 + 命令)

持久化任务类型:
  explore     — 探索代码库并生成结构化报告
  batch_run   — 批量执行命令

示例:
  /task explore 探索 src 目录  metadata:{"directory":"src"}
  /task batch_run 安装依赖    metadata:{"commands":["pip install -r requirements.txt","python setup.py develop"]}
  /task_list running
  /task_cancel a1b2c3d4
"""


class CLI:
    def __init__(self):
        self.llm: ChatOpenAI | None = None
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
                       provider: str = "openai_compatible") -> None:
        data = {
            "provider": provider,
            "base_url": base_url,
            "api_key": api_key,
            "model": model,
            "max_concurrent": max_concurrent,
            "max_depth": max_depth,
        }
        try:
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
            print("请配置 LLM 连接参数:\n")
            self.provider_config = self._prompt_provider_selection()

            max_concurrent = input("  最大并发子 Agent 数 (默认 10): ").strip()
            try:
                max_concurrent = int(max_concurrent) if max_concurrent else 10
            except ValueError:
                max_concurrent = 10
            max_concurrent = min(max_concurrent, 20)

            max_depth = input("  最大嵌套深度 (默认 3): ").strip()
            try:
                max_depth = int(max_depth) if max_depth else 3
            except ValueError:
                max_depth = 3

        print(f"\n  正在初始化...")
        self.llm = create_llm(self.provider_config)
        self.manager = SubAgentManager(
            max_concurrent=max_concurrent,
            max_spawn_depth=max_depth,
            state_file="subagents.json",
        )

        self.conversations = ConversationManager(
            db_path="conversations.db",
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

        self.task_manager = DurableTaskManager(
            db_path="tasks.db",
            max_workers=4,
        )
        self._init_tasks()
        self.task_manager.start()
        recovered = await self.task_manager.recover()
        if recovered:
            print(f"  🔄 恢复 {len(recovered)} 个中断的任务")

        self._init_skills()
        self.token_tracker = TokenTracker(model=self.provider_config.model)
        self.approval_system = ToolApprovalSystem()
        self.approval_system.auto_configure()
        self.approval_system.set_mode(PermissionMode.DEFAULT)
        self._save_settings(
            self.provider_config.base_url,
            self.provider_config.api_key,
            self.provider_config.model,
            max_concurrent,
            max_depth,
            provider=self.provider_config.provider_type.value,
        )
        print(f"  ✅ 初始化完成 | {get_provider_display(self.provider_config)} | 并发上限: {max_concurrent} | 深度上限: {max_depth}\n")

        checkpoint = self.conversations.get_last_checkpoint()
        if checkpoint:
            print(f"  📌 发现上次退出时的会话断点:")
            print(f"     会话: {checkpoint['title']}")
            print(f"     消息: {checkpoint['message_count']} 条 | Token: {checkpoint['token_count']}")
            print(f"     输入 /resume 恢复，或直接开始新对话")

    def _init_tasks(self) -> None:
        """注册内置持久化任务类型"""

        async def explore_codebase(ctx: TaskContext) -> str:
            from my_tui.tools.tools import BUILTIN_TOOLS
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
            from my_tui.tools.tools import BUILTIN_TOOLS
            execute = BUILTIN_TOOLS["execute_command"]

            results = []
            for i, cmd in enumerate(cmds):
                if ctx.cancel_token.is_set():
                    return f"任务被取消，已完成 {i}/{len(cmds)}"
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
        skills_dir = Path("skills")
        loaded = self.skill_registry.load_skills_from_directory(skills_dir)
        if loaded:
            print(f"  📦 从 skills/ 目录加载了 {len(loaded)} 个技能: {', '.join(s.name for s in loaded)}")
        else:
            from my_tui.tools.tools import BUILTIN_TOOLS as bt

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

        print("输入 /help 查看命令，/quit 退出\n")
        print("当前为 [Agent 模式]，直接输入内容即与 AI 对话\n")

        while self.running:
            try:
                prefix = "🤖 > " if self.agent_mode else "🔧 > "
                user_input = await asyncio.get_event_loop().run_in_executor(
                    None, lambda: input(prefix).strip()
                )
            except (EOFError, KeyboardInterrupt):
                print("\n正在退出...")
                break

            if not user_input:
                continue

            if user_input.startswith("/"):
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
                print(f"已取消 Agent {args.strip()}")
            case "/eval":
                eval_parts = args.split(maxsplit=1)
                if len(eval_parts) < 2:
                    print("用法: /eval <agent_id> <消息>")
                    return
                result = await self.manager.send_message(eval_parts[0], eval_parts[1])
                print(result)
            case "/skills":
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
                    print("直接输入内容即可与 AI 对话，输入 / 开头为命令")
                else:
                    print("提示符已变为 🔧，直接输入内容可与 AI 对话，输入 / 开头为命令")
            case "/plan":
                self.approval_system.set_mode(PermissionMode.PLAN)
                print("已切换到 [Plan 模式] 🔍")
                print("  只读调查模式 — 读操作自动放行，写操作/Shell 全部阻止")
            case "/agent":
                self.approval_system.set_mode(PermissionMode.DEFAULT)
                print("已切换到 [Agent 模式] 🤖")
                print("  默认交互模式 — 每次操作都会询问确认")
            case "/yolo":
                self.approval_system.set_mode(PermissionMode.YOLO)
                print("已切换到 [YOLO 模式] ⚡")
                print("  全部自动批准（安全守卫仍生效）— 谨慎操作！")
            case "/sessions":
                self._list_sessions()
            case "/session":
                self._switch_session(args)
            case "/new":
                await self._new_session(args)
            case "/session_rename":
                rename_parts = args.split(maxsplit=1)
                if len(rename_parts) < 2:
                    print("用法: /session_rename <session_id> <新名称>")
                    return
                self.conversations.update_session_title(rename_parts[0], rename_parts[1])
                print(f"会话 {rename_parts[0]} 已重命名为: {rename_parts[1]}")
            case "/session_delete":
                if not args:
                    print("用法: /session_delete <session_id>")
                    return
                self.conversations.delete_session(args.strip())
                print(f"已删除会话 {args.strip()}")
            case "/export":
                export_parts = args.split(maxsplit=1)
                fmt = export_parts[1] if len(export_parts) > 1 else "text"
                sid = export_parts[0] if export_parts else ""
                if not sid:
                    sid = self.conversations.current_session_id
                if not sid:
                    print("没有当前会话")
                    return
                result = self.conversations.export_session(sid, format=fmt)
                if fmt == "json":
                    filepath = f"session_{sid[:8]}.json"
                    Path(filepath).write_text(result, encoding="utf-8")
                    print(f"已导出到 {filepath}")
                else:
                    print(result[:2000])
                    if len(result) > 2000:
                        print(f"... (共 {len(result)} 字符)")
            case "/search":
                if not args:
                    print("用法: /search <关键词>")
                    return
                results = self.conversations.search_messages(args.strip())
                if not results:
                    print("未找到匹配消息")
                    return
                print(f"\n找到 {len(results)} 条匹配消息:")
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
                    print(f"已恢复 {len(recovered)} 个任务:")
                    for r in recovered:
                        print(f"  {r.task_id} — {r.name}: {r.description[:50]}")
                else:
                    print("没有需要恢复的任务")
            case "/quit":
                self.running = False
                print("再见! �")
            case _:
                print(f"未知命令: {cmd}，输入 /help 查看帮助")

    async def _do_chat(self, message: str) -> None:
        """主 Agent 对话: 流式输出 + 对话管理 + 可自动 spawn 子 Agent"""
        tools = get_tools_by_names(
            ROLE_REGISTRY[RoleType.GENERAL].allowed_tools,
        )

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
            depth=0,
        )
        subagent_tools = _build_subagent_tools(ctx)
        all_tools = tools + subagent_tools

        skills = [f"{s.name} — {s.description}" for s in self.skill_registry.list_all()]
        system_prompt = prompt_engine.render_main_system(
            "general", skills=skills, cwd=str(Path.cwd()),
            model=self.provider_config.model,
            provider=PROVIDER_DISPLAY_NAMES.get(
                self.provider_config.provider_type,
                self.provider_config.provider_type.value,
            ),
        )

        user_msg = HumanMessage(content=message)
        await self.conversations.add_message(user_msg)

        await self.conversations.compress_context()

        llm_with_tools = self.llm.bind_tools(all_tools)

        print(f"\n🤖 主 Agent 正在处理: {message[:80]}...\n")

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
                print(f"\n❌ LLM 调用失败: {e}")
                return

            print()

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
                print()
                await self.conversations.add_message(response)
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

                print(f"  🔧 调用工具: {tc_name}")

                if self.approval_system is not None:
                    approval_result = await self.approval_system.evaluate_tool_call(
                        name=tc_name,
                        arguments=tc_args,
                        command=str(tc_args.get("command", tc_args.get("filepath", ""))),
                        target_path=str(tc_args.get("filepath", tc_args.get("directory", ""))),
                    )
                    if approval_result.decision == Decision.BLOCK:
                        msg = f"工具 '{tc_name}' 被审批系统拒绝: {approval_result.message}"
                        print(f"    ⛔ {msg}")
                        tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                        await self.conversations.add_message(tool_msg)
                        continue
                    elif approval_result.decision == Decision.ASK:
                        detail = _format_tool_detail(tc_name, tc_args)
                        print(f"    ❓ {approval_result.message}")
                        if detail:
                            print(detail)
                        confirm = await asyncio.get_event_loop().run_in_executor(
                            None, lambda: input("    确认执行? [y/N]: ").strip().lower()
                        )
                        if confirm not in ("y", "yes"):
                            msg = f"用户拒绝工具 '{tc_name}'"
                            print(f"    ⛔ {msg}")
                            tool_msg = ToolMessage(content=msg, tool_call_id=tc_id)
                            await self.conversations.add_message(tool_msg)
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
                if len(result_str) > 500:
                    preview = result_str[:500]
                    print(f"    结果: {preview}... (共 {len(result_str)} 字符)")
                else:
                    print(f"    结果: {result_str}")

                tool_msg = ToolMessage(content=result_str, tool_call_id=tc_id)
                await self.conversations.add_message(tool_msg)

        print(f"\n⚠️ 达到最大轮次 ({MAX_AGENT_TURNS})")

    async def _do_spawn(self, role_str: str, task: str) -> None:
        try:
            role_type = RoleType(role_str.lower())
        except ValueError:
            valid = [r.value for r in RoleType]
            print(f"无效的角色 '{role_str}'，可选: {valid}")
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
            tools = tools + subagent_tools

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

        print(f"✅ 子 Agent 已启动: {agent.name} [{agent.agent_id}]")
        print(f"   角色: {role_type.value} | 任务: {task[:80]}")

    async def _resume_session(self, args: str) -> None:
        if not args:
            checkpoint = self.conversations.get_last_checkpoint()
            if checkpoint:
                sid = checkpoint["session_id"]
                print(f"  发现上次断点:")
                print(f"    会话: {checkpoint['title']}")
                print(f"    消息数: {checkpoint['message_count']}")
                print(f"    Token: {checkpoint['token_count']}")
                resp = await asyncio.get_event_loop().run_in_executor(
                    None, lambda: input("  恢复该会话? [Y/n]: ").strip().lower()
                )
                if resp in ("", "y", "yes"):
                    self.conversations.load_session(sid)
                    self.conversations.clear_checkpoint()
                    info = self.conversations.get_session_info(sid)
                    print(f"  ✅ 已恢复会话: {info.title} ({info.message_count} 条消息, {info.token_count} tokens)")
                    return
                print("  已跳过恢复")
            else:
                print("  没有可恢复的断点")
            return

        sid = args.strip()
        if self.conversations.load_session(sid):
            info = self.conversations.get_session_info(sid)
            if info:
                self.conversations.clear_checkpoint()
                print(f"  ✅ 已恢复会话: {info.title} ({info.message_count} 条消息, {info.token_count} tokens)")
            else:
                print(f"  ✅ 已恢复会话 {sid[:8]}")
        else:
            print(f"  会话不存在: {sid}")
            sessions = self.conversations.list_sessions(limit=5)
            if sessions:
                print("  最近的会话:")
                for s in sessions:
                    print(f"    {s.session_id[:8]} — {s.title} ({s.message_count} 条消息)")

    async def _fork_session(self, args: str) -> None:
        parts = args.split(maxsplit=1)
        if not parts:
            print("用法: /fork <session_id> [turn_number]")
            print("  turn_number: 分叉到指定轮次（1 开始），不指定则复制全部")
            return

        sid = parts[0].strip()
        turn_number = None
        if len(parts) > 1:
            try:
                turn_number = int(parts[1].strip())
                if turn_number < 1:
                    print("  turn_number 必须 >= 1")
                    return
            except ValueError:
                print(f"  turn_number 必须是数字: {parts[1]}")
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
        print(f"  ✅ 已分叉新会话: {new_id[:8]} — {title}")
        print(f"    消息数: {msg_count}")
        print(f"    源会话: {sid[:8]} (轮次: {turn_number or '全部'})")

    async def _switch_provider(self, args: str) -> None:
        if not args:
            print(f"\n  当前 Provider: {get_provider_display(self.provider_config)}")
            print(f"  Base URL: {self.provider_config.base_url}")
            print(f"\n  可用 Provider:")
            for p in get_available_providers():
                print(f"    {p['key']:20s} — {p['display']}")
            print(f"\n  用法: /provider <类型>")
            return

        provider_type = parse_provider(args.strip())
        if provider_type is None:
            print(f"  无效的 Provider: {args}")
            print(f"  可选: openai_compatible, anthropic")
            return

        if provider_type == self.provider_config.provider_type:
            print(f"  当前已经是 {PROVIDER_DISPLAY_NAMES[provider_type]}")
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
        print(f"  ✅ 已切换到: {get_provider_display(self.provider_config)}\n")

    def _switch_model(self, args: str) -> None:
        if not args:
            print(f"\n  当前模型: {self.provider_config.model}")
            print(f"  用法: /model <模型名称>")
            return

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
        print(f"  ✅ 模型已切换为: {model}\n")

    def _list_skills(self) -> None:
        skills = self.skill_registry.list_all()
        if not skills:
            print("没有注册的技能")
            return
        print("\n已注册的技能:")
        for s in skills:
            print(f"  📦 {s.name}")
        print()

    def _list_roles(self) -> None:
        print("\n可用角色类型:")
        for role_def in list_roles():
            spawn = "✅ 可 spawn" if role_def.can_spawn else "❌ 不可 spawn"
            tools_str = ", ".join(role_def.allowed_tools[:5])
            if len(role_def.allowed_tools) > 5:
                tools_str += f" ... 等{len(role_def.allowed_tools)}个"
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
            print(f"  {s.session_id[:8]} | {s.title[:30]:30s} | {s.message_count:3d} 条消息 | {created}{marker}")
        print()

    def _switch_session(self, session_id: str) -> None:
        session_id = session_id.strip("[]")
        if not session_id:
            sessions = self.conversations.list_sessions()
            if sessions:
                for s in sessions:
                    print(f"  {s.session_id[:8]} — {s.title} ({s.message_count} 条消息)")
            return

        if self.conversations.load_session(session_id):
            self.conversations.clear_checkpoint()
            info = self.conversations.list_sessions()
            matched = [s for s in info if s.session_id == session_id]
            if matched:
                s = matched[0]
                print(f"已切换到会话: {s.title} ({s.message_count} 条消息, {s.token_count} tokens)")
        else:
            print(f"会话不存在: {session_id}")

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
            print("用法: /task <任务名> [描述] [metadata:{...}]")
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
            print(f"❌ {err}")
        else:
            print(f"✅ 任务已提交: {task_id}")
            print(f"   名称: {task_name} | 描述: {description[:60]}")

    def _list_tasks(self, status_filter: str) -> None:
        if status_filter:
            try:
                st = TaskStatus(status_filter)
                tasks = self.task_manager.list_tasks(status=st.value)
            except ValueError:
                print(f"无效的状态: {status_filter}，可选: pending, running, completed, failed, cancelled, paused")
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

        print(f"\n系统状态:")
        mode_label = {
            PermissionMode.DEFAULT: "Agent",
            PermissionMode.PLAN: "Plan 🔍",
            PermissionMode.YOLO: "YOLO ⚡",
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
        print(f"  子 Agent 总数: {len(agents)}")
        print(f"    运行中: {running}")
        print(f"    已完成: {completed}")
        print(f"    失败: {failed}")
        print(f"    已取消: {cancelled}")
        print()

        if self.conversations:
            token_info = self.conversations.get_token_usage()
            current = self.conversations.current_session_id or "(无)"
            print(f"  对话系统:")
            print(f"    当前会话: {current[:8]}")
            print(f"    总会话数: {len(self.conversations.list_sessions(limit=9999))}")
            print(f"    当前消息数: {token_info['message_count']}")
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
            print(f"  断点: {checkpoint['title']} ({checkpoint['message_count']} 条消息)")
            print()

        if self.task_manager:
            running = self.task_manager.running_count
            pending = self.task_manager.pending_count
            total = len(self.task_manager.list_tasks(limit=9999))
            print(f"  后台任务管理器:")
            print(f"    Workers: {self.task_manager.worker_count}")
            print(f"    运行中: {running} | 排队中: {pending} | 总计: {total}")
            print()

    async def _cleanup(self) -> None:
        self.cancel_token.cancel()
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
        lines.append(f"    改哪里: {target}")
    if name in ("write_file", "edit_file", "write") and args.get("content"):
        content = args["content"]
        preview = content[:200].replace("\n", "\\n")
        lines.append(f"    改什么: {preview}")
    elif name in ("edit_file",) and args.get("new_str"):
        preview = args["new_str"][:200].replace("\n", "\\n")
        lines.append(f"    改什么: {preview}")
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


def main():
    cli = CLI()

    def sig_handler(signum, frame):
        cli.running = False
        cli.cancel_token.cancel()

    signal.signal(signal.SIGINT, sig_handler)
    signal.signal(signal.SIGTERM, sig_handler)

    try:
        asyncio.run(cli.run())
    except KeyboardInterrupt:
        print("\n已中断")
    except Exception as e:
        print(f"\n❌ 错误: {e}")
        traceback.print_exc()


if __name__ == "__main__":
    main()