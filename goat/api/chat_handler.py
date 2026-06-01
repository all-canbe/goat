from __future__ import annotations

import asyncio
import json
import logging
import sqlite3
from pathlib import Path

from langchain_openai import ChatOpenAI

from goat.conversation.conversation_manager import ConversationManager
from goat.conversation.context_compression import CompactionConfig
from goat.core.event_bus import EventBus, EventType
from goat.core.workspace import get_goat_home, get_skills_dir
from goat.agent.skill_system import SkillRegistry
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


def _save_settings(settings: dict) -> None:
    try:
        existing = {}
        if SETTINGS_FILE.exists():
            existing = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
        existing.update(settings)
        SETTINGS_FILE.write_text(
            json.dumps(existing, indent=2, ensure_ascii=False),
            encoding="utf-8",
        )
    except (OSError, json.JSONDecodeError) as e:
        logger.warning("保存 setting.json 失败: %s", e)


def _load_settings_safe() -> dict:
    try:
        if SETTINGS_FILE.exists():
            return json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        pass
    return {}


class SessionManager:
    def __init__(self):
        self._engines: dict[str, SessionEngine] = {}
        self.event_bus = EventBus()
        self.llm: ChatOpenAI | None = None
        self.conversations: ConversationManager | None = None
        self.provider_config: ProviderConfig | None = None
        self.workspace: Path = Path.cwd().resolve()
        self._initialized = False
        self._current_mode: PermissionMode | None = None
        self.skill_registry = SkillRegistry()

    async def initialize(self, workspace: str | None = None) -> None:
        if workspace:
            self.workspace = Path(workspace).resolve()
        else:
            settings = _load_settings_safe()
            saved = settings.get("workspace", "")
            if saved:
                p = Path(saved).resolve()
                if p.is_dir():
                    self.workspace = p
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

        skills_dir = get_skills_dir()
        if skills_dir.exists():
            self.skill_registry.load_skills_from_directory(skills_dir)
            loaded = self.skill_registry.list_all()
            if loaded:
                logger.info("加载了 %d 个技能: %s", len(loaded), [s.name for s in loaded])

        from goat.tools.retry import init_retry
        init_retry(self.event_bus)

        self._initialized = True
        logger.info("SessionManager 初始化完成 %s", get_provider_display(self.provider_config))

        from goat.tools.ask_user_tool import set_external_queue
        set_external_queue(asyncio.Queue())

    def set_workspace(self, path: str) -> tuple[bool, str]:
        p = Path(path).resolve()
        if not p.exists():
            return False, f"路径不存在: {path}"
        if not p.is_dir():
            return False, f"路径不是目录: {path}"
        self.workspace = p
        _save_settings({"workspace": str(p)})
        for engine in self._engines.values():
            engine.close()
        self._engines.clear()
        return True, str(self.workspace)

    def set_mode(self, mode: PermissionMode) -> None:
        self._current_mode = mode
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
            self.event_bus.publish_nowait(
                "system", EventType.ERROR,
                "已有运行中的任务，请等待完成或取消后重试",
                agent_name="system", session_id=session_id,
            )
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
                skill_registry=self.skill_registry,
            )
            await engine.initialize()
            if self._current_mode and engine.approval_system:
                engine.approval_system.set_mode(self._current_mode)
            self._engines[session_id] = engine
        return self._engines[session_id]


session_manager = SessionManager()


web_chat_handler = session_manager