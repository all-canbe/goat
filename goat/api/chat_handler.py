from __future__ import annotations

import json
import logging
import sqlite3
from pathlib import Path

from langchain_openai import ChatOpenAI

from goat.conversation.conversation_manager import ConversationManager
from goat.conversation.context_compression import CompactionConfig
from goat.core.event_bus import EventBus
from goat.core.workspace import get_goat_home
from goat.provider.provider import (
    ProviderConfig, ProviderType, create_llm, parse_provider,
    get_provider_display,
)
from goat.security.approval import PermissionMode

from goat.api.session_engine import SessionEngine

logger = logging.getLogger(__name__)

GOAT_HOME = get_goat_home()
SETTINGS_FILE = GOAT_HOME / "setting.json"


def _get_db_path() -> str:
    preferred = GOAT_HOME / "conversations.db"
    try:
        conn = sqlite3.connect(str(preferred))
        conn.close()
        return str(preferred)
    except (sqlite3.OperationalError, OSError):
        fallback = Path.cwd() / ".goat_web" / "conversations.db"
        fallback.parent.mkdir(parents=True, exist_ok=True)
        logger.warning("GOAT_HOME 不可写，回退到 %s", fallback)
        return str(fallback)


def _load_settings() -> dict | None:
    try:
        if SETTINGS_FILE.exists():
            data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
            if data.get("api_key"):
                return data
    except (json.JSONDecodeError, OSError):
        pass
    return None


class SessionManager:
    def __init__(self):
        self._engines: dict[str, SessionEngine] = {}
        self.event_bus = EventBus()
        self.llm: ChatOpenAI | None = None
        self.conversations: ConversationManager | None = None
        self.provider_config: ProviderConfig | None = None
        self.workspace: Path = Path.cwd().resolve()
        self._initialized = False

    async def initialize(self, workspace: str | None = None) -> None:
        if workspace:
            self.workspace = Path(workspace).resolve()
        settings = _load_settings()
        if not settings:
            logger.error("未找到配置文件 %s，请先运行 goat cli 完成初始化", SETTINGS_FILE)
            return

        provider_str = settings.get("provider", "openai_compatible")
        provider_type = parse_provider(provider_str) or ProviderType.OPENAI_COMPATIBLE
        self.provider_config = ProviderConfig(
            provider_type=provider_type,
            base_url=settings.get("base_url", ""),
            api_key=settings["api_key"],
            model=settings.get("model", ""),
        )

        self.llm = create_llm(self.provider_config)

        self.conversations = ConversationManager(
            db_path=_get_db_path(),
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

        from goat.tools.retry import init_retry
        init_retry(self.event_bus)

        self._initialized = True
        logger.info("SessionManager 初始化完成 %s", get_provider_display(self.provider_config))

    def set_workspace(self, path: str) -> tuple[bool, str]:
        p = Path(path).resolve()
        if not p.exists():
            return False, f"路径不存在: {path}"
        if not p.is_dir():
            return False, f"路径不是目录: {path}"
        self.workspace = p
        return True, str(self.workspace)

    def set_mode(self, mode: PermissionMode) -> None:
        for engine in self._engines.values():
            if engine.approval_system:
                engine.approval_system.set_mode(mode)

    async def handle_chat_send(self, text: str, session_id: str = "default") -> None:
        if not self._initialized:
            logger.error("SessionManager 未初始化，无法处理消息")
            return

        engine = await self._get_or_create_engine(session_id)
        if engine.is_running:
            logger.warning("Session %s 已有运行中的 Agent", session_id[:8])
            return
        await engine.start(text)

    async def cancel_session(self, session_id: str) -> None:
        engine = self._engines.get(session_id)
        if engine:
            await engine.cancel()

    async def _get_or_create_engine(self, session_id: str) -> SessionEngine:
        if session_id not in self._engines:
            engine = SessionEngine(
                session_id=session_id,
                event_bus=self.event_bus,
                provider_config=self.provider_config,
                workspace=self.workspace,
                db_path=_get_db_path(),
            )
            await engine.initialize()
            self._engines[session_id] = engine
        return self._engines[session_id]


session_manager = SessionManager()


web_chat_handler = session_manager