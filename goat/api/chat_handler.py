from __future__ import annotations

import asyncio
import json
import logging
import sqlite3
from pathlib import Path

from langchain_core.language_models.chat_models import BaseChatModel
from langchain_openai import ChatOpenAI

from goat.conversation.conversation_manager import ConversationManager
from goat.conversation.context_compression import CompactionConfig
from goat.core.event_bus import EventBus, EventType
from goat.core.workspace import (
    get_goat_home, get_skills_dir, get_workspace_goat_dir,
    ensure_workspace_goat_layout,
)
from goat.agent.skill_system import SkillRegistry
from goat.provider.provider import (
    ProviderConfig, ProviderType, create_llm, parse_provider,
    get_provider_display,
)
from goat.security.approval import PermissionMode

from goat.api.session_engine import SessionEngine
from goat.api.websocket import ws_manager

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
        self.review_provider_config: ProviderConfig | None = None
        self.review_llm: BaseChatModel | None = None
        self._flow_plan_event: asyncio.Event | None = None
        self._flow_plan_choice: str = ""

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
            max_tokens=settings.get("max_tokens"),
            timeout=settings.get("timeout"),
        )

        self.llm = create_llm(self.provider_config)

        # ---- 加载审查模型（Flow 模式用） ----
        review_settings = _load_settings_safe()
        if review_settings.get("review_api_key"):
            try:
                self.review_provider_config = ProviderConfig(
                    provider_type=parse_provider(review_settings.get("review_provider", ""))
                        or self.provider_config.provider_type,
                    base_url=review_settings.get("review_base_url", self.provider_config.base_url),
                    api_key=review_settings["review_api_key"],
                    model=review_settings.get("review_model", self.provider_config.model),
                )
                self.review_llm = create_llm(self.review_provider_config)
                logger.info("审查模型已加载: %s", review_settings.get("review_model", "(default)"))
            except Exception as e:
                logger.warning("审查模型加载失败，使用主模型: %s", e)
                self.review_provider_config = None
                self.review_llm = None

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

        # ---- 全局 + 项目 Skill 合并加载（Claude Code 模式） ----
        # 优先级：项目 .goat/skills > get_skills_dir > 包内 skills > ~/.goat/skills
        _pkg_root = Path(__file__).resolve().parent.parent.parent
        _goat_dir = ensure_workspace_goat_layout(self.workspace)
        _workspace_skills = _goat_dir / "skills"
        _user_skills = GOAT_HOME / "skills"
        _user_skills.mkdir(parents=True, exist_ok=True)
        _skill_candidates = [
            _workspace_skills,
            get_skills_dir(),
            _pkg_root / "skills",
            _user_skills,
        ]
        _loaded_dirs: set[Path] = set()
        for _d in _skill_candidates:
            if _d in _loaded_dirs or not _d.exists():
                continue
            _loaded = self.skill_registry.load_skills_from_directory(_d)
            _loaded_dirs.add(_d)
            if _loaded:
                logger.info("从 %s 加载了 %d 个技能: %s", _d, len(_loaded), [s.name for s in _loaded])

        # 内置回退：不论目录扫描结果如何，确保 kz-skill-creator 存在
        if not self.skill_registry.get("kz-skill-creator"):
            _kz_dir = _pkg_root / "skills" / "kz-skill-creator"
            if _kz_dir.exists():
                from goat.agent.skill_system import load_skill_from_directory
                _s = load_skill_from_directory(_kz_dir)
                if _s:
                    self.skill_registry.register(_s)
                    logger.info("已注册内置回退技能 kz-skill-creator")

        from goat.tools.retry import init_retry
        init_retry(self.event_bus)

        self._initialized = True
        logger.info("SessionManager 初始化完成 %s", get_provider_display(self.provider_config))

        from goat.tools.tools import set_plan_workspace
        set_plan_workspace(self.workspace)

        from goat.tools.ask_user_tool import set_external_queue
        set_external_queue(asyncio.Queue())

    def set_workspace(self, path: str) -> tuple[bool, str]:
        p = Path(path).resolve()
        if not p.exists():
            return False, f"路径不存在: {path}"
        if not p.is_dir():
            return False, f"路径不是目录: {path}"
        self.workspace = p
        from goat.tools.tools import set_plan_workspace
        set_plan_workspace(self.workspace)
        ensure_workspace_goat_layout(p)
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

        # 检测 /flow_plan 命令，走独立流水线
        if text.strip().startswith("/flow_plan"):
            await self.handle_flow_plan(text, session_id)
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
        # 通知前端刷新会话列表（可能有自动命名或新会话创建）
        await ws_manager.broadcast_to_all({
            "type": "session.list.update",
            "payload": {}
        })

    async def handle_flow_plan(self, text: str, session_id: str = "default") -> None:
        """处理 /flow_plan 命令：计划生成 → 审查 → 用户选择 → YOLO 执行"""
        if not self._initialized:
            logger.error("SessionManager 未初始化")
            return

        task = text
        if task.startswith("/flow_plan"):
            task = task[len("/flow_plan"):].strip()
        if not task:
            await ws_manager.broadcast_to_session(session_id, {
                "type": "system.error",
                "payload": {"message": "用法: /flow_plan <任务描述>"}
            })
            return

        from goat.agent.pipeline import FlowPipeline
        from goat.agent.subagent_manager import SubAgentManager
        from goat.core.cancellation import CancellationToken
        from goat.tools.tools import get_tools_by_names
        from goat.agent.subagent_roles import ROLE_REGISTRY, RoleType

        manager = SubAgentManager()
        cancel_token = CancellationToken()
        review_llm = self.review_llm or self.llm

        pipeline = FlowPipeline(
            impl_llm=self.llm,
            review_llm=review_llm,
            manager=manager,
            skill_registry=self.skill_registry,
            event_bus=self.event_bus,
            cancel_token=cancel_token,
        )

        async def plan_gate_handler(phase: str, info: dict):
            if phase == "plan_compare":
                self._flow_plan_event = asyncio.Event()
                await ws_manager.broadcast_to_session(session_id, {
                    "type": "flow.plan_compare",
                    "payload": {
                        "task": info.get("task", ""),
                        "original_plan": info.get("original_plan", ""),
                        "reviewed_plan": info.get("reviewed_plan", ""),
                    }
                })
                try:
                    await asyncio.wait_for(
                        self._flow_plan_event.wait(),
                        timeout=300.0,
                    )
                except asyncio.TimeoutError:
                    self._flow_plan_choice = "cancelled"
                finally:
                    self._flow_plan_event = None
                return self._flow_plan_choice
            return "reviewed"

        pipeline.gate_callback = plan_gate_handler

        report = await pipeline.run_plan_first(task)

        await ws_manager.broadcast_to_session(session_id, {
            "type": "flow.plan_result",
            "payload": {
                "choice": report.choice,
                "success": report.success,
                "summary": report.summary,
                "verification_errors": report.verification_errors,
            }
        })

    def submit_flow_plan_choice(self, choice: str) -> None:
        self._flow_plan_choice = choice
        if self._flow_plan_event:
            self._flow_plan_event.set()

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
            # 通知前端新会话已创建
            await ws_manager.broadcast_to_all({
                "type": "session.list.update",
                "payload": {}
            })
        return self._engines[session_id]

    def get_review_llm(self) -> BaseChatModel:
        """返回 review llm，未配置时退化为主 llm。"""
        return self.review_llm or self.llm

    def get_review_provider_config(self) -> ProviderConfig | None:
        """供 /api/config GET 使用。"""
        return self.review_provider_config

    def update_review_config(self, review_config: ProviderConfig | None) -> None:
        """更新审查模型配置：保存 setting.json + 更新运行时引用。"""
        if review_config is None:
            self.review_provider_config = None
            self.review_llm = None
            if SETTINGS_FILE.exists():
                try:
                    data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
                    for key in ("review_model", "review_api_key", "review_base_url",
                                "review_provider", "review_model_prompted"):
                        data.pop(key, None)
                    SETTINGS_FILE.write_text(
                        json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
                except (OSError, json.JSONDecodeError) as e:
                    logger.warning("清空审查配置失败: %s", e)
            return

        self.review_provider_config = review_config
        self.review_llm = create_llm(review_config)
        _save_settings({
            "review_provider": review_config.provider_type.value,
            "review_base_url": review_config.base_url,
            "review_api_key": review_config.api_key,
            "review_model": review_config.model,
        })


session_manager = SessionManager()


web_chat_handler = session_manager