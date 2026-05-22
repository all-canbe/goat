"""
上下文压缩系统 - 多层渐进式摘要合并
集 Claude Code、DeepSeek TUI、OpenAI Codex 优点于一身
"""

from __future__ import annotations

import hashlib
import json
import re
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Any, Callable, Optional


class CompactionTrigger(Enum):
    MANUAL = auto()
    AUTO_THRESHOLD = auto()
    AUTO_HEADROOM = auto()
    TIME_BASED = auto()
    MID_TURN = auto()


class CompactionPhase(Enum):
    MICRO = auto()
    CONTEXT_COLLAPSE = auto()
    SESSION_MEMORY = auto()
    FULL = auto()


@dataclass
class TokenEstimate:
    system: int = 0
    user_messages: int = 0
    assistant_messages: int = 0
    tool_calls: int = 0
    tool_results: int = 0
    total: int = 0

    @property
    def total_with_headroom(self) -> int:
        return self.total


@dataclass
class CompactionConfig:
    context_window: int = 100_000
    compaction_threshold_ratio: float = 0.8
    output_headroom: int = 10_000
    compaction_headroom: int = 15_000

    micro_compact_tool_count: int = 20
    micro_compact_keep_recent: int = 10
    micro_compact_min_tokens: int = 20_000
    micro_compact_time_threshold_minutes: int = 5

    collapse_min_tokens: int = 30_000
    collapse_max_tokens: int = 50_000
    session_memory_min_tokens: int = 10_000
    session_memory_max_tokens: int = 40_000
    session_memory_min_text_blocks: int = 5
    summary_max_tokens: int = 8_000
    user_message_max_tokens: int = 20_000

    cache_enabled: bool = True
    cache_ttl_minutes: int = 5
    preserve_cache_on_compact: bool = True

    hot_tail_size: int = 3
    hot_tail_tokens: int = 40_000
    prefer_cache_stability: bool = True

    def effective_threshold(self) -> int:
        return int(self.context_window * self.compaction_threshold_ratio)


@dataclass
class CompressionToolResult:
    id: str
    name: str
    arguments: dict
    result: str
    timestamp: float = field(default_factory=time.time)
    cached: bool = False

    @property
    def token_estimate(self) -> int:
        return len(self.result) // 4

    def to_reference(self) -> str:
        return f"[Tool result saved to: /tmp/tool_result_{self.id}]"


@dataclass
class CompressionMessage:
    role: str
    content: str
    tool_use_id: Optional[str] = None
    tool_name: Optional[str] = None
    tool_arguments: Optional[dict] = None
    timestamp: float = field(default_factory=time.time)
    cached: bool = False

    @classmethod
    def from_langchain(cls, msg: Any) -> "CompressionMessage":
        return cls(
            role=msg.type,
            content=str(msg.content or ""),
            tool_use_id=getattr(msg, "tool_call_id", None),
            tool_name=getattr(msg, "name", None),
            tool_arguments=getattr(msg, "additional_kwargs", None),
        )

    def token_estimate(self) -> int:
        return len(self.content) // 4


class CompactionSummary(ABC):
    @property
    @abstractmethod
    def text(self) -> str:
        ...

    @abstractmethod
    def token_count(self) -> int:
        ...


class SummaryChain:
    def __init__(self, max_tokens: int = 8000):
        self._summaries: list[str] = []
        self._max_tokens = max_tokens

    def add(self, summary: str) -> None:
        self._summaries.append(summary)

    def merge(self) -> str:
        return "\n\n".join(self._summaries)

    def token_count(self) -> int:
        return len(self.merge()) // 4

    def exceeded(self) -> bool:
        from_token_count = self.token_count()
        if from_token_count > self._max_tokens:
            return True
        return False


class PrefixCacheManager:
    def __init__(self, ttl_minutes: int = 5):
        self._cache: dict[str, tuple[str, float]] = {}
        self._ttl = ttl_minutes * 60

    def get(self, key: str) -> Optional[str]:
        entry = self._cache.get(key)
        if entry is None:
            return None
        if time.time() - entry[1] > self._ttl:
            del self._cache[key]
            return None
        return entry[0]

    def set(self, key: str, value: str) -> None:
        self._cache[key] = (value, time.time())

    def invalidate(self, key: str) -> None:
        self._cache.pop(key, None)

    def clear(self) -> None:
        self._cache.clear()

    def size(self) -> int:
        return len(self._cache)


class CompressionContext:
    def __init__(self):
        self.messages: list[CompressionMessage] = []
        self.tool_results: dict[str, CompressionToolResult] = {}
        self.summary_chain = SummaryChain()
        self.cache = PrefixCacheManager()
        self.token_estimate = TokenEstimate()

    def add_message(self, msg: CompressionMessage) -> None:
        self.messages.append(msg)

    def add_tool_result(self, result: CompressionToolResult) -> None:
        self.tool_results[result.id] = result

    def clear_tool_results(self) -> None:
        self.tool_results.clear()

    def total_messages(self) -> int:
        return len(self.messages)

    def estimate_tokens(self) -> TokenEstimate:
        est = TokenEstimate()
        for msg in self.messages:
            tokens = msg.token_estimate()
            if msg.role == "system":
                est.system += tokens
            elif msg.role in ("human", "user"):
                est.user_messages += tokens
            elif msg.role == "assistant":
                est.assistant_messages += tokens
                if msg.tool_use_id:
                    est.tool_calls += tokens
            elif msg.role == "tool":
                est.tool_results += tokens
        est.total = est.system + est.user_messages + est.assistant_messages + est.tool_calls + est.tool_results
        return est


class CompactionPipeline:
    def __init__(self, config: CompactionConfig | None = None):
        self.config = config or CompactionConfig()

    async def should_compact(self, ctx: CompressionContext) -> tuple[bool, CompactionTrigger, CompactionPhase]:
        est = ctx.estimate_tokens()
        effective_threshold = self.config.effective_threshold()

        tool_count = est.tool_calls
        if tool_count >= self.config.micro_compact_tool_count and est.total >= self.config.micro_compact_min_tokens:
            return True, CompactionTrigger.AUTO_THRESHOLD, CompactionPhase.MICRO

        if est.total >= effective_threshold:
            if est.total >= self.config.collapse_min_tokens:
                return True, CompactionTrigger.AUTO_THRESHOLD, CompactionPhase.CONTEXT_COLLAPSE
            return True, CompactionTrigger.AUTO_THRESHOLD, CompactionPhase.MICRO

        if ctx.summary_chain.token_count() >= self.config.session_memory_min_tokens:
            return True, CompactionTrigger.TIME_BASED, CompactionPhase.SESSION_MEMORY

        return False, CompactionTrigger.MANUAL, CompactionPhase.FULL

    async def compact(self, messages: list[CompressionMessage], phase: CompactionPhase) -> list[CompressionMessage]:
        if phase == CompactionPhase.MICRO:
            return self._micro_compact(messages)
        elif phase == CompactionPhase.CONTEXT_COLLAPSE:
            return self._context_collapse(messages)
        elif phase == CompactionPhase.SESSION_MEMORY:
            return self._session_memory_compact(messages)
        elif phase == CompactionPhase.FULL:
            return self._full_compact(messages)
        return messages

    def _micro_compact(self, messages: list[CompressionMessage]) -> list[CompressionMessage]:
        keep_recent = self.config.micro_compact_keep_recent
        if len(messages) <= keep_recent + 1:
            return messages

        unrecent = messages[:-keep_recent]
        recent = messages[-keep_recent:]

        summary_text = f"[系统已压缩 {len(unrecent)} 条历史消息]"
        summary_msg = CompressionMessage(role="system", content=summary_text)

        return [summary_msg] + recent

    def _context_collapse(self, messages: list[CompressionMessage]) -> list[CompressionMessage]:
        collapse_count = len(messages) // 2
        if collapse_count < 1:
            return messages

        to_collapse = messages[:collapse_count]
        remaining = messages[collapse_count:]

        collapse_text = f"[系统已压缩 {len(to_collapse)} 条上下文消息]"
        summary_msg = CompressionMessage(role="system", content=collapse_text)

        return [summary_msg] + remaining

    def _session_memory_compact(self, messages: list[CompressionMessage]) -> list[CompressionMessage]:
        return self._context_collapse(messages)

    def _full_compact(self, messages: list[CompressionMessage]) -> list[CompressionMessage]:
        summary_text = f"[系统已压缩全部 {len(messages)} 条消息]"
        return [CompressionMessage(role="system", content=summary_text) + messages[-1:]]


class ContextManager:
    def __init__(self, config: CompactionConfig | None = None):
        self.config = config or CompactionConfig()
        self.pipeline = CompactionPipeline(self.config)
        self.context = CompressionContext()
        self._phase = CompactionPhase.MICRO

    @property
    def phase(self) -> CompactionPhase:
        return self._phase

    def add_message(self, msg: CompressionMessage) -> None:
        self.context.add_message(msg)

    def add_messages(self, messages: list[CompressionMessage]) -> None:
        for msg in messages:
            self.context.add_message(msg)

    def get_messages(self) -> list[CompressionMessage]:
        return self.context.messages

    async def is_compact_needed(self) -> bool:
        should, trigger, phase = await self.pipeline.should_compact(self.context)
        return should

    async def run_compact(self) -> list[CompressionMessage]:
        should, trigger, phase = await self.pipeline.should_compact(self.context)
        if not should:
            return self.context.messages

        self._phase = phase
        compacted = await self.pipeline.compact(self.context.messages, phase)
        self.context.messages = compacted
        return compacted

    def estimate_tokens(self) -> TokenEstimate:
        return self.context.estimate_tokens()

    def reset(self) -> None:
        self.context = CompressionContext()
        self._phase = CompactionPhase.MICRO