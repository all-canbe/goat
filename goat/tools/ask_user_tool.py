from __future__ import annotations

import asyncio
import sys
from typing import Type

from pydantic import BaseModel, Field
from langchain_core.tools import BaseTool

from goat.core.event_bus import EventBus, EventType


_pending_event: asyncio.Event | None = None
_pending_question: str = ""
_pending_options: list[str] = []
_user_response: str = ""
_external_queue: asyncio.Queue[str] | None = None
_external_loop: asyncio.AbstractEventLoop | None = None


class AskUserInput(BaseModel):
    question: str = Field(description="要向用户提出的问题，应清晰具体")
    options: str = Field(
        default="",
        description="可选的快捷选项列表，用 | 分隔，如 '�?| �?| 取消'",
    )


def set_external_queue(queue: asyncio.Queue[str] | None) -> None:
    global _external_queue
    _external_queue = queue


def submit_user_answer(answer: str) -> None:
    global _user_response, _pending_event
    _user_response = answer
    if _pending_event is not None and not _pending_event.is_set():
        _pending_event.set()


def has_pending_question() -> bool:
    return _pending_event is not None and not _pending_event.is_set()


class AskUserTool(BaseTool):
    name: str = "ask_user"
    description: str = (
        "向用户提出一个问题并等待回答。当需要用户做决策、提供信息、确认操作时使用。\n"
        "典型用法: '确认是否执行此操作？' �?'请提供配置信�? �?'你想怎么处理�?\n"
        "可选提供选项列表（用 | 分隔），让用户从预设选项中选择。"
    )
    args_schema: Type[BaseModel] = AskUserInput
    return_direct: bool = False

    def _run(self, question: str, options: str = "") -> str:
        global _pending_event, _pending_question, _pending_options, _user_response

        opts_list = [o.strip() for o in options.split("|")] if options else []
        _pending_question = question
        _pending_options = opts_list
        _user_response = ""
        _pending_event = asyncio.Event()

        self._notify(question, opts_list)

        prompt = self._format_prompt(question, opts_list)
        try:
            return input(prompt).strip()
        except (EOFError, KeyboardInterrupt):
            return "用户取消了输入"

    async def _arun(self, question: str, options: str = "") -> str:
        global _pending_event, _pending_question, _pending_options, _user_response

        opts_list = [o.strip() for o in options.split("|")] if options else []
        _pending_question = question
        _pending_options = opts_list
        _user_response = ""
        _pending_event = asyncio.Event()

        self._notify(question, opts_list)

        if _external_queue is not None:
            _external_queue.put_nowait(f"__ask_user__:{question}")
            try:
                answer = await asyncio.wait_for(
                    self._wait_for_answer(), timeout=300.0
                )
                return answer
            except asyncio.TimeoutError:
                return "用户未在超时时间内回复"

        loop = asyncio.get_event_loop()
        try:
            prompt = self._format_prompt(question, opts_list)
            answer = await loop.run_in_executor(
                None, lambda: input(prompt).strip()
            )
            return answer
        except (EOFError, KeyboardInterrupt):
            return "用户取消了输入"

    async def _wait_for_answer(self) -> str:
        global _pending_event, _user_response
        while True:
            await asyncio.wait_for(_pending_event.wait(), timeout=300.0)
            if _user_response:
                return _user_response
            _pending_event.clear()

    def _notify(self, question: str, opts: list[str]) -> None:
        try:
            from goat.tools.notify_tool import _get_event_bus
            bus = _get_event_bus()
            opts_text = f" ({', '.join(opts)})" if opts else ""
            payload = f"{question}{opts_text}"
            bus.publish_nowait(
                "ask_user_tool", EventType.MESSAGE,
                f"[用户提问] {payload}",
                agent_name="system",
            )
            bus.publish_nowait(
                "ask_user_tool", EventType.ASK_USER,
                payload,
                agent_name="system",
            )
        except Exception:
            pass

    @staticmethod
    def _format_prompt(question: str, opts: list[str]) -> str:
        prompt = f"\n[用户提问] {question}"
        if opts:
            prompt += f"\n  选项: {', '.join(opts)}"
        prompt += "\n请输入回�? "
        return prompt


ASK_USER_TOOL = AskUserTool()