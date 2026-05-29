"""
Diff-based /flow pipeline: implement -> git diff -> review -> fix -> re-review

Inspired by Claude Code + Codex workflow:
  IMPLEMENTER writes code
  -> git diff captures exact changes
  -> REVIEW model reviews the diff (not whole files)
  -> findings -> IMPLEMENTER fixes
  -> re-review just the fix diff
  -> loop <= 3 rounds or LGTM
"""

from __future__ import annotations

import asyncio
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from langchain_openai import ChatOpenAI

from langchain_core.messages import SystemMessage, HumanMessage

from ..core.cancellation import CancellationToken
from ..agent.subagent_manager import SubAgentManager, SubAgent, SubAgentStatus
from ..agent.subagent_roles import RoleType, get_role
from ..agent.subagent_runtime import AgentContext, _run_agent_loop
from ..agent.skill_system import SkillRegistry
from ..core.event_bus import EventBus, EventType
from ..tools.tools import get_tools_by_names

try:
    from ..security.approval import ToolApprovalSystem
    _HAS_APPROVAL_SYSTEM = True
except ImportError:
    ToolApprovalSystem = None
    _HAS_APPROVAL_SYSTEM = False

try:
    from ..hooks.lifecycle import HookLifecycleSystem
    _HAS_HOOK_SYSTEM = True
except ImportError:
    HookLifecycleSystem = None
    _HAS_HOOK_SYSTEM = False


@dataclass
class ReviewFinding:
    severity: str
    file_path: str
    line: int | None
    description: str
    suggestion: str


@dataclass
class FlowReport:
    task: str
    success: bool
    iterations: int
    max_iterations: int = 3
    summary: str = ""
    changes_summary: list[str] = field(default_factory=list)
    findings_history: list[list[ReviewFinding]] = field(default_factory=list)


REVIEW_SYSTEM_PROMPT = """You are a code reviewer. Review the following code diff carefully.

You are READ-ONLY — you CANNOT modify, write, delete, or execute any files. Your ONLY job is to read and report issues. The fixer agent will handle all changes.

Check for:
- Correctness: Does the logic make sense? Are there edge cases?
- Security: Any injection risks, hardcoded secrets, unsafe operations?
- Quality: Proper error handling, clear naming, appropriate abstractions?
- Performance: Any obvious inefficiencies?

You MUST output your findings using <findings> tags exactly as shown below:

<findings>
[CRITICAL] path/file.py:42 - description of the issue
  suggestion: how to fix it
[HIGH] path/file.py:88 - another issue
  suggestion: how to fix
</findings>

If everything looks good, you MUST output:
<findings>PASS</findings>
"""


COMPLEX_TASK_PATTERNS = re.compile(
    r"\b("
    r"implement|write|create|add|fix|refactor|build|develop|design|"
    r"修改|实现|创建|编写|开发|重构|修复|添加|新增|"
    r"function|class|method|component|module|api|endpoint|route|"
    r"test|testcase|unittest|pytest|"
    r"feature|enhancement|change|update|migrate|upgrade|"
    r"feature|migration"
    r")\b",
    re.IGNORECASE,
)


def is_complex_task(task: str) -> bool:
    return bool(COMPLEX_TASK_PATTERNS.search(task))


# ── Independent functions for runtime mid-flow review ──

MUTATION_TOOLS = frozenset({
    "write_file", "delete_file", "move_file", "copy_file", "file_edit",
    "apply_diff", "edit", "write", "git_commit",
    "execute_command", "bash", "shell", "run", "terminal", "exec",
})


def has_mutation_tools(tool_calls: list[dict]) -> bool:
    for tc in tool_calls:
        name = tc.get("name", tc.get("function", {}).get("name", "")).lower()
        if name in MUTATION_TOOLS:
            return True
    return False


def get_git_diff(max_chars: int = 10000) -> str:
    try:
        result = subprocess.run(
            ["git", "diff"],
            capture_output=True, text=True, cwd=Path.cwd(),
            timeout=10,
        )
        return result.stdout[:max_chars] or ""
    except Exception:
        return ""


def parse_review_output(output: str) -> list[ReviewFinding]:
    tag_match = re.search(
        r"<findings>\s*(.*?)\s*</findings>", output, re.DOTALL | re.IGNORECASE
    )
    if tag_match:
        content = tag_match.group(1).strip()
        if content.upper() == "PASS":
            return []
        findings = _parse_findings_from_text(content)
        if findings:
            return findings
    return _parse_findings_from_text(output)


def _parse_findings_from_text(text: str) -> list[ReviewFinding]:
    findings = []
    pattern = re.compile(
        r"\[(CRITICAL|HIGH|MEDIUM|LOW)\]\s*"
        r"([^:]+?)(?::(\d+))?\s*-\s*(.+)"
        r"(?:\n\s*(?:建议|suggestion):\s*(.+))?",
        re.IGNORECASE,
    )
    for line in text.split("\n"):
        m = pattern.search(line.strip())
        if m:
            line_num = int(m.group(3)) if m.group(3) else None
            suggestion = m.group(5) or ""
            findings.append(ReviewFinding(
                severity=m.group(1).lower(),
                file_path=m.group(2).strip(),
                line=line_num,
                description=m.group(4).strip(),
                suggestion=suggestion.strip(),
            ))
    return findings


def _format_findings_text(findings: list[ReviewFinding]) -> str:
    return "\n".join(
        f"[{f.severity.upper()}] {f.file_path}:{f.line or '?'} - {f.description}\n"
        f"  suggestion: {f.suggestion}"
        for f in findings
    )


async def run_mid_flow_review(
    task_context: str,
    diff: str,
    review_llm: ChatOpenAI,
) -> list[ReviewFinding]:
    if not diff.strip():
        return []
    prompt = (
        f"## Context\n{task_context[:500]}\n\n"
        f"## Code Changes (git diff)\n{diff[:5000]}\n\n"
        "Review the diff above. Output your findings in <findings> tags or <findings>PASS</findings> if clean."
    )
    try:
        response = await review_llm.ainvoke([
            SystemMessage(content=REVIEW_SYSTEM_PROMPT),
            HumanMessage(content=prompt),
        ])
        return parse_review_output(response.content)
    except Exception:
        return []


class FlowPipeline:
    MAX_ITERATIONS = 3

    def __init__(
        self,
        impl_llm: ChatOpenAI,
        review_llm: ChatOpenAI,
        manager: SubAgentManager,
        skill_registry: SkillRegistry,
        event_bus: EventBus,
        cancel_token: CancellationToken,
        approval_system: ToolApprovalSystem | None = None,
        hook_system: HookLifecycleSystem | None = None,
    ):
        self.impl_llm = impl_llm
        self.review_llm = review_llm
        self.manager = manager
        self.skill_registry = skill_registry
        self.event_bus = event_bus
        self.cancel_token = cancel_token
        self.approval_system = approval_system
        self.hook_system = hook_system

    async def run(self, task: str) -> FlowReport:
        report = FlowReport(task=task, success=False, iterations=0)

        self._publish(f"[1/3] Implementation phase starting...")
        impl_agent = await self._run_impl(task)
        report.changes_summary.append(impl_agent.output[:500])
        self._publish(f"Implementation complete — {self._count_files_changed()}")

        for iteration in range(1, self.MAX_ITERATIONS + 1):
            report.iterations = iteration

            diff = self._get_git_diff()
            if not diff:
                diff = "(no changes detected)"

            self._publish(f"[{iteration}/3] Review phase starting...")
            review_agent = await self._run_review(task, impl_agent.output, diff, iteration)
            findings = self._parse_review(review_agent.output)
            report.findings_history.append(findings)

            if not findings:
                self._publish(f"Review round {iteration}: PASS — no issues found")
                report.success = True
                break

            finding_summary = "; ".join(
                f"[{f.severity.upper()}] {f.file_path}" for f in findings
            )
            self._publish(f"Review round {iteration}: {len(findings)} issue(s) — {finding_summary}")

            self._publish(f"[{iteration}/3] Fix phase starting...")
            impl_agent = await self._run_fix(task, findings, iteration)

        report.summary = self._build_summary(report)
        return report

    async def _run_impl(self, task: str) -> SubAgent:
        return await self._run_agent(
            phase="implementer",
            llm=self.impl_llm,
            role_type=RoleType.IMPLEMENTER,
            task=task,
        )

    async def _run_review(self, task: str, impl_output: str, diff: str, iteration: int) -> SubAgent:
        review_prompt = (
            f"## Original Task\n{task}\n\n"
            f"## Implementation Summary\n{impl_output[:1000]}\n\n"
            f"## Code Changes (git diff)\n{diff[:5000]}\n\n"
            "Review the diff above. Output your findings in <findings> tags or <findings>PASS</findings> if clean."
        )
        return await self._run_agent(
            phase=f"reviewer_{iteration}",
            llm=self.review_llm,
            role_type=RoleType.REVIEW,
            task=review_prompt,
            system_extra=REVIEW_SYSTEM_PROMPT,
        )

    async def _run_fix(self, task: str, findings: list[ReviewFinding], iteration: int) -> SubAgent:
        findings_text = "\n".join(
            f"[{f.severity.upper()}] {f.file_path}:{f.line or '?'} - {f.description}\n"
            f"  suggestion: {f.suggestion}"
            for f in findings
        )
        fix_prompt = (
            f"## Original Task\n{task}\n\n"
            f"## Review Findings to Fix\n{findings_text}\n\n"
            "Fix each issue above by editing the affected files."
        )
        return await self._run_agent(
            phase=f"fixer_{iteration}",
            llm=self.impl_llm,
            role_type=RoleType.IMPLEMENTER,
            task=fix_prompt,
        )

    async def _run_agent(
        self,
        phase: str,
        llm: ChatOpenAI,
        role_type: RoleType,
        task: str,
        system_extra: str = "",
    ) -> SubAgent:
        role_def = get_role(role_type)
        if system_extra:
            role_def.system_prompt = role_def.system_prompt + "\n\n" + system_extra
        role_def.system_prompt = role_def.system_prompt + f"\n\n---\n{task}\n---"
        tools = get_tools_by_names(role_def.allowed_tools)

        agent = await self.manager.spawn(
            parent_id="flow",
            role_type=role_type,
            task_description=task[:200],
            parent_cancel_token=self.cancel_token,
            spawn_depth=0,
        )

        ctx = AgentContext(
            agent_id=agent.agent_id,
            agent_name=f"flow-{phase}",
            role_type=role_type,
            role_def=role_def,
            cancel_token=agent.cancel_token,
            message_queue=agent.message_queue,
            event_bus=self.event_bus,
            llm=llm,
            tools=tools,
            skill_registry=self.skill_registry,
            subagent_manager=self.manager,
            approval_system=self.approval_system,
            hook_system=self.hook_system,
            depth=0,
        )

        agent.status = SubAgentStatus.RUNNING
        agent.task_handle = asyncio.create_task(self._run_flow_agent(agent, ctx))
        await agent.completion_event.wait()
        return agent

    async def _run_flow_agent(self, agent: SubAgent, ctx: AgentContext) -> None:
        try:
            agent.status = SubAgentStatus.RUNNING
            output = await _run_agent_loop(ctx)
            agent.status = SubAgentStatus.COMPLETED
            agent.output = output
        except asyncio.CancelledError:
            agent.status = SubAgentStatus.CANCELLED
        except Exception as e:
            agent.status = SubAgentStatus.FAILED
            agent.error = str(e)
        finally:
            agent.completion_event.set()

    def _get_git_diff(self) -> str:
        return get_git_diff() or "(no diff output)"

    def _publish(self, message: str):
        if self.event_bus:
            self.event_bus.publish_nowait("flow", EventType.MESSAGE, message)
        print(f"  {message}")

    def _count_files_changed(self) -> str:
        diff = self._get_git_diff()
        if not diff or diff == "(no diff output)":
            return "no files changed"
        lines = [l for l in diff.split("\n") if l.startswith("+++ ")]
        return f"{len(lines)} file(s) changed"

    def _parse_review(self, output: str) -> list[ReviewFinding]:
        return parse_review_output(output)

    def _build_summary(self, report: FlowReport) -> str:
        status = "PASS" if report.success else "PARTIAL (max iterations)"
        icon = "✅" if report.success else "❌"
        lines = [f"{icon} Flow: {status}", f"  Rounds: {report.iterations}/{report.MAX_ITERATIONS}"]
        for i, f in enumerate(report.findings_history, 1):
            count = len(f)
            lines.append(f"  Round {i}: {count} finding(s) {'✅' if count == 0 else ''}")
        return "\n".join(lines)