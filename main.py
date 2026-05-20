#!/usr/bin/env python3
"""
SubAgent Demo — LangChain + 子 Agent 并行调度系统 + SQLite 对话管理

用法:
    python main.py

启动后按提示输入 base_url、api_key、model 完成初始化。
然后进入交互命令行，支持以下命令:

    /chat <消息>          — 与主 Agent 对话（主 Agent 可自动 spawn 子 Agent，自动持久化）
    /spawn <角色> <任务>  — 手动创建子 Agent
    /list                 — 列出所有子 Agent
    /collect [ids]        — 收集子 Agent 结果
    /cancel <id>          — 取消子 Agent
    /eval <id> <msg>      — 向子 Agent 发送消息
    /sessions             — 列出对话历史
    /session [id]         — 切换会话
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

from subagent_demo.cancellation import CancellationToken
from subagent_demo.subagent_manager import SubAgentManager, SubAgentStatus
from subagent_demo.subagent_roles import (
    RoleType, ROLE_REGISTRY, get_role, list_roles,
)
from subagent_demo.subagent_runtime import (
    AgentContext, _run_agent_loop, _build_subagent_tools, MAX_AGENT_TURNS,
)
from subagent_demo.skill_system import SkillRegistry, Skill
from subagent_demo.tools import get_tools_by_names, BUILTIN_TOOLS
from subagent_demo.conversation_manager import ConversationManager, SessionInfo
from subagent_demo.event_bus import EventBus, EventType
from subagent_demo.durable_task_manager import (
    DurableTaskManager, TaskDef, TaskContext, TaskType, TaskStatus,
)
from subagent_demo.prompt_engine import engine as prompt_engine


SETTINGS_FILE = Path("setting.json")


BANNER = r"""
╔══════════════════════════════════════════════════════╗
║      🐋 SubAgent Demo — 子 Agent 并行调度系统        ║
║      LangChain + asyncio + SQLite 对话管理           ║
╚══════════════════════════════════════════════════════╝
"""

HELP_TEXT = """
模式说明:
  默认 [Agent 模式] — 直接输入内容即与 AI 对话
  输入 /mode 切换到 [命令模式] — 所有输入均视为命令

可用命令:
  /chat <消息>          — 与主 Agent 对话 (命令模式下使用)
  /mode                 — 切换 Agent/命令模式
  /spawn <角色> <任务>  — 手动创建子 Agent
  /list                 — 列出所有子 Agent 及其状态
  /collect [ids]        — 收集子 Agent 结果 (ids 可选，逗号分隔)
  /cancel <id>          — 取消指定子 Agent (级联取消所有子孙)
  /eval <id> <msg>      — 向运行中的子 Agent 发送消息
  /skills               — 列出已注册的技能
  /roles                — 列出可用的角色类型
  /status               — 显示系统状态（含对话统计）
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
                       max_concurrent: int, max_depth: int) -> None:
        data = {
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

    async def initialize(self) -> None:
        print(BANNER)

        settings = self._load_settings()
        if settings:
            base_url = settings.get("base_url", "https://api.openai.com/v1")
            api_key = settings["api_key"]
            model = settings.get("model", "gpt-4o")
            max_concurrent = min(settings.get("max_concurrent", 10), 20)
            max_depth = settings.get("max_depth", 3)
            print(f"  读取已保存的配置: {model} | {base_url}\n")
        else:
            print("请配置 LLM 连接参数:\n")

            base_url = input("  Base URL (默认 https://api.openai.com/v1): ").strip()
            if not base_url:
                base_url = "https://api.openai.com/v1"

            api_key = input("  API Key: ").strip()
            while not api_key:
                print("  API Key 不能为空")
                api_key = input("  API Key: ").strip()

            model = input("  模型名称 (默认 gpt-4o): ").strip()
            if not model:
                model = "gpt-4o"

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
        self.llm = ChatOpenAI(
            base_url=base_url,
            api_key=api_key,
            model=model,
            temperature=0.7,
        )
        self.manager = SubAgentManager(
            max_concurrent=max_concurrent,
            max_spawn_depth=max_depth,
            state_file="subagents.json",
        )

        self.conversations = ConversationManager(
            db_path="conversations.db",
            max_tokens=128000,
        )
        await self.conversations.create_session(model=model, title="默认会话")

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
        self._save_settings(base_url, api_key, model, max_concurrent, max_depth)
        print(f"  ✅ 初始化完成 | 模型: {model} | 并发上限: {max_concurrent} | 深度上限: {max_depth}\n")

    def _init_tasks(self) -> None:
        """注册内置持久化任务类型"""

        async def explore_codebase(ctx: TaskContext) -> str:
            from subagent_demo.tools import BUILTIN_TOOLS
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
            from subagent_demo.tools import BUILTIN_TOOLS
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
        from subagent_demo.tools import BUILTIN_TOOLS as bt

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
            elif self.agent_mode:
                await self._do_chat(user_input)
            else:
                print("请输入命令（以 / 开头），或输入 /mode 切换到 Agent 模式")

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
            case "/chat":
                if not args:
                    print("用法: /chat <消息>")
                    return
                await self._do_chat(args)
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
                    print("所有输入均视为命令，输入 /mode 切换回 Agent 模式")
            case "/sessions":
                self._list_sessions()
            case "/session":
                self._switch_session(args)
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
                print("再见! 🐋")
            case _:
                print(f"未知命令: {cmd}，输入 /help 查看帮助")

    async def _do_chat(self, message: str) -> None:
        """主 Agent 对话: 流式输出 + 对话管理 + 可自动 spawn 子 Agent"""
        tools = get_tools_by_names(
            ROLE_REGISTRY[RoleType.GENERAL].allowed_tools,
        )

        ctx = AgentContext(
            llm=self.llm,
            manager=self.manager,
            skill_registry=self.skill_registry,
            event_bus=self.event_bus,
            agent_id=self.main_agent_id,
            spawn_depth=0,
            cancel_token=self.cancel_token,
        )
        subagent_tools = _build_subagent_tools(ctx)
        all_tools = tools + subagent_tools

        skills = [f"{s.name} — {s.description}" for s in self.skill_registry.list_all()]
        system_prompt = prompt_engine.render_main_system(
            "general", skills=skills, cwd=str(Path.cwd()),
        )

        user_msg = HumanMessage(content=message)
        await self.conversations.add_message(user_msg)

        llm_with_tools = self.llm.bind_tools(all_tools)

        print(f"\n🤖 主 Agent 正在处理: {message[:80]}...\n")

        for turn in range(MAX_AGENT_TURNS):
            if self.cancel_token.is_cancelled():
                print("⚠️ 任务被取消")
                return

            messages = [
                SystemMessage(content=system_prompt),
                *self.conversations.get_context(),
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
            llm=self.llm,
            manager=self.manager,
            skill_registry=self.skill_registry,
            event_bus=self.event_bus,
            agent_id=self.main_agent_id,
            role_def=role_def,
            spawn_depth=0,
            cancel_token=self.cancel_token,
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
                ctx=AgentContext(
                    llm=self.llm,
                    manager=self.manager,
                    skill_registry=self.skill_registry,
                    event_bus=self.event_bus,
                    agent_id=agent.agent_id,
                    role_def=role_def,
                    spawn_depth=1,
                    parent_id=self.main_agent_id,
                    cancel_token=agent.cancel_token,
                ),
                agent=agent,
                tools=tools,
                task=task,
            )
        )

        print(f"✅ 子 Agent 已启动: {agent.name} [{agent.agent_id}]")
        print(f"   角色: {role_type.value} | 任务: {task[:80]}")

    def _list_skills(self) -> None:
        skills = self.skill_registry.list_all()
        if not skills:
            print("没有注册的技能")
            return
        print("\n已注册的技能:")
        for s in skills:
            role = s.metadata.get("role", "N/A")
            print(f"  📦 {s.name} — {s.description} (对应角色: {role})")
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
        if not session_id:
            sessions = self.conversations.list_sessions()
            if sessions:
                for s in sessions:
                    print(f"  {s.session_id[:8]} — {s.title} ({s.message_count} 条消息)")
            return

        if self.conversations.load_session(session_id):
            info = self.conversations.list_sessions()
            matched = [s for s in info if s.session_id == session_id]
            if matched:
                s = matched[0]
                print(f"已切换到会话: {s.title} ({s.message_count} 条消息, {s.token_count} tokens)")
        else:
            print(f"会话不存在: {session_id}")

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

    def _show_status(self) -> None:
        agents = self.manager.list_agents()
        running = sum(1 for a in agents if a.status == SubAgentStatus.RUNNING)
        completed = sum(1 for a in agents if a.status == SubAgentStatus.COMPLETED)
        failed = sum(1 for a in agents if a.status == SubAgentStatus.FAILED)
        cancelled = sum(1 for a in agents if a.status == SubAgentStatus.CANCELLED)

        print(f"\n系统状态:")
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
            self.conversations.close()


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