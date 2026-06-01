"""
工具审批系统 - Plan / Agent / YOLO 三模式

集 Claude Code、DeepSeek TUI、OpenAI Codex 优点于一身

核心设计理念：
1. 多层防御：规则 -> 分类器 -> 钩子 -> 沙箱 -> OS 级强制
2. 模式与审批正交：模式决定行为倾向，审批策略决定交互方式
3. 绕过免疫检查：安全守卫即使在 YOLO 模式下也生效
4. 沙箱隔离：从工作区边界到内核级强制
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable, Optional


# ============================================================================
# 枚举与基础类型
# ============================================================================

class PermissionMode(Enum):
    DEFAULT = "default"
    PLAN = "plan"
    FLOW = "flow"
    ACCEPT_EDITS = "acceptEdits"
    BYPASS = "bypassPermissions"
    AUTO = "auto"
    YOLO = "yolo"


class ApprovalPolicy(Enum):
    SUGGEST = "suggest"
    AUTO = "auto"
    NEVER = "never"


class SandboxLevel(Enum):
    READ_ONLY = "read-only"
    WORKSPACE_WRITE = "workspace-write"
    DANGER_FULL_ACCESS = "danger-full-access"


class ToolCategory(Enum):
    READ = "read"
    WRITE = "write"
    SHELL = "shell"
    NETWORK = "network"
    DESTRUCTIVE = "destructive"
    MCP = "mcp"


class Decision(Enum):
    ALLOW = "allow"
    BLOCK = "block"
    ASK = "ask"
    DEFER = "defer"


@dataclass
class PermissionResult:
    decision: Decision
    source: str = ""
    message: str = ""
    details: dict = field(default_factory=dict)
    bypass_immune: bool = False


@dataclass
class ToolCall:
    name: str
    arguments: dict
    category: ToolCategory = ToolCategory.READ
    input_text: str = ""
    target_path: str = ""
    network_url: str = ""
    command: str = ""
    sandbox_allowed: bool = False


@dataclass
class ToolDefinition:
    name: str
    category: ToolCategory
    description: str = ""
    requires_approval: bool = True
    is_destructive: bool = False
    check_permissions: Optional[Callable] = None


@dataclass
class PermissionContext:
    workspace_dirs: list[str] = field(default_factory=list)
    trusted_dirs: list[str] = field(default_factory=list)
    git_detected: bool = False
    is_sandboxed: bool = True
    sandbox_level: SandboxLevel = SandboxLevel.WORKSPACE_WRITE
    mode: PermissionMode = PermissionMode.DEFAULT
    approval_policy: ApprovalPolicy = ApprovalPolicy.SUGGEST
    is_authenticated: bool = False
    can_bypass: bool = False
    pre_plan_mode: Optional[PermissionMode] = None
    is_auto_mode_available: bool = False


# ============================================================================
# 规则系统
# ============================================================================

@dataclass
class ToolRule:
    pattern: str
    behavior: str
    source: str = ""
    message: str = ""
    bypass_immune: bool = False


class RuleMatcher:

    @staticmethod
    def matches(rule: ToolRule, tool_name: str, args: dict) -> bool:
        if rule.pattern == tool_name:
            return True

        match = re.match(r'^(\w+)\((.+)\)$', rule.pattern)
        if match:
            base_tool = match.group(1)
            arg_pattern = match.group(2)
            if base_tool != tool_name:
                return False
            if arg_pattern == '*':
                return True
            for key, value in args.items():
                if f"{key}:" in arg_pattern and str(value).startswith(arg_pattern.split(':')[1]):
                    return True

        if rule.pattern.startswith('mcp__') and tool_name.startswith('mcp__'):
            return tool_name.startswith(rule.pattern)

        return False


class RuleEngine:

    def __init__(self):
        self.rules: list[ToolRule] = []

    def add_rule(self, rule: ToolRule):
        self.rules.append(rule)

    def add_rules_from_config(self, config: dict):
        for level in ['builtin', 'policy', 'project', 'user']:
            level_rules = config.get(level, [])
            for rule_conf in level_rules:
                self.rules.append(ToolRule(
                    pattern=rule_conf['pattern'],
                    behavior=rule_conf['behavior'],
                    source=level,
                    message=rule_conf.get('message', ''),
                    bypass_immune=rule_conf.get('bypass_immune', False)
                ))

    def find_rule(self, tool_name: str, args: dict) -> Optional[ToolRule]:
        for rule in self.rules:
            if RuleMatcher.matches(rule, tool_name, args):
                return rule
        return None

    def find_deny_rule(self, tool_name: str, args: dict) -> Optional[ToolRule]:
        rule = self.find_rule(tool_name, args)
        if rule and rule.behavior == 'deny':
            return rule
        return None

    def find_allow_rule(self, tool_name: str, args: dict) -> Optional[ToolRule]:
        rule = self.find_rule(tool_name, args)
        if rule and rule.behavior == 'allow':
            return rule
        return None


# ============================================================================
# 沙箱管理器
# ============================================================================

class SandboxManager:

    def __init__(self, level: SandboxLevel = SandboxLevel.WORKSPACE_WRITE, workspace: str | None = None):
        self.level = level
        self.workspace = workspace or os.getcwd()
        self.trusted_dirs: set[str] = set()
        self.trusted_dirs.add(self.workspace)
        self.trusted_dirs.add(os.path.abspath(os.sep + 'tmp') if os.name != 'nt' else os.path.abspath(os.environ.get('TEMP', os.sep + 'tmp')))
        self.network_allowed = False
        self.sandbox_enabled = True
        self.auto_allow_bash_if_sandboxed = False

    def is_path_allowed(self, path: str) -> bool:
        if self.level == SandboxLevel.DANGER_FULL_ACCESS:
            return True
        if self.level == SandboxLevel.READ_ONLY:
            return True
        abs_path = os.path.abspath(path)
        for trusted in self.trusted_dirs:
            if abs_path.startswith(os.path.abspath(trusted)):
                return True
        return False

    def is_write_allowed(self, path: str) -> bool:
        if self.level == SandboxLevel.READ_ONLY:
            return False
        return self.is_path_allowed(path)

    def trust_directory(self, path: str):
        self.trusted_dirs.add(os.path.abspath(path))

    def is_network_allowed(self) -> bool:
        if self.level == SandboxLevel.DANGER_FULL_ACCESS:
            return True
        return self.network_allowed

    def get_platform_sandbox_args(self) -> list[str]:
        system_platform = os.name
        if system_platform == 'posix':
            import platform
            if platform.system() == 'Darwin':
                return self._get_macos_sandbox_args()
            elif platform.system() == 'Linux':
                return self._get_linux_sandbox_args()
        return []

    def _get_macos_sandbox_args(self) -> list[str]:
        if self.level == SandboxLevel.READ_ONLY:
            return ['sandbox-exec', '-p', '(version 1) (deny default) (allow file-read*)']
        elif self.level == SandboxLevel.WORKSPACE_WRITE:
            profile = f'(version 1) (deny default) (allow file-read*) (allow file-write* (subpath "{self.workspace}"))'
            return ['sandbox-exec', '-p', profile]
        return []

    def _get_linux_sandbox_args(self) -> list[str]:
        return []


# ============================================================================
# ML 分类器
# ============================================================================

class MLClassifier:

    def __init__(self):
        self.rules: list[dict] = []

    def add_rule(self, pattern: str, decision: str, confidence: float = 0.9):
        self.rules.append({
            'pattern': pattern,
            'decision': decision,
            'confidence': confidence
        })

    def classify(self, tool_call: ToolCall) -> tuple[str, float]:
        best = ("ask", 0.0)

        for rule in self.rules:
            if re.search(rule['pattern'], tool_call.name, re.IGNORECASE) or \
               re.search(rule['pattern'], tool_call.command, re.IGNORECASE) or \
               re.search(rule['pattern'], tool_call.input_text, re.IGNORECASE):
                if rule['confidence'] > best[1]:
                    best = (rule['decision'], rule['confidence'])

        for pattern, decision, confidence in self._builtin_rules(tool_call):
            if re.search(pattern, tool_call.command, re.IGNORECASE):
                if confidence > best[1]:
                    best = (decision, confidence)

        return best

    def _builtin_rules(self, tool_call: ToolCall) -> list[tuple[str, str, float]]:
        rules = [
            (r'rm\s+(-rf\s+)?\/', 'block', 1.0),
            (r'>\s*/dev/', 'block', 0.95),
            (r':\(\)\s*\{', 'block', 1.0),
            (r'chmod\s+777', 'ask', 0.8),
            (r'curl\s+.*\|\s*bash', 'block', 0.95),
            (r'mv\s+.*\s+/', 'ask', 0.85),
            (r'^(ls|cat|head|tail|wc|grep|find|echo)', 'allow', 0.9),
            (r'^npm\s+(test|run\s+test)', 'allow', 0.85),
            (r'^cargo\s+(test|check|build)', 'allow', 0.85),
            (r'^python\s+(-m\s+)?(pytest|unittest)', 'allow', 0.85),
            (r'^git\s+(status|diff|log|branch)', 'allow', 0.9),
            (r'^npm\s+(install|publish)', 'ask', 0.7),
            (r'^pip\s+install', 'ask', 0.7),
            (r'^cargo\s+publish', 'ask', 0.8),
            (r'^git\s+(commit|push)', 'ask', 0.8),
            (r'^docker\s+(push|build)', 'ask', 0.8),
        ]
        return rules


# ============================================================================
# 安全守卫 - 绕过免疫检查
# ============================================================================

class SafetyGuard:

    PROTECTED_PATHS = [
        '.git',
        '.claude',
        '.vscode',
        '.idea',
        'node_modules/.cache',
        os.path.expanduser('~/.ssh'),
        os.path.expanduser('~/.config'),
        '/etc/passwd',
        '/etc/shadow',
    ]

    PROTECTED_SHELL_CONFIGS = [
        os.path.expanduser('~/.bashrc'),
        os.path.expanduser('~/.zshrc'),
        os.path.expanduser('~/.profile'),
        os.path.expanduser('~/.bash_profile'),
        '/etc/bashrc',
        '/etc/profile',
        os.path.expanduser('~/.ssh/authorized_keys'),
    ]

    DANGEROUS_COMMANDS = [
        r'rm\s+-rf\s+/\s*$',
        r':\(\)\s*\{',
        r'chmod\s+777\s+/',
        r'curl\s+.*\|\s*(ba)?sh',
        r'wget\s+.*\|\s*(ba)?sh',
        r'>\s*/dev/[a-z]+',
        r'mkfs\..*',
        r'dd\s+if=.*of=/dev/',
        r'^sudo\s+rm\s+-rf\s+/',
    ]

    @classmethod
    def check_path(cls, path: str) -> Optional[PermissionResult]:
        abs_path = os.path.abspath(os.path.expanduser(path))
        for protected in cls.PROTECTED_PATHS:
            protected_abs = os.path.abspath(os.path.expanduser(protected))
            if abs_path.startswith(protected_abs) or protected in path:
                return PermissionResult(
                    decision=Decision.BLOCK,
                    source='safety_guard_path',
                    message=f"路径 '{path}' 受安全守卫保护",
                    bypass_immune=True
                )
        for config in cls.PROTECTED_SHELL_CONFIGS:
            if config in path:
                return PermissionResult(
                    decision=Decision.BLOCK,
                    source='safety_guard_shell_config',
                    message=f"Shell 配置 '{path}' 受安全守卫保护",
                    bypass_immune=True
                )
        return None

    @classmethod
    def check_workspace_boundary(cls, target_path: str, workspace: str | None) -> PermissionResult | None:
        if not target_path or not workspace:
            return None
        abs_target = os.path.abspath(os.path.expanduser(target_path))
        abs_workspace = os.path.abspath(os.path.expanduser(workspace))
        if not abs_target.startswith(abs_workspace):
            return PermissionResult(
                decision=Decision.ASK,
                source='workspace_boundary',
                message=f"文件操作位于工作空间之外: {target_path}\n路径: {abs_target}\n工作空间: {abs_workspace}\n是否允许？",
                bypass_immune=True,
            )
        return None

    @classmethod
    def check_command(cls, command: str) -> Optional[PermissionResult]:
        for pattern in cls.DANGEROUS_COMMANDS:
            if re.search(pattern, command.strip()):
                return PermissionResult(
                    decision=Decision.BLOCK,
                    source='safety_guard_dangerous_command',
                    message=f"命令 '{command}' 被安全守卫拦截",
                    bypass_immune=True
                )
        return None

    @classmethod
    def check_tool_implementation(cls, tool_name: str) -> Optional[PermissionResult]:
        return None


# ============================================================================
# 钩子系统（旧版 - 保持向后兼容）
# ============================================================================

class HookEvent(Enum):
    TOOL_CALL_BEFORE = "tool_call_before"
    TOOL_CALL_AFTER = "tool_call_after"
    MODE_CHANGE = "mode_change"
    SESSION_START = "session_start"
    ON_ERROR = "on_error"


class Hook:

    def __init__(self, event: HookEvent, handler: Callable, timeout: int = 30):
        self.event = event
        self.handler = handler
        self.timeout = timeout

    async def execute(self, context: dict) -> Optional[PermissionResult]:
        try:
            result = self.handler(context)
            if isinstance(result, PermissionResult):
                return result
            return None
        except Exception as e:
            return PermissionResult(
                decision=Decision.ASK,
                source='hook_error',
                message=f"钩子执行错误: {e}"
            )


class HookEngine:

    def __init__(self):
        self.hooks: dict[HookEvent, list[Hook]] = {}

    def register(self, event: HookEvent, hook: Hook):
        if event not in self.hooks:
            self.hooks[event] = []
        self.hooks[event].append(hook)

    async def execute(self, event: HookEvent, context: dict) -> Optional[PermissionResult]:
        for hook in self.hooks.get(event, []):
            result = await hook.execute(context)
            if result is not None:
                return result
        return None


# ============================================================================
# 审批管道
# ============================================================================

class ApprovalPipeline:

    def __init__(
        self,
        rule_engine: RuleEngine,
        sandbox: SandboxManager,
        classifier: MLClassifier,
        hook_engine: HookEngine,
    ):
        self.rule_engine = rule_engine
        self.sandbox = sandbox
        self.classifier = classifier
        self.hooks = hook_engine

    async def evaluate(
        self,
        tool_call: ToolCall,
        ctx: PermissionContext
    ) -> PermissionResult:

        current_decision: Optional[PermissionResult] = None

        def update_decision(result: PermissionResult):
            nonlocal current_decision
            if current_decision is None:
                current_decision = result
                return

            if result.bypass_immune:
                current_decision = result
                return

            block_wins = {Decision.ASK, Decision.DEFER}
            allow_wins = {Decision.ASK, Decision.DEFER}
            ask_wins = {Decision.DEFER}

            if result.decision == Decision.BLOCK and current_decision.decision in block_wins:
                current_decision = result
            elif result.decision == Decision.ALLOW and current_decision.decision in allow_wins:
                current_decision = result
            elif result.decision == Decision.ASK and current_decision.decision in ask_wins:
                current_decision = result

        deny_rule = self.rule_engine.find_deny_rule(tool_call.name, tool_call.arguments)
        if deny_rule:
            update_decision(PermissionResult(
                decision=Decision.BLOCK,
                source=f'deny_rule:{deny_rule.source}',
                message=deny_rule.message or f"工具 '{tool_call.name}' 被规则拒绝",
                bypass_immune=deny_rule.bypass_immune
            ))

        ask_rule = self.rule_engine.find_rule(tool_call.name, tool_call.arguments)
        if ask_rule and ask_rule.behavior == 'ask' and current_decision is None:
            update_decision(PermissionResult(
                decision=Decision.ASK,
                source=f'ask_rule:{ask_rule.source}',
                message=ask_rule.message or f"是否允许工具 '{tool_call.name}'?"
            ))

        if current_decision is None or current_decision.decision == Decision.DEFER:
            if tool_call.category == ToolCategory.WRITE and tool_call.target_path:
                if not self.sandbox.is_write_allowed(tool_call.target_path):
                    update_decision(PermissionResult(
                        decision=Decision.BLOCK,
                        source='sandbox_write_check',
                        message=f"沙箱拒绝写操作: {tool_call.target_path}"
                    ))

            if tool_call.category == ToolCategory.SHELL and tool_call.command:
                if not self.sandbox.is_path_allowed(self.sandbox.workspace):
                    update_decision(PermissionResult(
                        decision=Decision.BLOCK,
                        source='sandbox_shell_check',
                        message="沙箱拒绝 shell 操作"
                    ))

            if tool_call.category == ToolCategory.NETWORK:
                if not self.sandbox.is_network_allowed():
                    update_decision(PermissionResult(
                        decision=Decision.BLOCK,
                        source='sandbox_network_check',
                        message="沙箱拒绝网络访问"
                    ))

        impl_result = SafetyGuard.check_tool_implementation(tool_call.name)
        if impl_result:
            update_decision(impl_result)

        if tool_call.category == ToolCategory.DESTRUCTIVE and (
            current_decision is None or current_decision.decision not in (Decision.BLOCK, Decision.ALLOW)
        ):
            update_decision(PermissionResult(
                decision=Decision.ASK,
                source='destructive_ask',
                message=f"破坏性操作 '{tool_call.name}' 需要确认"
            ))

        if tool_call.category == ToolCategory.SHELL and tool_call.command:
            content_ask_patterns = [
                r'^npm\s+(publish|unpublish)',
                r'^cargo\s+publish',
                r'^pip\s+install',
                r'^git\s+(push|commit)',
                r'^docker\s+(push|login)',
            ]
            for pattern in content_ask_patterns:
                if re.search(pattern, tool_call.command.strip()):
                    if current_decision is None or current_decision.decision not in (Decision.BLOCK, Decision.ALLOW):
                        update_decision(PermissionResult(
                            decision=Decision.ASK,
                            source='content_ask_rule',
                            message=f"命令 '{tool_call.command}' 需要确认"
                        ))
                    break

        path_result = SafetyGuard.check_path(tool_call.target_path)
        if path_result:
            update_decision(path_result)

        cmd_result = SafetyGuard.check_command(tool_call.command)
        if cmd_result:
            update_decision(cmd_result)

        workspace_boundary_result = SafetyGuard.check_workspace_boundary(
            tool_call.target_path, self.sandbox.workspace,
        )
        if workspace_boundary_result:
            update_decision(workspace_boundary_result)

        hook_result = await self.hooks.execute(HookEvent.TOOL_CALL_BEFORE, {
            'tool_call': tool_call,
            'context': ctx,
            'current_decision': current_decision,
        })
        if hook_result:
            update_decision(hook_result)

        should_bypass = (
            (ctx.mode == PermissionMode.BYPASS and ctx.can_bypass) or
            (ctx.mode == PermissionMode.YOLO) or
            (ctx.mode == PermissionMode.FLOW) or
            (ctx.mode == PermissionMode.PLAN and ctx.can_bypass)
        )

        if should_bypass and (
            current_decision is None or
            current_decision.decision == Decision.ASK
        ):
            current_decision = PermissionResult(
                decision=Decision.ALLOW,
                source=f'mode:{ctx.mode.value}',
                message=f"被 {ctx.mode.value} 模式自动批准"
            )

        allow_rule = self.rule_engine.find_allow_rule(tool_call.name, tool_call.arguments)
        if allow_rule and (
            current_decision is None or
            current_decision.decision != Decision.BLOCK
        ):
            current_decision = PermissionResult(
                decision=Decision.ALLOW,
                source=f'allow_rule:{allow_rule.source}',
                message=allow_rule.message
            )

        if current_decision is None or current_decision.decision not in (Decision.BLOCK, Decision.ALLOW):
            if ctx.mode == PermissionMode.PLAN:
                if tool_call.category in (ToolCategory.READ,):
                    pass
                else:
                    current_decision = PermissionResult(
                        decision=Decision.BLOCK,
                        source='plan_mode_block',
                        message=f"工具 '{tool_call.name}' 在 Plan 模式下不可用"
                    )

            elif ctx.mode == PermissionMode.ACCEPT_EDITS:
                if tool_call.category == ToolCategory.WRITE:
                    current_decision = PermissionResult(
                        decision=Decision.ALLOW,
                        source='accept_edits_mode',
                        message="编辑被 AcceptEdits 模式自动批准"
                    )

            elif ctx.mode == PermissionMode.AUTO and ctx.is_auto_mode_available:
                decision, confidence = self.classifier.classify(tool_call)
                if confidence > 0.8:
                    current_decision = PermissionResult(
                        decision=Decision(decision),
                        source=f'auto_classifier:{confidence:.2f}',
                        message=f"自动分类为 '{decision}' (置信度: {confidence:.2f})"
                    )

        if current_decision is None:
            if ctx.approval_policy == ApprovalPolicy.AUTO:
                current_decision = PermissionResult(
                    decision=Decision.ALLOW,
                    source='approval_policy:auto',
                    message="被审批策略自动批准"
                )
            elif ctx.approval_policy == ApprovalPolicy.NEVER:
                current_decision = PermissionResult(
                    decision=Decision.BLOCK,
                    source='approval_policy:never',
                    message="被审批策略拒绝: never"
                )
            elif tool_call.category == ToolCategory.READ:
                current_decision = PermissionResult(
                    decision=Decision.ALLOW,
                    source='default_read',
                    message="读操作默认允许"
                )
            else:
                current_decision = PermissionResult(
                    decision=Decision.ASK,
                    source='default_ask',
                    message=f"是否允许 '{tool_call.name}'?"
                )

        return current_decision


# ============================================================================
# 模式管理器
# ============================================================================

class ModeManager:

    def __init__(self):
        self.mode_cycle = [
            PermissionMode.DEFAULT,
            PermissionMode.PLAN,
            PermissionMode.YOLO,
            PermissionMode.FLOW,
        ]
        self.current_index = 0

    @property
    def current_mode(self) -> PermissionMode:
        return self.mode_cycle[self.current_index]

    def next_mode(self) -> PermissionMode:
        self.current_index = (self.current_index + 1) % len(self.mode_cycle)
        return self.current_mode

    def set_mode(self, mode: PermissionMode) -> PermissionMode:
        if mode in self.mode_cycle:
            self.current_index = self.mode_cycle.index(mode)
        return self.current_mode

    def get_mode_behavior(self, mode: PermissionMode) -> dict:
        behaviors = {
            PermissionMode.DEFAULT: {
                'label': 'Default',
                'description': '每次操作都询问',
                'read': 'ask',
                'write': 'ask',
                'shell': 'ask',
                'network': 'ask',
            },
            PermissionMode.PLAN: {
                'label': 'Plan',
                'description': 'Plan 模式 - 只读规划，需审批执行',
                'read': 'allow',
                'write': 'block',
                'shell': 'block',
                'network': 'allow',
            },
            PermissionMode.FLOW: {
                'label': 'Flow',
                'description': '流水线模式: 智能判断 → 实现 → 审查 → 修复闭环',
                'read': 'allow',
                'write': 'allow',
                'shell': 'allow',
                'network': 'allow',
            },
            PermissionMode.ACCEPT_EDITS: {
                'label': 'Accept Edits',
                'description': '文件编辑自动批准，Shell 询问',
                'read': 'allow',
                'write': 'allow',
                'shell': 'ask',
                'network': 'ask',
            },
            PermissionMode.BYPASS: {
                'label': 'Bypass',
                'description': '全部自动批准（安全守卫仍生效）',
                'read': 'allow',
                'write': 'allow',
                'shell': 'allow',
                'network': 'allow',
            },
            PermissionMode.AUTO: {
                'label': 'Auto',
                'description': 'ML 分类器自动决策',
                'read': 'auto',
                'write': 'auto',
                'shell': 'auto',
                'network': 'auto',
            },
            PermissionMode.YOLO: {
                'label': 'YOLO',
                'description': '全部自动批准 + 解除沙箱边界（危险！）',
                'read': 'allow',
                'write': 'allow',
                'shell': 'allow',
                'network': 'allow',
            },
        }
        return behaviors.get(mode, behaviors[PermissionMode.DEFAULT])


# ============================================================================
# 审批交互器
# ============================================================================

class ApprovalInteraction:

    @staticmethod
    def format_tool_call(tool_call: ToolCall) -> str:
        lines = [
            f"Tool: {tool_call.name}",
            f"Category: {tool_call.category.value}",
        ]
        if tool_call.arguments:
            lines.append(f"Arguments: {json.dumps(tool_call.arguments, indent=2)}")
        if tool_call.command:
            lines.append(f"Command: {tool_call.command}")
        if tool_call.target_path:
            lines.append(f"Target: {tool_call.target_path}")
        return "\n".join(lines)

    @staticmethod
    def format_result(result: PermissionResult) -> str:
        icon = {
            Decision.ALLOW: "✅",
            Decision.BLOCK: "❌",
            Decision.ASK: "❓",
            Decision.DEFER: "⏳",
        }.get(result.decision, "❓")

        immune = " [🔒 Bypass-Immune]" if result.bypass_immune else ""
        return f"{icon} {result.decision.value.upper()}{immune} - {result.message} [{result.source}]"


# ============================================================================
# 预设配置系统 - Codex Profiles 风格
# ============================================================================

@dataclass
class Profile:
    name: str
    description: str = ""
    mode: PermissionMode = PermissionMode.DEFAULT
    approval_policy: ApprovalPolicy = ApprovalPolicy.SUGGEST
    sandbox_level: SandboxLevel = SandboxLevel.WORKSPACE_WRITE
    network_access: bool = False
    auto_allow_bash: bool = False
    rules: list[dict] = field(default_factory=list)


class ProfileManager:

    def __init__(self):
        self.profiles: dict[str, Profile] = {}
        self._load_builtin_profiles()

    def _load_builtin_profiles(self):
        self.profiles['safe'] = Profile(
            name='safe',
            description='安全只读浏览模式',
            mode=PermissionMode.PLAN,
            approval_policy=ApprovalPolicy.NEVER,
            sandbox_level=SandboxLevel.READ_ONLY,
        )

        self.profiles['auto'] = Profile(
            name='auto',
            description='自动模式，工作区可写',
            mode=PermissionMode.AUTO,
            approval_policy=ApprovalPolicy.SUGGEST,
            sandbox_level=SandboxLevel.WORKSPACE_WRITE,
        )

        self.profiles['full_auto'] = Profile(
            name='full_auto',
            description='全自动模式，工作区可写',
            mode=PermissionMode.BYPASS,
            approval_policy=ApprovalPolicy.AUTO,
            sandbox_level=SandboxLevel.WORKSPACE_WRITE,
            auto_allow_bash=True,
        )

        self.profiles['yolo'] = Profile(
            name='yolo',
            description='YOLO - 无沙箱，无提示（危险！）',
            mode=PermissionMode.YOLO,
            approval_policy=ApprovalPolicy.AUTO,
            sandbox_level=SandboxLevel.DANGER_FULL_ACCESS,
            network_access=True,
            auto_allow_bash=True,
        )

        self.profiles['agent'] = Profile(
            name='agent',
            description='Agent 模式 - 默认行为',
            mode=PermissionMode.DEFAULT,
            approval_policy=ApprovalPolicy.SUGGEST,
            sandbox_level=SandboxLevel.WORKSPACE_WRITE,
        )

        self.profiles['plan'] = Profile(
            name='plan',
            description='Plan 模式 - 只读调查',
            mode=PermissionMode.PLAN,
            approval_policy=ApprovalPolicy.NEVER,
            sandbox_level=SandboxLevel.READ_ONLY,
        )

        self.profiles['flow'] = Profile(
            name='flow',
            description='Flow 模式 - 流水线闭环',
            mode=PermissionMode.FLOW,
            approval_policy=ApprovalPolicy.AUTO,
            sandbox_level=SandboxLevel.WORKSPACE_WRITE,
        )

    def get_profile(self, name: str) -> Optional[Profile]:
        return self.profiles.get(name)

    def add_profile(self, profile: Profile):
        self.profiles[profile.name] = profile

    def apply_profile(self, name: str, context: PermissionContext) -> bool:
        profile = self.get_profile(name)
        if not profile:
            return False
        context.mode = profile.mode
        context.approval_policy = profile.approval_policy
        context.sandbox_level = profile.sandbox_level
        return True


# ============================================================================
# 主审批系统
# ============================================================================

class ToolApprovalSystem:

    def __init__(self, workspace: str | None = None):
        self.rule_engine = RuleEngine()
        self.sandbox = SandboxManager(workspace=workspace)
        self.classifier = MLClassifier()
        self.hooks = HookEngine()
        self.pipeline = ApprovalPipeline(
            self.rule_engine, self.sandbox, self.classifier, self.hooks
        )
        self.mode_manager = ModeManager()
        self.profile_manager = ProfileManager()
        self.context = PermissionContext()
        self._init_default_rules()

    def _init_default_rules(self):
        builtin_rules = [
            {'pattern': 'Bash(rm -rf /:*)', 'behavior': 'deny', 'bypass_immune': True},
            {'pattern': 'Bash(chmod 777 /:*)', 'behavior': 'deny', 'bypass_immune': True},
            {'pattern': 'Bash(curl *| bash:*)', 'behavior': 'deny', 'bypass_immune': True},
            {'pattern': 'Bash(wget *| bash:*)', 'behavior': 'deny', 'bypass_immune': True},
            {'pattern': 'execute_command', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'async_execute_command', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'write_file', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'file_edit', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'apply_patch', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'delete_file', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'move_file', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'copy_file', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'git_commit', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'git_branch', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'git_stash', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'git_restore', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'git_cherry_pick', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'remember', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'read_file', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'list_files', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'file_grep', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'glob_search', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'search_code', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'get_file_info', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'git_status', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'git_diff', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'git_log', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'git_blame', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'ask_user', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'notify', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'web_search', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'web_fetch', 'behavior': 'allow', 'source': 'builtin'},
            {'pattern': 'web_run', 'behavior': 'ask', 'source': 'builtin'},
            {'pattern': 'web_screenshot', 'behavior': 'ask', 'source': 'builtin'},
        ]
        for rule_conf in builtin_rules:
            self.rule_engine.add_rule(ToolRule(
                pattern=rule_conf['pattern'],
                behavior=rule_conf['behavior'],
                source=rule_conf.get('source', 'builtin'),
                message=rule_conf.get('message', ''),
                bypass_immune=rule_conf.get('bypass_immune', False)
            ))

        self.classifier.add_rule(r'^npm\s+(install|ci)', 'allow', 0.85)
        self.classifier.add_rule(r'^cargo\s+(build|check|test)', 'allow', 0.85)
        self.classifier.add_rule(r'^python\s+-m\s+pytest', 'allow', 0.85)
        self.classifier.add_rule(r'^git\s+(push|commit)', 'ask', 0.75)
        self.classifier.add_rule(r'^npm\s+publish', 'ask', 0.9)
        self.classifier.add_rule(r'^docker\s+(push|login)', 'ask', 0.9)

    def detect_git(self) -> bool:
        try:
            result = subprocess.run(
                ['git', 'rev-parse', '--is-inside-work-tree'],
                capture_output=True, text=True, timeout=5
            )
            self.context.git_detected = result.returncode == 0
            return self.context.git_detected
        except Exception:
            self.context.git_detected = False
            return False

    def auto_configure(self):
        is_git = self.detect_git()
        if is_git:
            self.context.mode = PermissionMode.AUTO
            self.context.sandbox_level = SandboxLevel.WORKSPACE_WRITE
            self.context.approval_policy = ApprovalPolicy.SUGGEST
        else:
            self.context.mode = PermissionMode.PLAN
            self.context.sandbox_level = SandboxLevel.READ_ONLY
            self.context.approval_policy = ApprovalPolicy.NEVER

    def trust_directory(self, path: str):
        self.sandbox.trust_directory(path)

    _MODE_PROFILE_MAP: dict[PermissionMode, str] = {
        PermissionMode.DEFAULT: 'agent',
        PermissionMode.PLAN: 'plan',
        PermissionMode.FLOW: 'flow',
        PermissionMode.YOLO: 'yolo',
        PermissionMode.ACCEPT_EDITS: 'full_auto',
        PermissionMode.BYPASS: 'full_auto',
        PermissionMode.AUTO: 'auto',
    }

    def cycle_mode(self) -> PermissionMode:
        new_mode = self.mode_manager.next_mode()
        self._apply_mode_profile(new_mode)
        return new_mode

    def set_mode(self, mode: PermissionMode) -> PermissionMode:
        self.mode_manager.set_mode(mode)
        self._apply_mode_profile(mode)
        return mode

    def _apply_mode_profile(self, mode: PermissionMode):
        profile_name = self._MODE_PROFILE_MAP.get(mode)
        if profile_name and not self.apply_profile(profile_name):
            self.context.mode = mode
            self.context.approval_policy = ApprovalPolicy.SUGGEST
            self.context.sandbox_level = SandboxLevel.WORKSPACE_WRITE

    def set_approval_policy(self, policy: ApprovalPolicy):
        self.context.approval_policy = policy

    def apply_profile(self, name: str) -> bool:
        profile = self.profile_manager.get_profile(name)
        if not profile:
            return False
        self.context.mode = profile.mode
        self.context.approval_policy = profile.approval_policy
        self.context.sandbox_level = profile.sandbox_level
        self.sandbox.network_allowed = profile.network_access
        self.sandbox.auto_allow_bash_if_sandboxed = profile.auto_allow_bash
        for rule_conf in profile.rules:
            self.rule_engine.add_rule(ToolRule(**rule_conf))
        return True

    def categorize_tool(self, name: str, command: str = "", target_path: str = "") -> ToolCategory:
        name_lower = name.lower()
        read_tools = {
            'list_files', 'read_file', 'search_code', 'file_grep', 'glob_search',
            'get_file_info', 'git_status', 'git_diff', 'git_log', 'git_blame',
            'glob', 'grep', 'read', 'ask_user', 'notify',
        }
        write_tools = {
            'write_file', 'delete_file', 'move_file', 'copy_file',
            'file_edit', 'apply_patch', 'apply_diff', 'edit', 'write',
            'git_commit', 'git_branch', 'git_stash', 'git_cherry_pick',
            'remember',
        }
        shell_tools = {
            'execute_command', 'async_execute_command',
            'bash', 'shell', 'run', 'terminal', 'exec',
        }
        network_tools = {
            'web_search', 'web_fetch', 'web_download', 'web_run', 'web_screenshot',
            'curl', 'wget',
        }
        destructive_tools = {
            'delete_file', 'git_restore', 'rm', 'remove', 'delete', 'format', 'reset',
        }

        if name_lower in destructive_tools:
            return ToolCategory.DESTRUCTIVE
        if name_lower in network_tools:
            return ToolCategory.NETWORK
        if name_lower in shell_tools:
            return ToolCategory.SHELL
        if name_lower in write_tools:
            return ToolCategory.WRITE
        if name_lower in read_tools:
            return ToolCategory.READ

        if command:
            command_stripped = command.strip()
            if command_stripped.startswith(('curl ', 'wget ', 'http ', 'https://')):
                return ToolCategory.NETWORK
            if command_stripped.startswith(('rm ', 'del ', 'rd ', 'rmdir ', 'format ', 'diskpart')):
                return ToolCategory.DESTRUCTIVE
            if command_stripped.startswith(('echo >', 'write ', 'copy ', 'move ', 'ren ')):
                return ToolCategory.WRITE

        if name_lower.startswith('mcp__'):
            return ToolCategory.MCP

        return ToolCategory.READ

    def build_tool_call(
        self,
        name: str,
        arguments: dict = None,
        command: str = "",
        target_path: str = "",
    ) -> ToolCall:
        if arguments is None:
            arguments = {}

        category = self.categorize_tool(
            name,
            command=command or arguments.get("command", ""),
            target_path=target_path or arguments.get("file_path", "")
                                          or arguments.get("filepath", "")
                                          or arguments.get("directory", "")
                                          or arguments.get("path", ""),
        )
        return ToolCall(
            name=name,
            arguments=arguments,
            category=category,
            command=command or arguments.get("command", ""),
            target_path=target_path or arguments.get("file_path", "")
                                     or arguments.get("filepath", "")
                                     or arguments.get("directory", "")
                                     or arguments.get("path", ""),
        )

    async def request_tool_approval(
        self,
        agent_id: str,
        tool_name: str,
        tool_args: dict,
    ) -> PermissionResult:
        tool_call = self.build_tool_call(tool_name, tool_args)
        return await self.pipeline.evaluate(tool_call, self.context)

    async def evaluate_tool_call(
        self,
        name: str,
        arguments: dict = None,
        command: str = "",
        target_path: str = "",
    ) -> PermissionResult:
        if arguments is None:
            arguments = {}
        tool_call = self.build_tool_call(name, arguments, command, target_path)
        return await self.pipeline.evaluate(tool_call, self.context)