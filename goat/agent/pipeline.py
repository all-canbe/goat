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
    acceptance_criteria: list[str] = field(default_factory=list)
    ac_satisfied: list[bool] = field(default_factory=list)
    verification_errors: list[str] = field(default_factory=list)
    verification_passed: bool = True


@dataclass
class PlanFirstReport:
    task: str
    original_plan: str
    reviewed_plan: str
    choice: str = ""  # "original" | "reviewed" | "cancelled"
    success: bool = False
    summary: str = ""
    verification_errors: list[str] = field(default_factory=list)
    verification_passed: bool = True


REVIEW_SYSTEM_PROMPT = """You are an adversarial code reviewer. Your default assumption is that the code HAS flaws — your job is to prove otherwise.

You are READ-ONLY — you CANNOT modify, write, delete, or execute any files. Your ONLY job is to read and report issues. The fixer agent will handle all changes.

Adversarial checklist — actively hunt for:
- Design flaws: Does the approach solve the right problem? Are abstractions justified or over-engineered? Would a simpler approach work? Are there hidden assumptions?
- Edge cases: What happens with empty/null/malformed input? What if a dependency fails? What if the network is down or disk is full?
- Failure modes: How could this code fail catastrophically? What's the "worst thing that could happen" and is it handled? Are there silent failures?
- Security: Can this be exploited? Are there injection risks, path traversal, hardcoded secrets, privilege escalation paths?
- Correctness: Does the logic actually work? Are there race conditions, off-by-one errors, type mismatches, incorrect state transitions?
- Quality: Is error handling meaningful or just generic? Are names misleading? Is there dead code or unused imports?
- Performance: Are there N+1 queries, unnecessary allocations, blocking calls in async paths, unbounded resource usage?

You MUST output your findings using <findings> tags exactly as shown below:

<findings>
[CRITICAL] path/file.py:42 - description of the issue
  suggestion: how to fix it
[HIGH] path/file.py:88 - another issue
  suggestion: how to fix
</findings>

Severity guide:
- CRITICAL: Will cause data loss, security breach, or crash in production. MUST fix.
- HIGH: Likely to cause incorrect behavior or significant degradation. Should fix.
- MEDIUM: Could cause issues under specific conditions. Worth fixing.
- LOW: Style, minor robustness, or documentation improvements.

If after thorough analysis you find no real issues, output:
<findings>PASS</findings>

Do NOT output PASS if you haven't genuinely challenged the code. Honest scrutiny is your purpose.
"""


PLAN_GENERATION_PROMPT = """You are an expert software architect and planner. Your task is to create a detailed, actionable implementation plan.

Output your plan in the following structure using <plan> tags:

<plan>
## Overview
Brief summary of the approach (2-3 sentences).

## Steps
For each step, provide:
1. **Step N: Title** — What to do
   - **Action**: Specific actions to take (files to create/modify, commands to run)
   - **Expected Result**: What should happen after this step
   - **Risk**: What could go wrong and how to mitigate

## Dependencies
List any step dependencies (e.g., "Step 3 depends on Step 1 completing first")

## Verification
How to verify the overall implementation is correct (tests to run, manual checks)
</plan>

Rules:
- Be specific: name actual files, functions, commands
- Be realistic: don't skip error handling, edge cases, or cleanup
- Be minimal: don't over-engineer, don't add unnecessary abstractions
- Each step must be independently verifiable
- Output ONLY the plan, no conversational text outside the <plan> tags
"""

PLAN_REVIEW_PROMPT = """You are an expert code reviewer specializing in implementation plans. You review plans for logical soundness, completeness, and risk.

Your task: Review the given implementation plan and produce an IMPROVED version.

Checklist — actively hunt for:
- Logical gaps: Are there missing steps between steps? Can step N succeed without step N-1?
- Edge cases: What about empty input, network failure, concurrent access, large data?
- Over-engineering: Is the plan doing more than necessary? Can steps be simplified?
- Under-engineering: Are error handling, rollback, cleanup addressed?
- Dependency ordering: Are steps in the right order? Are hidden dependencies missed?
- Verification: Is the verification plan testable? Are there acceptance criteria?

Output the improved plan using the exact same <plan> structure:

<plan>
## Overview
[Improved overview]

## Steps
[Improved steps — add missing steps, reorder if needed, remove unnecessary ones]

## Dependencies
[Updated dependencies]

## Verification
[Improved verification]
</plan>

If the original plan is already excellent and needs no changes, output it as-is inside <plan> tags.
Do NOT output any conversational text outside the <plan> tags.
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

SIMPLE_TASK_PATTERNS = re.compile(
    r"^\s*(what|why|when|where|who|how|is|are|can|could|would|"
    r"什么是|为什么|什么时候|怎么|能否|是不是|有没有|"
    r"explain|describe|tell me|show me|what is|what are|"
    r"format|rename|typo|spelling|grammar|"
    r"格式化|重命名|拼写|语法)"
    r".*[?？]?\s*$",
    re.IGNORECASE,
)


def is_complex_task(task: str) -> bool:
    if SIMPLE_TASK_PATTERNS.match(task.strip()):
        return False
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
        gate_callback: callable | None = None,
    ):
        self.impl_llm = impl_llm
        self.review_llm = review_llm
        self.manager = manager
        self.skill_registry = skill_registry
        self.event_bus = event_bus
        self.cancel_token = cancel_token
        self.approval_system = approval_system
        self.hook_system = hook_system
        self.gate_callback = gate_callback
        self._project_context = self._load_project_context()

    async def run(self, task: str) -> FlowReport:
        report = FlowReport(task=task, success=False, iterations=0)

        self._publish(f"[1/3] Implementation phase starting...")
        impl_agent = await self._run_impl(task)
        report.changes_summary.append(impl_agent.output[:500])
        self._publish(f"Implementation complete — {self._count_files_changed()}")

        verify_ok, verify_errors = await self._run_verification()
        report.verification_passed = verify_ok
        report.verification_errors = verify_errors
        if not verify_ok:
            self._publish(f"  ⚠️ Verification failed — {len(verify_errors)} error(s)")
            self._publish(f"  🔧 Auto-fixing verification errors...")
            fix_findings = [ReviewFinding(
                severity="CRITICAL",
                file_path="build",
                line=None,
                description=err,
                suggestion="Fix this error"
            ) for err in verify_errors[:10]]
            impl_agent = await self._run_fix(task, fix_findings, 0)

        if self.gate_callback:
            proceed = await self.gate_callback("impl_complete", {
                "changes": self._count_files_changed(),
                "verified": verify_ok,
                "verify_errors": verify_errors[:3],
            })
            if not proceed:
                self._publish("  ⏸️ User paused after implementation phase")
                report.success = False
                report.summary = "Paused by user after implementation"
                return report

        self._publish(f"Generating acceptance criteria...")
        report.acceptance_criteria = await self._generate_acceptance_criteria(task)
        if report.acceptance_criteria:
            report.ac_satisfied = [False] * len(report.acceptance_criteria)
            self._publish(f"  📋 {len(report.acceptance_criteria)} acceptance criteria defined")
            for i, ac in enumerate(report.acceptance_criteria, 1):
                self._publish(f"    {i}. {ac[:100]}")

        for iteration in range(1, self.MAX_ITERATIONS + 1):
            report.iterations = iteration

            diff = self._get_git_diff()
            if not diff:
                diff = "(no changes detected)"

            self._publish(f"[{iteration}/3] Review phase starting...")
            review_agent = await self._run_review(
                task, impl_agent.output, diff, iteration,
                acceptance_criteria=report.acceptance_criteria,
            )
            findings = self._parse_review(review_agent.output)
            report.findings_history.append(findings)

            if report.acceptance_criteria:
                report.ac_satisfied = self._parse_ac_results(review_agent.output, len(report.acceptance_criteria))

            if not findings:
                self._publish(f"Review round {iteration}: PASS — no issues found")
                report.success = True
                break

            finding_summary = "; ".join(
                f"[{f.severity.upper()}] {f.file_path}" for f in findings
            )
            self._publish(f"Review round {iteration}: {len(findings)} issue(s) — {finding_summary}")

            has_critical = any(f.severity.upper() == "CRITICAL" for f in findings)
            if has_critical:
                self._publish(f"  ⚠️ {sum(1 for f in findings if f.severity.upper() == 'CRITICAL')} CRITICAL issue(s) detected — must fix")

            if findings and self.gate_callback:
                proceed = await self.gate_callback("review_findings", {
                    "iteration": iteration,
                    "count": len(findings),
                    "critical_count": sum(1 for f in findings if f.severity.upper() == "CRITICAL"),
                    "summary": finding_summary,
                })
                if not proceed:
                    self._publish(f"  ⏸️ User paused after review — skipping fix round {iteration}")
                    report.success = False
                    return report

            self._publish(f"[{iteration}/3] Fix phase starting...")
            impl_agent = await self._run_fix(task, findings, iteration)

        if not report.success and any(
            any(f.severity.upper() == "CRITICAL" for f in findings)
            for findings in report.findings_history
        ):
            report.success = False
            report.summary += " [WARNING: Max iterations reached with unresolved CRITICAL issues]"
            self._publish("  ⚠️ CRITICAL issues remain unresolved after max iterations — manual review required")

        report.summary = self._build_summary(report)
        return report

    async def _run_impl(self, task: str) -> SubAgent:
        return await self._run_agent(
            phase="implementer",
            llm=self.impl_llm,
            role_type=RoleType.IMPLEMENTER,
            task=task,
        )

    async def _generate_acceptance_criteria(self, task: str) -> list[str]:
        """Ask the impl LLM to decompose the task into measurable acceptance criteria."""
        prompt = (
            f"Decompose the following task into 3-6 measurable acceptance criteria.\n"
            f"Each criterion must be verifiable (can be checked by reading code or running tests).\n"
            f"Output each criterion on a separate line starting with '- '.\n"
            f"Do NOT output anything else.\n\n"
            f"Task: {task[:800]}"
        )
        try:
            response = await self.impl_llm.ainvoke([HumanMessage(content=prompt)])
            lines = response.content.strip().split("\n")
            criteria = []
            for line in lines:
                line = line.strip()
                if line.startswith("- "):
                    criteria.append(line[2:].strip())
                elif line.startswith("* "):
                    criteria.append(line[2:].strip())
            return [c for c in criteria if len(c) > 5][:6]
        except Exception:
            return []

    async def _run_review(self, task: str, impl_output: str, diff: str, iteration: int,
                          acceptance_criteria: list[str] | None = None) -> SubAgent:
        ac_block = ""
        if acceptance_criteria:
            ac_lines = "\n".join(f"- {ac}" for ac in acceptance_criteria)
            ac_block = (
                f"\n## Acceptance Criteria\n"
                f"Verify if the implementation satisfies each criterion:\n"
                f"{ac_lines}\n\n"
                f"For each criterion, state PASS or FAIL in your output using:\n"
                f"<ac-result>\n"
                f"1. PASS - explanation\n"
                f"2. FAIL - explanation\n"
                f"...\n"
                f"</ac-result>"
            )

        review_prompt = (
            f"## Original Task\n{task}\n\n"
            f"## Implementation Summary\n{impl_output[:1000]}\n\n"
            f"## Code Changes (git diff)\n{diff[:5000]}\n\n"
            f"{ac_block}\n\n"
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
        from copy import copy
        role_def = copy(get_role(role_type))
        if self._project_context:
            role_def.system_prompt = role_def.system_prompt + self._project_context
        # system_extra 不再拼入 System Prompt，而是通过 metadata 传递
        # 由 _run_agent_loop 作为独立 SystemMessage 追加，保持基础 System Prompt 稳定
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
            metadata={"task": task, "system_extra": system_extra},
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

    async def _run_verification(self) -> tuple[bool, list[str]]:
        diff = self._get_git_diff()
        if diff == "(no diff output)" or not diff.strip():
            return True, []

        project_root = Path.cwd()
        errors: list[str] = []

        if (project_root / "package.json").exists():
            try:
                result = subprocess.run(
                    ["npx", "tsc", "--noEmit"],
                    capture_output=True, text=True, cwd=project_root,
                    timeout=30,
                )
                if result.returncode != 0:
                    for line in (result.stdout + result.stderr).split("\n"):
                        line = line.strip()
                        if line and "error" in line.lower():
                            errors.append(line[:200])
            except FileNotFoundError:
                pass
            except subprocess.TimeoutExpired:
                errors.append("TypeScript check timed out after 30s")
            except Exception:
                pass

        elif (project_root / "pyproject.toml").exists() or (project_root / "requirements.txt").exists():
            try:
                result = subprocess.run(
                    ["git", "diff", "--name-only", "--diff-filter=ACM"],
                    capture_output=True, text=True, cwd=project_root,
                    timeout=10,
                )
                py_files = [f.strip() for f in result.stdout.split("\n") if f.strip().endswith(".py")]
                for py_file in py_files[:20]:
                    try:
                        content = (project_root / py_file).read_text(encoding="utf-8")
                        compile(content, py_file, "exec")
                    except SyntaxError as e:
                        errors.append(f"{py_file}:{e.lineno} - {e.msg}")
            except Exception:
                pass

        elif (project_root / "Cargo.toml").exists():
            try:
                result = subprocess.run(
                    ["cargo", "check"],
                    capture_output=True, text=True, cwd=project_root,
                    timeout=30,
                )
                if result.returncode != 0:
                    for line in (result.stdout + result.stderr).split("\n"):
                        line = line.strip()
                        if line and ("error" in line.lower() or "warning" in line.lower()):
                            errors.append(line[:200])
            except FileNotFoundError:
                pass
            except subprocess.TimeoutExpired:
                errors.append("Cargo check timed out after 30s")
            except Exception:
                pass

        return len(errors) == 0, errors[:50]

    @staticmethod
    def _load_project_context() -> str:
        context_file = None
        for fname in ("CLAUDE.md", "goat.md"):
            candidate = Path.cwd() / fname
            if candidate.exists():
                context_file = candidate
                break
        if context_file:
            try:
                content = context_file.read_text(encoding="utf-8")
                return f"\n## Project Context ({context_file.name})\n{content[:2000]}\n"
            except Exception:
                pass
        return ""

    def _parse_review(self, output: str) -> list[ReviewFinding]:
        return parse_review_output(output)

    # ── Plan-First Flow ──

    async def run_plan_first(self, task: str) -> PlanFirstReport:
        """Plan-first flow: generate plan → review plan → user chooses → YOLO execute."""
        report = PlanFirstReport(task=task, original_plan="", reviewed_plan="")

        self._publish("[1/3] Generating implementation plan...")
        report.original_plan = await self._generate_plan(task)
        if not report.original_plan:
            self._publish("  ❌ Plan generation failed")
            report.summary = "Plan generation failed"
            return report
        self._publish("  ✅ Plan generated")

        self._publish("[2/3] Reviewing plan with stronger model...")
        report.reviewed_plan = await self._review_plan(task, report.original_plan)
        if not report.reviewed_plan:
            self._publish("  ⚠️ Plan review failed, using original plan")
            report.reviewed_plan = report.original_plan
        else:
            self._publish("  ✅ Plan reviewed")

        self._publish("[3/3] Waiting for user to choose plan...")

        if self.gate_callback:
            choice = await self.gate_callback("plan_compare", {
                "task": task,
                "original_plan": report.original_plan,
                "reviewed_plan": report.reviewed_plan,
            })
            report.choice = choice
        else:
            report.choice = "reviewed"

        if report.choice == "cancelled":
            self._publish("  ⏸️ User cancelled")
            report.summary = "Cancelled by user"
            return report

        chosen_plan = report.original_plan if report.choice == "original" else report.reviewed_plan
        label = "原始计划" if report.choice == "original" else "审查后计划"
        self._publish(f"  ▶️ 执行: {label}")

        self._publish("Executing plan in YOLO mode...")
        if self.approval_system:
            prev_mode = self.approval_system.context.mode
            self.approval_system.set_mode(
                __import__('goat.security.approval', fromlist=['PermissionMode']).PermissionMode.YOLO
            )
        try:
            exec_prompt = (
                f"Execute the following plan step by step. Do NOT skip any step.\n\n"
                f"{chosen_plan}"
            )
            impl_agent = await self._run_agent(
                phase="plan_executor",
                llm=self.impl_llm,
                role_type=RoleType.IMPLEMENTER,
                task=exec_prompt,
            )
            report.success = True
            report.summary = f"Plan executed successfully ({label})"
            self._publish(f"  ✅ Plan execution complete")
        except Exception as e:
            report.success = False
            report.summary = f"Execution failed: {e}"
            self._publish(f"  ❌ Execution failed: {e}")
        finally:
            if self.approval_system:
                self.approval_system.set_mode(prev_mode)

        verify_ok, verify_errors = await self._run_verification()
        report.verification_passed = verify_ok
        report.verification_errors = verify_errors
        if not verify_ok:
            self._publish(f"  ⚠️ Verification issues: {len(verify_errors)} error(s)")
            for err in verify_errors[:5]:
                self._publish(f"    {err[:120]}")

        return report

    async def _generate_plan(self, task: str) -> str:
        """Generate an implementation plan using impl_llm."""
        prompt = (
            f"Create a detailed implementation plan for the following task.\n\n"
            f"Task: {task}"
        )
        try:
            response = await self.impl_llm.ainvoke([
                SystemMessage(content=PLAN_GENERATION_PROMPT),
                HumanMessage(content=prompt),
            ])
            return self._extract_plan(response.content)
        except Exception:
            return ""

    async def _review_plan(self, task: str, original_plan: str) -> str:
        """Review and improve the plan using review_llm."""
        prompt = (
            f"## Original Task\n{task}\n\n"
            f"## Original Plan\n{original_plan}\n\n"
            "Review the plan above and output an improved version."
        )
        try:
            response = await self.review_llm.ainvoke([
                SystemMessage(content=PLAN_REVIEW_PROMPT),
                HumanMessage(content=prompt),
            ])
            return self._extract_plan(response.content)
        except Exception:
            return ""

    @staticmethod
    def _extract_plan(output: str) -> str:
        """Extract plan content from <plan> tags."""
        m = re.search(r"<plan>\s*(.*?)\s*</plan>", output, re.DOTALL | re.IGNORECASE)
        if m:
            return m.group(1).strip()
        return output.strip()

    @staticmethod
    def _parse_ac_results(output: str, ac_count: int) -> list[bool]:
        """Parse <ac-result> block to determine which ACs pass/fail."""
        m = re.search(r'<ac-result>(.*?)</ac-result>', output, re.DOTALL)
        if not m:
            return [False] * ac_count
        results = []
        for line in m.group(1).strip().split("\n"):
            line = line.strip()
            if line and re.match(r'\d+\.\s*(PASS|FAIL)', line, re.IGNORECASE):
                results.append(line.upper().startswith("1.") or "PASS" in line.split("-")[0].upper())
        return results[:ac_count] if results else [False] * ac_count

    def _build_summary(self, report: FlowReport) -> str:
        status = "PASS" if report.success else "PARTIAL (max iterations)"
        icon = "✅" if report.success else "❌"
        lines = [f"{icon} Flow: {status}", f"  Rounds: {report.iterations}/{report.MAX_ITERATIONS}"]
        for i, f in enumerate(report.findings_history, 1):
            count = len(f)
            lines.append(f"  Round {i}: {count} finding(s) {'✅' if count == 0 else ''}")
        if report.acceptance_criteria:
            ac_pass = sum(report.ac_satisfied) if report.ac_satisfied else 0
            lines.append(f"  Acceptance Criteria: {ac_pass}/{len(report.acceptance_criteria)} ✅")
        return "\n".join(lines)