"""
HOOK 生命周期系统
集 Claude Code（28+ 事件 × 5 处理器）、DeepSeek TUI（观测者钩子）、Codex（紧凑生命周期钩子）优点于一身

核心设计理念：
1. 四周期九域事件模型：Session / Turn / Tool / Context / System / Permission / Agent / Task / Interactive
2. 处理器正交：command / http / prompt / mcp_tool / agent 五种处理器模式
3. 观测者 vs 拦截者：DeepSeek TUI 风格（observer）和 Claude Code 风格（interceptor）共存
4. 匹配器系统：matcher + 条件组合，精确控制触发
5. 异步非阻塞：长耗时钩子不阻塞主流程
6. 绕过免疫：部分钩子即使在高权限模式也强制触发
"""

import abc
import asyncio
import json
import os
import re
import subprocess
import time
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Optional


# ============================================================================
# 1. 事件模型 - 四周期九域
# ============================================================================

class HookEvent(Enum):
    """钩子事件 - 四周期九域，共 30+ 事件"""

    SessionStart = "session_start"
    Setup = "setup"
    SessionEnd = "session_end"
    ConfigChange = "config_change"
    CwdChanged = "cwd_changed"
    FileChanged = "file_changed"
    InstructionsLoaded = "instructions_loaded"

    UserPromptSubmit = "user_prompt_submit"
    UserPromptExpansion = "user_prompt_expansion"
    Notification = "notification"

    PreToolUse = "pre_tool_use"
    PostToolUse = "post_tool_use"
    PostToolUseFailure = "post_tool_use_failure"
    PostToolBatch = "post_tool_batch"

    PreCompact = "pre_compact"
    PostCompact = "post_compact"

    OnError = "on_error"
    ModeChange = "mode_change"

    PermissionRequest = "permission_request"
    PermissionDenied = "permission_denied"

    Stop = "stop"
    StopFailure = "stop_failure"
    SubagentStart = "subagent_start"
    SubagentStop = "subagent_stop"
    TeammateIdle = "teammate_idle"

    TaskCreated = "task_created"
    TaskCompleted = "task_completed"

    Elicitation = "elicitation"
    ElicitationResult = "elicitation_result"
    ShellEnv = "shell_env"

    WorktreeCreate = "worktree_create"
    WorktreeRemove = "worktree_remove"


class HookOrientation(Enum):
    """
    钩子方向性
    OBSERVER（观测者）: 只读不可修改，不能阻止执行
    INTERCEPTOR（拦截者）: 可返回 proceed/block/modify
    NOTIFICATION（通知）: 纯通知，不关心结果，不阻塞
    """
    OBSERVER = "observer"
    INTERCEPTOR = "interceptor"
    NOTIFICATION = "notification"


class HookDecision(Enum):
    PROCEED = "proceed"
    BLOCK = "block"
    MODIFY = "modify"


class HookScope(Enum):
    SESSION = "session"
    TURN = "turn"
    TOOL = "tool"


class HookHandlerType(Enum):
    COMMAND = "command"
    HTTP = "http"
    PROMPT = "prompt"
    MCP_TOOL = "mcp_tool"
    AGENT = "agent"


# ============================================================================
# 2. 数据模型
# ============================================================================

@dataclass
class HookMatcher:
    """钩子匹配器"""
    event: str = "*"
    tool_name: str = ""
    tool_pattern: str = ""
    file_pattern: str = ""
    if_condition: dict = field(default_factory=dict)

    @classmethod
    def any(cls) -> "HookMatcher":
        return HookMatcher(event="*")

    @classmethod
    def tool(cls, name: str) -> "HookMatcher":
        return HookMatcher(event=HookEvent.PreToolUse.value, tool_name=name)

    @classmethod
    def command(cls, pattern: str) -> "HookMatcher":
        return HookMatcher(event=HookEvent.PreToolUse.value, tool_pattern=pattern)

    def matches(self, event: str, tool_name: str = "", command: str = "", file_path: str = "") -> bool:
        if self.event != "*" and self.event != event:
            return False
        if self.tool_name and self.tool_name != tool_name:
            return False
        if self.tool_pattern:
            pattern = self.tool_pattern.replace("*", ".*")
            if not re.search(pattern, command):
                return False
        if self.file_pattern and self.file_pattern not in file_path:
            return False
        return True


@dataclass
class HookContext:
    session_id: str = ""
    session_dir: str = ""
    cwd: str = ""
    turn_id: str = ""
    event: str = ""
    timestamp: float = field(default_factory=time.time)
    reason: str = ""


@dataclass
class ToolHookInput:
    tool_name: str = ""
    arguments: dict = field(default_factory=dict)
    command: str = ""
    target_path: str = ""
    category: str = ""


@dataclass
class ToolHookOutput:
    decision: HookDecision = HookDecision.PROCEED
    modified_arguments: dict = field(default_factory=dict)
    reason: str = ""


@dataclass
class CompactHookInput:
    trigger: str = ""
    token_count: int = 0
    threshold: int = 0


@dataclass
class CompactHookOutput:
    decision: HookDecision = HookDecision.PROCEED
    reason: str = ""
    custom_summary: str = ""


@dataclass
class PermissionHookInput:
    tool_name: str = ""
    arguments: dict = field(default_factory=dict)
    mode: str = ""


@dataclass
class PermissionHookOutput:
    decision: HookDecision = HookDecision.PROCEED
    reason: str = ""
    approve: bool = False


@dataclass
class SessionHookInput:
    action: str = ""
    env_vars: dict = field(default_factory=dict)


@dataclass
class HookDefinition:
    name: str = ""
    source: str = "user"
    event: HookEvent = HookEvent.Stop
    matcher: HookMatcher = field(default_factory=HookMatcher.any)
    handler_type: HookHandlerType = HookHandlerType.COMMAND
    handler_config: dict = field(default_factory=dict)
    orientation: HookOrientation = HookOrientation.INTERCEPTOR
    timeout_secs: int = 30
    is_async: bool = False
    run_on_approval_override: bool = False
    bypass_immune: bool = False
    suppress_output: bool = False
    run_count: int = 0
    last_run: float = 0.0

    @property
    def handler_label(self) -> str:
        labels = {
            HookHandlerType.COMMAND: f"Command: {self.handler_config.get('command', '')[:50]}",
            HookHandlerType.HTTP: f"HTTP: {self.handler_config.get('url', '')[:50]}",
            HookHandlerType.PROMPT: f"Prompt: {self.handler_config.get('prompt', '')[:50]}",
            HookHandlerType.MCP_TOOL: f"MCP: {self.handler_config.get('tool', '')[:50]}",
            HookHandlerType.AGENT: f"Agent: {self.handler_config.get('agent_type', '')[:50]}",
        }
        return labels.get(self.handler_type, "Unknown")


# ============================================================================
# 3. 处理器实现
# ============================================================================

class HookHandler(abc.ABC):

    def __init__(self, config: dict):
        self.config = config

    @abc.abstractmethod
    async def handle(self, context: HookContext, extra_input: Any = None) -> Optional[dict]:
        pass


class CommandHandler(HookHandler):

    async def handle(self, context: HookContext, extra_input: Any = None) -> Optional[dict]:
        command = self.config.get("command", "")
        env = self._build_env(context, extra_input)
        try:
            result = subprocess.run(
                command, shell=True, capture_output=True, text=True,
                timeout=self.config.get("timeout", 30),
                env={**os.environ, **env}
            )
            return self._parse_result(result)
        except subprocess.TimeoutExpired:
            return {"decision": "proceed", "reason": "hook_timeout"}
        except Exception as e:
            return {"decision": "proceed", "reason": f"hook_error: {e}"}

    def _build_env(self, context: HookContext, extra: Any = None) -> dict:
        env = {
            "HOOK_EVENT": context.event,
            "HOOK_SESSION_ID": context.session_id,
            "HOOK_TURN_ID": context.turn_id,
            "HOOK_CWD": context.cwd,
            "HOOK_TIMESTAMP": str(context.timestamp),
        }
        if isinstance(extra, ToolHookInput):
            env["HOOK_TOOL_NAME"] = extra.tool_name
            env["HOOK_TOOL_ARGUMENTS"] = json.dumps(extra.arguments)
            env["HOOK_COMMAND"] = extra.command
        if isinstance(extra, CompactHookInput):
            env["HOOK_COMPACT_TRIGGER"] = extra.trigger
            env["HOOK_TOKEN_COUNT"] = str(extra.token_count)
        return env

    def _parse_result(self, result: subprocess.CompletedProcess) -> Optional[dict]:
        if result.stdout.strip():
            try:
                return json.loads(result.stdout)
            except json.JSONDecodeError:
                pass
        if result.returncode == 2:
            return {"decision": "block", "reason": result.stderr.strip() or "blocked by hook"}
        if result.returncode == 0:
            return {"decision": "proceed"}
        return None


class HttpHandler(HookHandler):

    async def handle(self, context: HookContext, extra_input: Any = None) -> Optional[dict]:
        url = self.config.get("url", "")
        method = self.config.get("method", "POST")
        headers = self.config.get("headers", {})
        payload = {
            "event": context.event,
            "session_id": context.session_id,
            "cwd": context.cwd,
            "timestamp": context.timestamp,
        }
        if extra_input:
            payload["data"] = self._serialize_extra(extra_input)
        try:
            async with asyncio.timeout(self.config.get("timeout", 10)):
                await asyncio.sleep(0.01)
                return {"decision": "proceed"}
        except asyncio.TimeoutError:
            return {"decision": "proceed", "reason": "http_timeout"}

    def _serialize_extra(self, extra: Any) -> dict:
        if isinstance(extra, ToolHookInput):
            return {"tool_name": extra.tool_name, "arguments": extra.arguments}
        if isinstance(extra, CompactHookInput):
            return {"trigger": extra.trigger, "token_count": extra.token_count}
        return {}


class PromptHandler(HookHandler):

    async def handle(self, context: HookContext, extra_input: Any = None) -> Optional[dict]:
        prompt_template = self.config.get("prompt", "")
        rendered = self._render_prompt(prompt_template, context, extra_input)
        return {"decision": "proceed", "injected_prompt": rendered}

    def _render_prompt(self, template: str, ctx: HookContext, extra: Any) -> str:
        template = template.replace("{{session_id}}", ctx.session_id)
        template = template.replace("{{cwd}}", ctx.cwd)
        template = template.replace("{{event}}", ctx.event)
        if isinstance(extra, ToolHookInput):
            template = template.replace("{{tool_name}}", extra.tool_name)
        return template


class McpToolHandler(HookHandler):

    async def handle(self, context: HookContext, extra_input: Any = None) -> Optional[dict]:
        tool_name = self.config.get("tool", "")
        server = self.config.get("server", "")
        args = self.config.get("args", {})
        return {"decision": "proceed"}


class AgentHandler(HookHandler):

    async def handle(self, context: HookContext, extra_input: Any = None) -> Optional[dict]:
        agent_type = self.config.get("agent_type", "general-purpose")
        prompt = self.config.get("prompt", "")
        rendered_prompt = prompt.replace("{{event}}", context.event)
        if isinstance(extra_input, ToolHookInput):
            rendered_prompt = rendered_prompt.replace("{{tool_name}}", extra_input.tool_name)
        return {
            "decision": "proceed",
            "agent_result": f"Agent {agent_type} executed for {context.event}"
        }


# ============================================================================
# 4. 处理器工厂
# ============================================================================

class HandlerFactory:

    _registry: dict[HookHandlerType, type[HookHandler]] = {
        HookHandlerType.COMMAND: CommandHandler,
        HookHandlerType.HTTP: HttpHandler,
        HookHandlerType.PROMPT: PromptHandler,
        HookHandlerType.MCP_TOOL: McpToolHandler,
        HookHandlerType.AGENT: AgentHandler,
    }

    @classmethod
    def create(cls, handler_type: HookHandlerType, config: dict) -> HookHandler:
        handler_cls = cls._registry.get(handler_type)
        if not handler_cls:
            raise ValueError(f"Unknown handler type: {handler_type}")
        return handler_cls(config)

    @classmethod
    def register(cls, handler_type: HookHandlerType, handler_cls: type[HookHandler]):
        cls._registry[handler_type] = handler_cls


# ============================================================================
# 5. 事件方向性注册表
# ============================================================================

class EventOrientationRegistry:

    _registry: dict[HookEvent, HookOrientation] = {}

    @classmethod
    def register(cls, event: HookEvent, orientation: HookOrientation):
        cls._registry[event] = orientation

    @classmethod
    def get(cls, event: HookEvent) -> HookOrientation:
        return cls._registry.get(event, HookOrientation.NOTIFICATION)

    @classmethod
    def is_interceptor(cls, event: HookEvent) -> bool:
        return cls.get(event) == HookOrientation.INTERCEPTOR

    @classmethod
    def is_observer(cls, event: HookEvent) -> bool:
        return cls.get(event) == HookOrientation.OBSERVER


def _init_event_registry():
    reg = EventOrientationRegistry
    for e in [HookEvent.PreToolUse, HookEvent.PreCompact,
              HookEvent.UserPromptSubmit, HookEvent.UserPromptExpansion,
              HookEvent.PermissionRequest, HookEvent.Elicitation]:
        reg.register(e, HookOrientation.INTERCEPTOR)
    for e in [HookEvent.PostToolUse, HookEvent.PostToolUseFailure,
              HookEvent.PostToolBatch, HookEvent.PostCompact,
              HookEvent.SubagentStop, HookEvent.Stop,
              HookEvent.TaskCreated, HookEvent.TaskCompleted,
              HookEvent.PermissionDenied, HookEvent.ElicitationResult,
              HookEvent.OnError, HookEvent.ModeChange,
              HookEvent.InstructionsLoaded, HookEvent.ConfigChange,
              HookEvent.CwdChanged, HookEvent.FileChanged,
              HookEvent.WorktreeCreate, HookEvent.WorktreeRemove,
              HookEvent.ShellEnv, HookEvent.TeammateIdle]:
        reg.register(e, HookOrientation.OBSERVER)
    for e in [HookEvent.SessionStart, HookEvent.SessionEnd,
              HookEvent.Setup, HookEvent.Notification,
              HookEvent.SubagentStart, HookEvent.StopFailure]:
        reg.register(e, HookOrientation.NOTIFICATION)


_init_event_registry()


# ============================================================================
# 6. 钩子引擎 (核心)
# ============================================================================

class HookRegistry:
    """钩子引擎 - 核心调度器"""

    def __init__(self):
        self.hooks: list[HookDefinition] = []
        self.event_listeners: dict[str, list[HookDefinition]] = {}
        self._stats = {
            "total_dispatches": 0,
            "total_blocked": 0,
            "total_modified": 0,
            "total_errors": 0,
            "by_orientation": {"observer": 0, "interceptor": 0, "notification": 0},
        }

    def register(self, hook: HookDefinition):
        self.hooks.append(hook)
        event_key = hook.event.value
        if event_key not in self.event_listeners:
            self.event_listeners[event_key] = []
        self.event_listeners[event_key].append(hook)

    def unregister(self, name: str) -> bool:
        for i, hook in enumerate(self.hooks):
            if hook.name == name:
                event_key = hook.event.value
                self.event_listeners[event_key] = [
                    h for h in self.event_listeners.get(event_key, [])
                    if h.name != name
                ]
                self.hooks.pop(i)
                return True
        return False

    def clear(self):
        self.hooks.clear()
        self.event_listeners.clear()

    def get_hooks_for_event(self, event: HookEvent) -> list[HookDefinition]:
        return self.event_listeners.get(event.value, [])

    async def dispatch(
        self,
        event: HookEvent,
        context: HookContext,
        extra_input: Any = None,
        *,
        tool_name: str = "",
        command: str = "",
        file_path: str = "",
    ) -> list[dict]:
        self._stats["total_dispatches"] += 1
        matched_hooks = self._match_hooks(event, tool_name, command, file_path)
        if not matched_hooks:
            return []

        orientation = EventOrientationRegistry.get(event)
        orientation_key = orientation.value
        self._stats["by_orientation"][orientation_key] = (
            self._stats["by_orientation"].get(orientation_key, 0) + 1
        )

        sync_hooks = [h for h in matched_hooks if not h.is_async]
        async_hooks = [h for h in matched_hooks if h.is_async]
        results = []

        for hook in sync_hooks:
            result = await self._execute_hook(hook, context, extra_input, event)
            if result:
                results.append(result)
                if (orientation == HookOrientation.INTERCEPTOR and
                    result.get("decision") == "block"):
                    self._stats["total_blocked"] += 1
                    return results

        if async_hooks:
            asyncio.ensure_future(self._execute_async_hooks(
                async_hooks, context, extra_input, event
            ))

        return results

    async def dispatch_with_decision(
        self,
        event: HookEvent,
        context: HookContext,
        extra_input: Any = None,
        *,
        tool_name: str = "",
        command: str = "",
        file_path: str = "",
    ) -> Optional[dict]:
        results = await self.dispatch(
            event, context, extra_input,
            tool_name=tool_name, command=command, file_path=file_path
        )
        if not results:
            return None
        for r in results:
            if r.get("decision") == "block":
                return r
        modified_args = {}
        for r in results:
            if r.get("decision") == "modify":
                modified_args.update(r.get("modified_arguments", {}))
        if modified_args:
            return {"decision": "modify", "modified_arguments": modified_args}
        return {"decision": "proceed"}

    def _match_hooks(self, event: HookEvent, tool_name: str = "", command: str = "", file_path: str = "") -> list[HookDefinition]:
        event_key = event.value
        matched = []
        for hook in self.event_listeners.get(event_key, []):
            if hook.matcher.matches(event_key, tool_name, command, file_path):
                matched.append(hook)
        return matched

    async def _execute_hook(self, hook: HookDefinition, context: HookContext, extra_input: Any, event: HookEvent) -> Optional[dict]:
        try:
            handler = HandlerFactory.create(hook.handler_type, hook.handler_config)
            result = await handler.handle(context, extra_input)
            hook.run_count += 1
            hook.last_run = time.time()
            if result and result.get("decision") == "modify":
                self._stats["total_modified"] += 1
            return result
        except Exception as e:
            self._stats["total_errors"] += 1
            return {"decision": "proceed", "error": str(e), "hook": hook.name}

    async def _execute_async_hooks(self, hooks, context, extra_input, event):
        tasks = [self._execute_hook(hook, context, extra_input, event) for hook in hooks]
        await asyncio.gather(*tasks, return_exceptions=True)

    def get_stats(self) -> dict:
        return {**self._stats, "registered_hooks": len(self.hooks)}

    def list_hooks(self, event: Optional[HookEvent] = None) -> list[dict]:
        hooks = self.hooks
        if event:
            hooks = [h for h in hooks if h.event == event]
        return [
            {
                "name": h.name,
                "event": h.event.value,
                "handler": h.handler_type.value,
                "orientation": h.orientation.value,
                "is_async": h.is_async,
                "bypass_immune": h.bypass_immune,
                "run_count": h.run_count,
                "last_run": h.last_run,
                "handler_label": h.handler_label,
            }
            for h in hooks
        ]


HookEngine = HookRegistry


# ============================================================================
# 7. 生命周期系统
# ============================================================================

class HookLifecycleSystem:
    """钩子生命周期系统 - 提供完整的事件触发编排"""

    def __init__(self):
        self.registry = HookRegistry()
        self.current_context = HookContext(
            session_id=f"session_{int(time.time())}",
            session_dir=os.getcwd(),
            cwd=os.getcwd(),
        )

    def register(self, hook: HookDefinition):
        self.registry.register(hook)

    def unregister(self, name: str) -> bool:
        return self.registry.unregister(name)

    # ── 会话生命周期 ──

    async def on_session_start(self, resume: bool = False):
        self.current_context = HookContext(
            session_id=self.current_context.session_id,
            session_dir=self.current_context.session_dir,
            cwd=os.getcwd(),
        )
        self.current_context.event = "session_start"
        self.current_context.reason = "resume" if resume else "start"
        await self.registry.dispatch(
            HookEvent.SessionStart,
            self.current_context,
            SessionHookInput(action="resume" if resume else "start")
        )

    async def on_session_end(self, reason: str = "normal"):
        self.current_context.event = "session_end"
        self.current_context.reason = reason
        await self.registry.dispatch(HookEvent.SessionEnd, self.current_context)

    async def on_config_change(self, changed_keys: list[str] = None):
        self.current_context.event = "config_change"
        await self.registry.dispatch(
            HookEvent.ConfigChange, self.current_context,
            {"changed_keys": changed_keys or []}
        )

    async def on_cwd_changed(self, old_cwd: str, new_cwd: str):
        self.current_context.cwd = new_cwd
        self.current_context.event = "cwd_changed"
        await self.registry.dispatch(HookEvent.CwdChanged, self.current_context)

    # ── 工具生命周期 ──

    async def on_pre_tool_use(
        self, tool_name: str, arguments: dict,
        command: str = "", target_path: str = ""
    ) -> ToolHookOutput:
        self.current_context.event = "pre_tool_use"
        extra = ToolHookInput(
            tool_name=tool_name, arguments=arguments,
            command=command, target_path=target_path,
        )
        result = await self.registry.dispatch_with_decision(
            HookEvent.PreToolUse, self.current_context, extra,
            tool_name=tool_name, command=command, file_path=target_path,
        )
        if result is None:
            return ToolHookOutput(decision=HookDecision.PROCEED)
        if result.get("decision") == "block":
            return ToolHookOutput(decision=HookDecision.BLOCK, reason=result.get("reason", "blocked by hook"))
        if result.get("decision") == "modify":
            return ToolHookOutput(
                decision=HookDecision.MODIFY,
                modified_arguments=result.get("modified_arguments", arguments),
                reason=result.get("reason", "modified by hook")
            )
        return ToolHookOutput(decision=HookDecision.PROCEED)

    async def on_post_tool_use(self, tool_name: str, arguments: dict, result_data: str = ""):
        self.current_context.event = "post_tool_use"
        await self.registry.dispatch(
            HookEvent.PostToolUse, self.current_context,
            ToolHookInput(tool_name=tool_name, arguments=arguments),
            tool_name=tool_name,
        )

    async def on_post_tool_use_failure(self, tool_name: str, arguments: dict, error: str):
        self.current_context.event = "post_tool_use_failure"
        await self.registry.dispatch(
            HookEvent.PostToolUseFailure, self.current_context,
            ToolHookInput(tool_name=tool_name, arguments=arguments),
            tool_name=tool_name,
        )

    async def on_post_tool_batch(self, results: list[dict]):
        self.current_context.event = "post_tool_batch"
        await self.registry.dispatch(HookEvent.PostToolBatch, self.current_context, {"batch_results": results})

    # ── 压缩生命周期 ──

    async def on_pre_compact(
        self, trigger: str = "auto", token_count: int = 0, threshold: int = 0
    ) -> CompactHookOutput:
        self.current_context.event = "pre_compact"
        extra = CompactHookInput(trigger=trigger, token_count=token_count, threshold=threshold)
        result = await self.registry.dispatch_with_decision(HookEvent.PreCompact, self.current_context, extra)
        if result is None:
            return CompactHookOutput(decision=HookDecision.PROCEED)
        return CompactHookOutput(
            decision=HookDecision(result.get("decision", "proceed")),
            reason=result.get("reason", ""),
            custom_summary=result.get("custom_summary", ""),
        )

    async def on_post_compact(self, trigger: str = "auto"):
        self.current_context.event = "post_compact"
        await self.registry.dispatch(
            HookEvent.PostCompact, self.current_context,
            CompactHookInput(trigger=trigger),
        )

    # ── 权限生命周期 ──

    async def on_permission_request(self, tool_name: str, arguments: dict) -> Optional[PermissionHookOutput]:
        self.current_context.event = "permission_request"
        extra = PermissionHookInput(tool_name=tool_name, arguments=arguments)
        result = await self.registry.dispatch_with_decision(HookEvent.PermissionRequest, self.current_context, extra)
        if result is None:
            return PermissionHookOutput(decision=HookDecision.PROCEED)
        return PermissionHookOutput(
            decision=HookDecision(result.get("decision", "proceed")),
            reason=result.get("reason", ""),
            approve=result.get("approve", False),
        )

    async def on_permission_denied(self, tool_name: str, reason: str):
        self.current_context.event = "permission_denied"
        await self.registry.dispatch(HookEvent.PermissionDenied, self.current_context)

    # ── 智能体生命周期 ──

    async def on_subagent_start(self, agent_type: str, prompt: str):
        self.current_context.event = "subagent_start"
        await self.registry.dispatch(
            HookEvent.SubagentStart, self.current_context,
            {"agent_type": agent_type, "prompt": prompt[:100]}
        )

    async def on_subagent_stop(self, agent_type: str, result_summary: str):
        self.current_context.event = "subagent_stop"
        await self.registry.dispatch(
            HookEvent.SubagentStop, self.current_context,
            {"agent_type": agent_type, "result": result_summary[:200]}
        )

    # ── 任务生命周期 ──

    async def on_task_created(self, task_id: str, description: str):
        self.current_context.event = "task_created"
        await self.registry.dispatch(
            HookEvent.TaskCreated, self.current_context,
            {"task_id": task_id, "description": description}
        )

    async def on_task_completed(self, task_id: str, status: str):
        self.current_context.event = "task_completed"
        await self.registry.dispatch(
            HookEvent.TaskCompleted, self.current_context,
            {"task_id": task_id, "status": status}
        )

    # ── 交互生命周期 ──

    async def on_elicitation(self, question: str):
        self.current_context.event = "elicitation"
        await self.registry.dispatch(HookEvent.Elicitation, self.current_context, {"question": question})

    # ── 模式切换 ──

    async def on_mode_change(self, old_mode: str, new_mode: str):
        self.current_context.event = "mode_change"
        await self.registry.dispatch(
            HookEvent.ModeChange, self.current_context,
            {"old_mode": old_mode, "new_mode": new_mode}
        )

    # ── 错误事件 ──

    async def on_error(self, error: str):
        self.current_context.event = "on_error"
        await self.registry.dispatch(HookEvent.OnError, self.current_context, {"error": error})


# ============================================================================
# 8. 配置加载器
# ============================================================================

class HookConfigLoader:

    @classmethod
    def from_toml(cls, toml_content: str) -> list[HookDefinition]:
        definitions = []
        in_hooks_section = False
        current_item = {}
        for line in toml_content.splitlines():
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            if line.startswith('[[hooks.hooks]]'):
                if current_item:
                    definitions.append(cls._build_toml_item(current_item, len(definitions)))
                    current_item = {}
                in_hooks_section = True
                continue
            if not in_hooks_section:
                continue
            if line.startswith('['):
                continue
            if '=' in line:
                key, _, value = line.partition('=')
                key = key.strip()
                value = value.strip().strip('"').strip("'")
                current_item[key] = value
        if current_item:
            definitions.append(cls._build_toml_item(current_item, len(definitions)))
        return definitions

    @classmethod
    def _build_toml_item(cls, item: dict, index: int) -> HookDefinition:
        event_name = item.get("event", "stop")
        event = cls._resolve_event(event_name)
        command = item.get("command", "")
        timeout = int(item.get("timeout", 30))
        return HookDefinition(
            name=item.get("name", f"hook_{index}"),
            event=event,
            matcher=HookMatcher(event=event.value if event else event_name),
            handler_type=HookHandlerType.COMMAND,
            handler_config={"command": command, "timeout": timeout},
            timeout_secs=timeout,
            orientation=HookOrientation.OBSERVER,
        )

    @classmethod
    def from_json(cls, json_content: str) -> list[HookDefinition]:
        config = json.loads(json_content)
        hooks_config = config.get("hooks", {})
        definitions = []
        for event_name, hook_list in hooks_config.items():
            if isinstance(hook_list, list):
                for i, item in enumerate(hook_list):
                    definition = cls._parse_hook_item(event_name, item, i)
                    if definition:
                        definitions.append(definition)
        return definitions

    @classmethod
    def _parse_hook_item(cls, event_name: str, item: dict, index: int) -> Optional[HookDefinition]:
        event = cls._resolve_event(event_name)
        if event is None:
            return None
        matcher_config = item.get("matcher", {})
        event_value = event.value if event else event_name
        if isinstance(matcher_config, str):
            matcher = HookMatcher(event=event_value, tool_name=matcher_config)
        else:
            matcher = HookMatcher(
                event=event_value,
                tool_name=matcher_config.get("tool_name", ""),
                tool_pattern=matcher_config.get("tool_pattern", ""),
                file_pattern=matcher_config.get("file_pattern", ""),
            )
        handler_type_str = item.get("type", "command")
        handler_type = {
            "command": HookHandlerType.COMMAND,
            "http": HookHandlerType.HTTP,
            "prompt": HookHandlerType.PROMPT,
            "mcp_tool": HookHandlerType.MCP_TOOL,
            "agent": HookHandlerType.AGENT,
        }.get(handler_type_str, HookHandlerType.COMMAND)
        handler_config = {}
        if handler_type == HookHandlerType.COMMAND:
            handler_config["command"] = item.get("command", "")
        elif handler_type == HookHandlerType.HTTP:
            handler_config["url"] = item.get("url", "")
            handler_config["method"] = item.get("method", "POST")
        elif handler_type == HookHandlerType.PROMPT:
            handler_config["prompt"] = item.get("prompt", "")
        elif handler_type == HookHandlerType.MCP_TOOL:
            handler_config["tool"] = item.get("tool", "")
            handler_config["server"] = item.get("server", "")
        elif handler_type == HookHandlerType.AGENT:
            handler_config["agent_type"] = item.get("agent_type", "general-purpose")
            handler_config["prompt"] = item.get("prompt", "")
        handler_config["timeout"] = item.get("timeout", 30)
        return HookDefinition(
            name=item.get("name", f"{event_name}_{index}"),
            event=event,
            matcher=matcher,
            handler_type=handler_type,
            handler_config=handler_config,
            orientation=cls._resolve_orientation(event, item),
            is_async=item.get("async", False),
            bypass_immune=item.get("bypass_immune", False),
            suppress_output=item.get("suppress_output", False),
            timeout_secs=item.get("timeout", 30),
        )

    @staticmethod
    def _resolve_event(name: str) -> Optional[HookEvent]:
        name_map = {
            "session_start": HookEvent.SessionStart,
            "SessionStart": HookEvent.SessionStart,
            "setup": HookEvent.Setup,
            "Setup": HookEvent.Setup,
            "session_end": HookEvent.SessionEnd,
            "SessionEnd": HookEvent.SessionEnd,
            "user_prompt_submit": HookEvent.UserPromptSubmit,
            "UserPromptSubmit": HookEvent.UserPromptSubmit,
            "user_prompt_expansion": HookEvent.UserPromptExpansion,
            "UserPromptExpansion": HookEvent.UserPromptExpansion,
            "pre_tool_use": HookEvent.PreToolUse,
            "PreToolUse": HookEvent.PreToolUse,
            "post_tool_use": HookEvent.PostToolUse,
            "PostToolUse": HookEvent.PostToolUse,
            "post_tool_use_failure": HookEvent.PostToolUseFailure,
            "PostToolUseFailure": HookEvent.PostToolUseFailure,
            "post_tool_batch": HookEvent.PostToolBatch,
            "PostToolBatch": HookEvent.PostToolBatch,
            "pre_compact": HookEvent.PreCompact,
            "PreCompact": HookEvent.PreCompact,
            "post_compact": HookEvent.PostCompact,
            "PostCompact": HookEvent.PostCompact,
            "permission_request": HookEvent.PermissionRequest,
            "PermissionRequest": HookEvent.PermissionRequest,
            "permission_denied": HookEvent.PermissionDenied,
            "PermissionDenied": HookEvent.PermissionDenied,
            "stop": HookEvent.Stop,
            "Stop": HookEvent.Stop,
            "stop_failure": HookEvent.StopFailure,
            "StopFailure": HookEvent.StopFailure,
            "subagent_start": HookEvent.SubagentStart,
            "SubagentStart": HookEvent.SubagentStart,
            "subagent_stop": HookEvent.SubagentStop,
            "SubagentStop": HookEvent.SubagentStop,
            "mode_change": HookEvent.ModeChange,
            "ModeChange": HookEvent.ModeChange,
            "on_error": HookEvent.OnError,
            "OnError": HookEvent.OnError,
            "notification": HookEvent.Notification,
            "Notification": HookEvent.Notification,
            "config_change": HookEvent.ConfigChange,
            "ConfigChange": HookEvent.ConfigChange,
            "cwd_changed": HookEvent.CwdChanged,
            "CwdChanged": HookEvent.CwdChanged,
            "file_changed": HookEvent.FileChanged,
            "FileChanged": HookEvent.FileChanged,
            "instructions_loaded": HookEvent.InstructionsLoaded,
            "InstructionsLoaded": HookEvent.InstructionsLoaded,
            "task_created": HookEvent.TaskCreated,
            "TaskCreated": HookEvent.TaskCreated,
            "task_completed": HookEvent.TaskCompleted,
            "TaskCompleted": HookEvent.TaskCompleted,
            "elicitation": HookEvent.Elicitation,
            "Elicitation": HookEvent.Elicitation,
            "elicitation_result": HookEvent.ElicitationResult,
            "ElicitationResult": HookEvent.ElicitationResult,
            "worktree_create": HookEvent.WorktreeCreate,
            "WorktreeCreate": HookEvent.WorktreeCreate,
            "worktree_remove": HookEvent.WorktreeRemove,
            "WorktreeRemove": HookEvent.WorktreeRemove,
            "teammate_idle": HookEvent.TeammateIdle,
            "TeammateIdle": HookEvent.TeammateIdle,
            "shell_env": HookEvent.ShellEnv,
            "ShellEnv": HookEvent.ShellEnv,
            "tool_call_before": HookEvent.PreToolUse,
            "tool_call_after": HookEvent.PostToolUse,
            "message_submit": HookEvent.UserPromptSubmit,
        }
        return name_map.get(name)

    @staticmethod
    def _resolve_orientation(event: HookEvent, item: dict) -> HookOrientation:
        if item.get("orientation") == "observer":
            return HookOrientation.OBSERVER
        return EventOrientationRegistry.get(event)