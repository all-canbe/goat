"""Plan 模式专用执行器 — 独立于 Agent 循环的两个阶段：Explore → Plan。

Phase 1 (Explore): 只读探索代码库，LLM 自然输出文本分析，完成后显式标记 "## 进入规划"
Phase 2 (Plan):   生成执行计划，完成后调用 save_plan_doc 或输出 "## 计划完成"
Phase 3 (Review): 由调用方（CLI / Web）处理用户审批，不在本模块内完成

与 Agent 模式的关键差异：
  - 无 "纯文本=任务完成" 约束 — Plan 模式下文本输出是正常行为
  - 完成判定显式化 — 依赖标记词或工具调用，不依赖启发式猜测
  - CLI/Web 通用 — PlanRunner 只管理 LLM 循环，I/O 由外部回调处理
"""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Callable, Optional

from langchain_core.messages import HumanMessage, SystemMessage, ToolMessage
from langchain_core.language_models.chat_models import BaseChatModel

from ..core.cancellation import CancellationToken
from ..core.event_bus import EventBus, EventType, SubagentEvent

logger = logging.getLogger(__name__)


# ── 阶段完成标记 ──

EXPLORE_COMPLETE_MARKERS = (
    "## 进入规划", "## 开始规划", "## 规划阶段",
    "## 开始制定计划", "准备进入规划阶段",
)

PLAN_COMPLETE_MARKERS = (
    "## 计划完成", "## 规划完成",
)


# ── 阶段系统提示词 ──

_EXPLORE_SYSTEM_PROMPT = """\
## Plan 模式 — 探索阶段

你正在以【只读规划模式】探索代码库。你的任务是深入理解用户需求并收集足够信息来制定计划。

行为规则：
- 每个回复中**至少调用一个工具**（read_file / search_code / list_files）
- 可以自由输出文本进行分析和总结 — 文本输出是正常的探索行为，不会导致退出
- 当你的分析产生阶段性结论时，可以用文字表达，但**继续调用工具深入探索**
- 收集到的信息足以支撑完整计划时，在回复末尾输出 **## 进入规划** 来切换阶段
- 你也可以在分析过程中调用 save_plan_doc 保存探索笔记到 .goat/doc/

不要急于进入规划阶段 — 确保理解了：
- 项目架构和模块关系
- 涉及的关键文件和数据流
- 用户需求的完整范围"""

_PLAN_SYSTEM_PROMPT = """\
## Plan 模式 — 规划阶段

你现在进入【规划阶段】。基于探索阶段的结果，请制定一份完整的执行计划。

计划文档结构：
## 需求理解
  - 用户目标是什么
  - 涉及的功能范围
## 涉及模块
  - 列出需要修改的文件及原因
## 执行步骤
  - 每一步：操作描述、涉及文件、预期结果
## 风险与注意事项
## 验证方案

行为规则：
- 可以自由输出文本草案和完善计划 — 文本输出不会导致退出
- 可以调用 read_file / search_code 补充确认细节
- 计划完善后，**调用 save_plan_doc 工具**保存到 .goat/doc/
- 或输出 **## 计划完成** 标记来结束规划"""


# ── 结果类型 ──

@dataclass
class PlanResult:
    """Plan 模式执行结果，供 Phase 3（用户审批）使用。"""
    plan_path: Optional[Path] = None
    plan_content: str = ""
    phase: str = ""  # "explore_complete" | "plan_complete" | "cancelled" | "error"


# ── PlanRunner ──

class PlanRunner:
    """Plan 模式独立循环执行器。

    用法:
        runner = PlanRunner(llm, tools, conversations, event_bus, workspace, cancel_token)
        result = await runner.run(task)
        # result.plan_path 和 result.plan_content 供 Phase 3 使用
    """

    MAX_EXPLORE_TURNS = 20
    MAX_PLAN_TURNS = 10
    MAX_CONTINUATION = 6  # 连续纯文本最大容忍次数

    # ── 构造 ──

    def __init__(
        self,
        llm: BaseChatModel,
        tools: list,
        conversations: Any,
        event_bus: EventBus,
        workspace: Path,
        cancel_token: CancellationToken,
        agent_id: str = "main",
    ) -> None:
        self.llm = llm
        self.tools = tools
        self.conversations = conversations
        self.event_bus = event_bus
        self.workspace = workspace
        self.cancel_token = cancel_token
        self.agent_id = agent_id

    # ── 主入口 ──

    async def run(self, task: str) -> PlanResult:
        """运行完整 Plan 生命周期 (Explore + Plan)，返回 PlanResult。"""
        # Phase 1: Explore
        await self.conversations.add_message(HumanMessage(content=task))
        explore_content = await self._run_explore_phase()

        if self.cancel_token.is_cancelled():
            return PlanResult(phase="cancelled")

        # Phase 2: Plan
        plan_path, plan_content = await self._run_plan_phase()

        if self.cancel_token.is_cancelled():
            return PlanResult(
                plan_content=plan_content or explore_content,
                phase="cancelled",
                plan_path=plan_path,
            )

        return PlanResult(
            plan_path=plan_path,
            plan_content=plan_content,
            phase="plan_complete",
        )

    # ── Phase 1: 探索 ──

    async def _run_explore_phase(self) -> str:
        """探索阶段循环。返回探索过程中累积的文本内容。"""
        await self._notify("system", "🔍 Plan 模式 — 开始探索代码库")

        system_prompt = _EXPLORE_SYSTEM_PROMPT
        llm_with_tools = self.llm.bind_tools(self.tools)
        continuation_count = 0

        for turn in range(self.MAX_EXPLORE_TURNS):
            if self.cancel_token.is_cancelled():
                return ""

            messages = [SystemMessage(content=system_prompt)]
            messages.extend(self.conversations.get_messages())

            response = await llm_with_tools.ainvoke(messages)
            content = response.content or ""

            await self._stream_content(content)

            # 检查阶段完成标记
            if self._has_marker(content, EXPLORE_COMPLETE_MARKERS):
                await self.conversations.add_message(response)
                await self._notify("system", "📋 探索完成，进入规划阶段")
                return content

            tool_calls = getattr(response, "tool_calls", [])

            if not tool_calls:
                content = response.content or ""
                continuation_count += 1

                # 短文本 + 探索性措辞 → 不是完成信号，继续催促
                if self._looks_like_exploring(content):
                    await self.conversations.add_message(response)
                    await self.conversations.add_message(HumanMessage(
                        content=self._explore_nudge(continuation_count),
                    ), extra_metadata={"hint": True})
                    await self._notify("system",
                        f"探索中… 自动继续（{continuation_count}/{self.MAX_CONTINUATION}）")
                    continue

                # 长文本无完成标记 → 可能是分析输出，继续
                if len(content.strip()) > 80:
                    await self.conversations.add_message(response)
                    await self.conversations.add_message(HumanMessage(
                        content="请继续探索相关代码，或输出 ## 进入规划 切换到规划阶段。",
                    ), extra_metadata={"hint": True})
                    continue

                # 短文本无标记 → 催促
                if continuation_count <= self.MAX_CONTINUATION:
                    await self.conversations.add_message(response)
                    await self.conversations.add_message(HumanMessage(
                        content=self._explore_nudge(continuation_count),
                    ), extra_metadata={"hint": True})
                    continue

            continuation_count = 0
            await self.conversations.add_message(response)

            # 处理工具调用
            await self._process_tool_calls(response, tool_calls, llm_with_tools, system_prompt)

        # 超出最大轮次 → 强制进入规划
        await self.conversations.add_message(HumanMessage(
            content="探索轮次已达上限。请基于已收集信息进入规划阶段，输出 ## 进入规划。",
        ), extra_metadata={"hint": True})
        # 再给一轮机会让 LLM 生成规划标记
        try:
            messages = [SystemMessage(content=system_prompt)]
            messages.extend(self.conversations.get_messages())
            response = await llm_with_tools.ainvoke(messages)
            await self.conversations.add_message(response)
            return response.content or ""
        except Exception:
            return ""

    # ── Phase 2: 规划 ──

    async def _run_plan_phase(self) -> tuple[Optional[Path], str]:
        """规划阶段循环。返回 (plan_path, plan_content)。"""
        await self._notify("system", "📝 Plan 模式 — 开始制定计划")

        system_prompt = _PLAN_SYSTEM_PROMPT
        llm_with_tools = self.llm.bind_tools(self.tools)

        # 注入过渡提示
        await self.conversations.add_message(HumanMessage(
            content="请基于上述探索结果，制定完整的执行计划。完成后调用 save_plan_doc 保存。",
        ), extra_metadata={"hint": True})

        plan_content_parts: list[str] = []

        for turn in range(self.MAX_PLAN_TURNS):
            if self.cancel_token.is_cancelled():
                return None, "".join(plan_content_parts)

            messages = [SystemMessage(content=system_prompt)]
            messages.extend(self.conversations.get_messages())

            response = await llm_with_tools.ainvoke(messages)
            content = response.content or ""
            plan_content_parts.append(content)

            await self._stream_content(content)

            # 检查完成标记
            if self._has_marker(content, PLAN_COMPLETE_MARKERS):
                await self.conversations.add_message(response)
                full = "".join(plan_content_parts)
                plan_path = self._save_content_to_plan_file(full)
                await self._notify("system", f"✅ 计划已保存: {plan_path}")
                return plan_path, full

            tool_calls = getattr(response, "tool_calls", [])

            if not tool_calls:
                # 规划阶段文本输出是正常的 — 不强制工具调用
                await self.conversations.add_message(response)
                await self.conversations.add_message(HumanMessage(
                    content="请继续完善计划，完成后调用 save_plan_doc 保存或输出 ## 计划完成。",
                ), extra_metadata={"hint": True})
                continue

            await self.conversations.add_message(response)

            # 检查是否有 save_plan_doc
            for tc in tool_calls:
                if tc.get("name") == "save_plan_doc":
                    args = tc.get("args", {})
                    filename = args.get("filename", "")
                    file_content = args.get("content", "".join(plan_content_parts))
                    plan_path = self._save_plan_doc(filename, file_content)
                    tool_id = tc.get("id", "")
                    result = f"计划已保存到: {plan_path}"
                    await self.conversations.add_message(
                        ToolMessage(content=result, tool_call_id=tool_id)
                    )
                    await self._notify("system", f"✅ 计划已保存: {plan_path}")
                    return plan_path, file_content or "".join(plan_content_parts)

            # 其他工具调用 → 正常处理
            await self._process_tool_calls(response, tool_calls, llm_with_tools, system_prompt)

        # 超出最大轮次 → 保存已有内容
        full = "".join(plan_content_parts)
        plan_path = self._save_content_to_plan_file(full) if full.strip() else None
        if plan_path:
            await self._notify("system", f"⚠️ 规划轮次达上限，已自动保存: {plan_path}")
        return plan_path, full

    # ── 工具调用处理 ──

    async def _process_tool_calls(
        self,
        response: Any,
        tool_calls: list,
        llm_with_tools: Any,
        system_prompt: str,
    ) -> None:
        """处理一轮工具调用，将结果写入 conversations。"""
        tool_name_map = {t.name: t for t in self.tools}

        for tc in tool_calls:
            tool_name = tc.get("name", "")
            tool_args = tc.get("args", {})
            tool_id = tc.get("id", "")

            await self._notify("tool", f"🔧 {tool_name}")

            tool = tool_name_map.get(tool_name)
            if tool is None:
                result = f"工具不可用: {tool_name}"
            else:
                try:
                    if self.cancel_token.is_cancelled():
                        result = "已取消"
                    else:
                        result = await tool.ainvoke(tool_args)
                except Exception as e:
                    result = f"工具执行失败: {e}"

            tool_info = str(result)[:5000] if result else "(空结果)"
            await self.conversations.add_message(
                ToolMessage(content=tool_info, tool_call_id=tool_id)
            )

    # ── 辅助方法 ──

    def _has_marker(self, content: str, markers: tuple[str, ...]) -> bool:
        """检查内容是否包含任意完成标记。"""
        return any(m in content for m in markers)

    def _looks_like_exploring(self, content: str) -> bool:
        """检查文本是否像探索中的过渡输出（非最终答案）。"""
        stripped = content.strip()
        if not stripped:
            return True
        if len(stripped) < 8:
            return True  # 太短 → 可能在思考
        exploration_phrases = (
            "让我", "先看", "查看", "找到", "搜索", "定位",
            "需要", "了解", "深入", "分析", "确认",
            "检查", "探索", "还不", "还需要", "进一步",
        )
        return any(p in stripped for p in exploration_phrases)

    def _explore_nudge(self, count: int) -> str:
        """探索阶段的渐进式催促。"""
        if count <= 2:
            return "请继续探索代码库，调用 read_file 或 search_code 了解更多相关代码。"
        elif count <= 4:
            return "请深入分析关键模块和依赖关系，调用工具继续探索。充分理解后再进入规划。"
        else:
            return "如果已收集足够信息，请输出 ## 进入规划 切换到规划阶段。"

    def _save_plan_doc(self, filename: str, content: str) -> Path:
        """保存计划文档到 .goat/doc/ 目录。"""
        safe_name = Path(filename).name
        if not safe_name.endswith(".md"):
            safe_name += ".md"

        doc_dir = self.workspace / ".goat" / "doc"
        doc_dir.mkdir(parents=True, exist_ok=True)

        target = doc_dir / safe_name
        target.write_text(content, encoding="utf-8")
        return target

    def _save_content_to_plan_file(self, content: str) -> Optional[Path]:
        """兜底保存：当 LLM 未调用 save_plan_doc 时自动保存。"""
        if not content.strip():
            return None
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        return self._save_plan_doc(f"plan_{timestamp}.md", content)

    async def _stream_content(self, content: str) -> None:
        """将文本内容通过 event_bus 推送（给 Web UI 流式显示）。"""
        if not content or not self.event_bus:
            return
        try:
            await self.event_bus.publish(SubagentEvent(
                EventType.LLM_STREAM, self.agent_id, "main",
                content, session_id=self.agent_id,
            ))
        except Exception:
            pass

    async def _notify(self, role: str, message: str) -> None:
        """发送系统通知到 event_bus。"""
        if not self.event_bus:
            return
        try:
            await self.event_bus.publish(SubagentEvent(
                EventType.MESSAGE, self.agent_id, role,
                message, session_id=self.agent_id,
            ))
        except Exception:
            pass