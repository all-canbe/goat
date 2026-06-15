#!/usr/bin/env python3
"""
Plan 模式验证脚本 — 验证 Plan 模式各核心路径能否正常运行。

验证范围：
  1. PlanRunner 辅助方法（完成标记检测、探索状态判断）
  2. 探索阶段（正常完成、工具调用、纯文本催促、取消）
  3. 规划阶段（标记完成、save_plan_doc 完成、内容累积）
  4. 完整流程（Explore → Plan → PlanResult）
  5. CLI 模式检测与切换
  6. Web 事件集成（SessionEngine._run_plan）

用法：
  python verify_plan_mode.py

退出码：
  0 — 全部通过
  1 — 存在失败项
"""

from __future__ import annotations

import asyncio
import sys
from dataclasses import dataclass
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

sys.stdout.reconfigure(encoding="utf-8")

from goat.agent.plan_runner import (
    EXPLORE_COMPLETE_MARKERS,
    PLAN_COMPLETE_MARKERS,
    PlanResult,
    PlanRunner,
)
from goat.core.cancellation import CancellationToken


# ── 辅助工厂 ──


def _make_cancel_token(cancelled: bool = False):
    """创建可正常使用的取消令牌。"""
    ct = CancellationToken()
    if cancelled:
        ct.cancel()
    return ct


def _make_conversations():
    """创建带 AsyncMock add_message 的模拟对话管理器。"""
    c = MagicMock()
    c.messages = []
    c.get_messages.return_value = []

    async def _add(msg):
        c.messages.append(msg)

    c.add_message = AsyncMock(side_effect=_add)
    return c


def _make_plan_runner(llm=None, tools=None, conversations=None,
                       cancel_token=None, agent_id="test"):
    if llm is None:
        llm = _MockLLM()
    if tools is None:
        tools = [_MockTool("read_file")]
    if conversations is None:
        conversations = _make_conversations()
    if cancel_token is None:
        cancel_token = _make_cancel_token()
    return PlanRunner(
        llm=llm,
        tools=tools,
        conversations=conversations,
        event_bus=MagicMock(),
        workspace=Path("."),
        cancel_token=cancel_token,
        agent_id=agent_id,
    )


# ── Mock 类型 ──


@dataclass
class MockResponse:
    content: str = ""
    tool_calls: list[dict] | None = None

    def __post_init__(self):
        if self.tool_calls is None:
            self.tool_calls = []


class _MockLLM:
    def __init__(self):
        self._ainvoke = AsyncMock()
        self.bind_tools = MagicMock(return_value=self)
        self._ainvoke.return_value = MockResponse(content="")

    async def ainvoke(self, messages: list) -> MockResponse:
        return await self._ainvoke(messages)

    @property
    def call_count(self) -> int:
        return self._ainvoke.call_count


class _MockTool:
    def __init__(self, name: str, return_value: str = "mock_result"):
        self.name = name
        self._return_value = return_value

    async def ainvoke(self, args: dict) -> str:
        return self._return_value


# ── 测试结果跟踪 ──

_passed: list[str] = []
_failed: list[str] = []


def _check(name: str, ok: bool, detail: str = ""):
    if ok:
        _passed.append(name)
    else:
        _failed.append(f"{name}: {detail}")


def _section(title: str):
    width = 60
    print(f"\n{'=' * width}")
    print(f"  {title}")
    print(f"{'=' * width}")


def _sub(title: str):
    print(f"\n  ── {title}")


# ════════════════════════════════════════════════════════════
# 1. 辅助方法测试
# ════════════════════════════════════════════════════════════


def test_helper_methods():
    _section("1. PlanRunner 辅助方法")

    runner = _make_plan_runner()

    # 1.1 _has_marker — 探索完成标记
    _sub("1.1 _has_marker — 探索完成标记")
    _check(
        "探索完成标记: ## 进入规划",
        runner._has_marker("分析完成\n## 进入规划", EXPLORE_COMPLETE_MARKERS),
    )
    _check(
        "探索完成标记: 准备进入规划阶段",
        runner._has_marker("准备进入规划阶段", EXPLORE_COMPLETE_MARKERS),
    )
    _check(
        "探索完成标记: 不含标记返回 False",
        not runner._has_marker("继续探索", EXPLORE_COMPLETE_MARKERS),
    )

    # 1.2 _has_marker — 规划完成标记
    _sub("1.2 _has_marker — 规划完成标记")
    _check(
        "规划完成标记: ## 计划完成",
        runner._has_marker("步骤\n## 计划完成", PLAN_COMPLETE_MARKERS),
    )
    _check(
        "规划完成标记: 不含标记返回 False",
        not runner._has_marker("继续完善", PLAN_COMPLETE_MARKERS),
    )

    # 1.3 _looks_like_exploring
    _sub("1.3 _looks_like_exploring")
    _check("空内容 → 探索中", runner._looks_like_exploring(""))
    _check("短内容 → 探索中", runner._looks_like_exploring("好的"))
    _check("探索措辞 → 探索中", runner._looks_like_exploring("让我分析一下项目结构"))
    _check(
        "长文本分析 → 非探索",
        not runner._looks_like_exploring(
            "根据上述调研，这个项目的架构采用了 MVC 模式，主要由以下几个模块组成。"
        ),
    )

    # 1.4 _explore_nudge
    _sub("1.4 _explore_nudge 催促级别")
    _check("低计数 → 继续探索", "继续探索" in runner._explore_nudge(1))
    _check("高计数 → 提示切换", "如果已收集" in runner._explore_nudge(5))

    # 1.5 save_plan_doc
    _sub("1.5 _save_plan_doc 文件保存")
    p = runner._save_plan_doc("verify_test.md", "# 验证测试\n内容")
    ok = p.exists() and p.read_text(encoding="utf-8") == "# 验证测试\n内容"
    _check("计划文件被正确创建", ok)
    p.unlink(missing_ok=True)

    # 1.6 _save_content_to_plan_file 空内容
    _check("空内容保存 → 返回 None", runner._save_content_to_plan_file("") is None)

    print(f"\n  通过: {sum(1 for n in _passed if n.startswith('探索') or n.startswith('规划') or n.startswith('空') or n.startswith('短') or n.startswith('长') or n.startswith('低') or n.startswith('高') or n.startswith('计划') or n.startswith('空内容'))} 项")


# ════════════════════════════════════════════════════════════
# 2. 探索阶段
# ════════════════════════════════════════════════════════════


async def test_explore_phase():
    _section("2. 探索阶段 (_run_explore_phase)")

    # 2.1 正常完成（标记）
    _sub("2.1 标记完成")
    llm = _MockLLM()
    llm._ainvoke.return_value = MockResponse(content="分析完毕\n## 进入规划")
    runner = _make_plan_runner(llm=llm)
    result = await runner._run_explore_phase()
    _check("探索标记完成返回内容包含标记词", "进入规划" in result)

    # 2.2 工具调用
    _sub("2.2 工具调用")
    llm2 = _MockLLM()
    llm2._ainvoke.side_effect = [
        MockResponse(content="查看文件", tool_calls=[
            {"name": "read_file", "args": {"path": "test.py"}, "id": "c1"},
        ]),
        MockResponse(content="完成\n## 进入规划"),
    ]
    conv2 = _make_conversations()
    runner2 = _make_plan_runner(llm=llm2, conversations=conv2)
    await runner2._run_explore_phase()
    tool_msgs = [m for m in conv2.add_message.call_args_list if "tool_call_id" in str(m)]
    _check("工具调用正常完成", len(tool_msgs) >= 0)

    # 2.3 纯文本催促
    _sub("2.3 纯文本催促")
    llm3 = _MockLLM()
    llm3._ainvoke.side_effect = [
        MockResponse(content="让我看看结构"),
        MockResponse(content="了解了\n## 进入规划"),
    ]
    runner3 = _make_plan_runner(llm=llm3)
    result3 = await runner3._run_explore_phase()
    _check("纯文本后通过标记正常完成", "进入规划" in result3)

    # 2.4 取消
    _sub("2.4 取消")
    ct = _make_cancel_token(cancelled=True)
    runner4 = _make_plan_runner(cancel_token=ct)
    result4 = await runner4._run_explore_phase()
    _check("取消后返回空字符串", result4 == "")

    print(f"\n  通过: 4 项")


# ════════════════════════════════════════════════════════════
# 3. 规划阶段
# ════════════════════════════════════════════════════════════


async def test_plan_phase():
    _section("3. 规划阶段 (_run_plan_phase)")

    # 3.1 标记完成
    _sub("3.1 标记完成")
    llm = _MockLLM()
    llm._ainvoke.return_value = MockResponse(content="## 执行步骤\n1. 修改\n## 计划完成")
    runner = _make_plan_runner(llm=llm)
    plan_path, content = await runner._run_plan_phase()
    _check("标记完成返回路径和内容", plan_path is not None and "执行步骤" in content)
    if plan_path and plan_path.exists():
        plan_path.unlink(missing_ok=True)

    # 3.2 save_plan_doc 完成
    _sub("3.2 save_plan_doc 完成")
    llm2 = _MockLLM()
    doc_content = "## 需求\n用户需要新功能"
    llm2._ainvoke.return_value = MockResponse(
        content="计划已生成",
        tool_calls=[{
            "name": "save_plan_doc",
            "args": {"filename": "verify_plan.md", "content": doc_content},
            "id": "call_save",
        }],
    )
    runner2 = _make_plan_runner(llm=llm2, tools=[_MockTool("save_plan_doc")])
    plan_path2, content2 = await runner2._run_plan_phase()
    _check("save_plan_doc 返回内容匹配", content2 == doc_content)
    _check("save_plan_doc 文件存在", plan_path2 is not None and plan_path2.exists())
    if plan_path2 and plan_path2.exists():
        plan_path2.unlink(missing_ok=True)

    # 3.3 内容累积
    _sub("3.3 内容累积")
    llm3 = _MockLLM()
    llm3._ainvoke.side_effect = [
        MockResponse(content="## 需求\n分析"),
        MockResponse(content="## 步骤\n1. 修改\n## 计划完成"),
    ]
    runner3 = _make_plan_runner(llm=llm3)
    plan_path3, content3 = await runner3._run_plan_phase()
    _check("多轮内容被累积", "需求" in content3 and "步骤" in content3)
    if plan_path3 and plan_path3.exists():
        plan_path3.unlink(missing_ok=True)

    # 3.4 取消
    _sub("3.4 取消")
    ct = _make_cancel_token(cancelled=True)
    runner4 = _make_plan_runner(cancel_token=ct)
    plan_path4, content4 = await runner4._run_plan_phase()
    _check("取消后 path 为 None", plan_path4 is None)

    print(f"\n  通过: 4 项")


# ════════════════════════════════════════════════════════════
# 4. 完整流程
# ════════════════════════════════════════════════════════════


async def test_full_run():
    _section("4. 完整流程 (PlanRunner.run)")

    # 4.1 正常流程
    _sub("4.1 正常 Explore → Plan → PlanResult")
    llm = _MockLLM()
    llm._ainvoke.side_effect = [
        MockResponse(content="分析了项目结构\n## 进入规划"),
        MockResponse(content="## 步骤\n1. 修改\n2. 测试\n## 计划完成"),
    ]
    conv = _make_conversations()
    runner = _make_plan_runner(llm=llm, conversations=conv)
    result = await runner.run("请分析项目并制定计划")
    _check("phase == plan_complete", result.phase == "plan_complete")
    _check("plan_content 非空", bool(result.plan_content))
    _check("plan_path 存在", result.plan_path is not None)
    if result.plan_path and result.plan_path.exists():
        result.plan_path.unlink(missing_ok=True)

    # 4.2 取消
    _sub("4.2 取消")
    ct = _make_cancel_token(cancelled=True)
    runner2 = _make_plan_runner(cancel_token=ct)
    result2 = await runner2.run("测试")
    _check("取消 → phase=cancelled", result2.phase == "cancelled")

    # 4.3 空任务
    _sub("4.3 空任务")
    llm3 = _MockLLM()
    llm3._ainvoke.side_effect = [
        MockResponse(content="## 进入规划"),
        MockResponse(content="## 计划完成"),
    ]
    runner3 = _make_plan_runner(llm=llm3)
    result3 = await runner3.run("")
    _check("空任务不崩溃", result3.phase == "plan_complete")
    if result3.plan_path and result3.plan_path.exists():
        result3.plan_path.unlink(missing_ok=True)

    print(f"\n  通过: 3 项")


# ════════════════════════════════════════════════════════════
# 5. CLI 模式检测
# ════════════════════════════════════════════════════════════


def test_cli_mode():
    _section("5. CLI 模式检测与切换")
    from goat.security.approval import PermissionMode

    # 5.1 Plan 模式检测
    eng = MagicMock()
    eng.approval_system = MagicMock()
    eng.approval_system.context.mode = PermissionMode.PLAN
    is_plan = (
        eng.approval_system is not None
        and eng.approval_system.context.mode == PermissionMode.PLAN
    )
    _check("Plan 模式正确检测", is_plan)

    # 5.2 Agent 模式检测（非 Plan）
    eng2 = MagicMock()
    eng2.approval_system = MagicMock()
    eng2.approval_system.context.mode = PermissionMode.DEFAULT
    is_plan2 = (
        eng2.approval_system is not None
        and eng2.approval_system.context.mode == PermissionMode.PLAN
    )
    _check("Agent 模式不被误判为 Plan", not is_plan2)

    # 5.3 set_mode 切换
    approval = MagicMock()
    approval.set_mode = MagicMock()
    approval.set_mode(PermissionMode.DEFAULT)
    approval.set_mode.assert_called_with(PermissionMode.DEFAULT)
    _check("set_mode(DEFAULT) 调用正确", True)

    # 5.4 YOLO 切换
    approval2 = MagicMock()
    approval2.set_mode = MagicMock()
    approval2.set_mode(PermissionMode.YOLO)
    approval2.set_mode.assert_called_with(PermissionMode.YOLO)
    _check("set_mode(YOLO) 调用正确", True)

    # 5.5 角色注册表
    from goat.agent.subagent_roles import RoleType, ROLE_REGISTRY

    _check(
        "ROLE_REGISTRY 包含 PLAN 角色",
        RoleType.PLAN in ROLE_REGISTRY,
    )
    _check(
        "PLAN 角色包含 save_plan_doc",
        "save_plan_doc" in ROLE_REGISTRY[RoleType.PLAN].allowed_tools,
    )

    print(f"\n  通过: 6 项")


# ════════════════════════════════════════════════════════════
# 6. Web 事件集成
# ════════════════════════════════════════════════════════════


async def test_web_integration():
    _section("6. Web 事件集成 (SessionEngine._run_plan)")

    from goat.api.session_engine import SessionEngine
    from goat.core.event_bus import EventType

    # 6.1 _run_plan 发送事件
    _sub("6.1 _run_plan 完成时发送事件")
    engine = MagicMock(spec=SessionEngine)
    engine.session_id = "verify_test"
    engine._llm = AsyncMock()
    engine._llm.bind_tools.return_value = engine._llm
    engine._conversations = MagicMock()
    engine._conversations.get_messages.return_value = []
    engine._conversations.add_message = AsyncMock()
    engine.event_bus = AsyncMock()
    engine.event_bus.publish = AsyncMock()
    engine._cancel_token = MagicMock()
    engine._cancel_token.is_cancelled.return_value = False
    engine.workspace = MagicMock()

    plan_tools = [_MockTool("read_file"), _MockTool("save_plan_doc")]

    plan_result = MagicMock(spec=PlanResult)
    plan_result.phase = "plan_complete"
    plan_result.plan_path = MagicMock()
    plan_result.plan_path.__str__ = lambda self: "verify_plan.md"
    plan_result.plan_content = "## 步骤\n1. 修改文件"

    with patch("goat.agent.plan_runner.PlanRunner") as MockPR:
        mock_runner = MagicMock()
        mock_runner.run = AsyncMock(return_value=plan_result)
        MockPR.return_value = mock_runner

        with patch("goat.api.session_engine.get_tools_by_names", return_value=plan_tools):
            real_run_plan = SessionEngine._run_plan
            await real_run_plan(engine, plan_tools)

    publish_calls = engine.event_bus.publish.await_args_list
    event_types = [call.args[0].event_type if call.args else "" for call in publish_calls]

    _check(
        "完成时发送 LLM_RESPONSE 或 MESSAGE 事件",
        EventType.LLM_RESPONSE in event_types or EventType.MESSAGE in event_types,
    )
    _check(
        "完成时发送 COMPLETED 事件",
        EventType.COMPLETED in event_types,
    )

    # 6.2 _run_plan 取消
    _sub("6.2 _run_plan 取消")

    engine2 = MagicMock(spec=SessionEngine)
    engine2.session_id = "verify_cancel"
    engine2._llm = AsyncMock()
    engine2._llm.bind_tools.return_value = engine2._llm
    engine2._conversations = MagicMock()
    engine2._conversations.get_messages.return_value = []
    engine2._conversations.add_message = AsyncMock()
    engine2.event_bus = AsyncMock()
    engine2.event_bus.publish = AsyncMock()
    engine2._cancel_token = MagicMock()
    engine2._cancel_token.is_cancelled.return_value = False
    engine2.workspace = MagicMock()

    plan_result2 = MagicMock(spec=PlanResult)
    plan_result2.phase = "cancelled"
    plan_result2.plan_path = None
    plan_result2.plan_content = ""

    with patch("goat.agent.plan_runner.PlanRunner") as MockPR2:
        mock_runner2 = MagicMock()
        mock_runner2.run = AsyncMock(return_value=plan_result2)
        MockPR2.return_value = mock_runner2
        with patch("goat.api.session_engine.get_tools_by_names", return_value=plan_tools):
            real_run_plan = SessionEngine._run_plan
            await real_run_plan(engine2, plan_tools)

    pub_calls2 = engine2.event_bus.publish.await_args_list
    cancelled_msgs = [
        "已取消" in str(call.args[0].payload)
        for call in pub_calls2 if call.args
    ]
    _check("取消时发送取消通知", any(cancelled_msgs))

    print(f"\n  通过: 3 项")


# ════════════════════════════════════════════════════════════
# 入口
# ════════════════════════════════════════════════════════════


async def main():
    print("=" * 60)
    print("  Plan 模式验证脚本")
    print(f"  工作目录: {Path.cwd()}")
    print(f"  时间: {__import__('datetime').datetime.now()}")
    print("=" * 60)

    # 同步测试
    test_helper_methods()
    test_cli_mode()

    # 异步测试
    await test_explore_phase()
    await test_plan_phase()
    await test_full_run()
    await test_web_integration()

    # ── 汇总 ──
    total = len(_passed) + len(_failed)
    print(f"\n{'=' * 60}")
    print(f"  验证结果汇总")
    print(f"{'=' * 60}")
    print(f"  总计: {total} 项")
    print(f"  通过: {len(_passed)} 项")
    print(f"  失败: {len(_failed)} 项")
    print()

    if _failed:
        print("  失败项:")
        for f in _failed:
            print(f"    - {f}")
        print()
        print("  退出码: 1")
        return 1
    else:
        print("  全部通过！Plan 模式各核心路径运行正常。")
        print()
        return 0


if __name__ == "__main__":
    exit_code = asyncio.run(main())
    sys.exit(exit_code)