//! Flow 流水线 — 实现 → 审查 → 修复
//!
//! 自动化的代码质量闭环（对标 Python 版 pipeline.py）：
//! 1. 让 Agent 实现需求
//! 2. 运行编译验证（tsc/cargo check/compile）
//! 3. 生成验收标准（AC）
//! 4. 生成 git diff
//! 5. 让对抗性 review agent 审查
//! 6. 如果未通过，让实现 agent 修复
//! 7. 最多循环 max_flow_rounds 次
//! 8. Gate 回调在实现完成和审查后暂停等待用户确认

use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};

use crate::agent::react::ReActAgent;
use crate::agent::types::{AgentError, AgentRunResult};
use crate::provider::provider::{ChatMessage, ChatOptions, ChatResponse, MessageContent, Role};

// ============================================================================
// 常量：审查/规划提示词（移植自 Python pipeline.py）
// ============================================================================

/// 对抗性审查系统提示词
pub const REVIEW_SYSTEM_PROMPT: &str = r#"You are an adversarial code reviewer. Your default assumption is that the code HAS flaws — your job is to prove otherwise.

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
"#;

/// 计划生成提示词
pub const PLAN_GENERATION_PROMPT: &str = r#"You are an expert software architect and planner. Your task is to create a detailed, actionable implementation plan.

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
"#;

/// 计划审查提示词
pub const PLAN_REVIEW_PROMPT: &str = r#"You are an expert code reviewer specializing in implementation plans. You review plans for logical soundness, completeness, and risk.

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
"#;

// ============================================================================
// 数据结构
// ============================================================================

/// 审查发现
#[derive(Debug, Clone)]
pub struct ReviewFinding {
    pub severity: String,
    pub file_path: String,
    pub line: Option<usize>,
    pub description: String,
    pub suggestion: String,
}

/// Flow 结果（对齐 Python FlowReport）
#[derive(Debug, Clone)]
pub struct FlowResult {
    pub implementation: AgentRunResult,
    pub review: Option<String>,
    pub findings_history: Vec<Vec<ReviewFinding>>,
    pub fix_rounds: usize,
    pub passed: bool,
    pub final_answer: String,
    pub acceptance_criteria: Vec<String>,
    pub ac_satisfied: Vec<bool>,
    pub verification_passed: bool,
    pub verification_errors: Vec<String>,
    pub changes_summary: Vec<String>,
}

/// Plan-First Flow 结果
#[derive(Debug, Clone)]
pub struct PlanFirstResult {
    pub task: String,
    pub original_plan: String,
    pub reviewed_plan: String,
    pub choice: String,
    pub success: bool,
    pub summary: String,
    pub verification_passed: bool,
    pub verification_errors: Vec<String>,
}

/// Gate 回调阶段
#[derive(Debug, Clone)]
pub enum GatePhase {
    ImplComplete,
    ReviewFindings,
    PlanCompare,
}

/// Gate 回调信息
#[derive(Debug, Clone)]
pub struct GateInfo {
    pub phase: GatePhase,
    pub changes: String,
    pub verified: bool,
    pub verify_errors: Vec<String>,
    pub findings_count: usize,
    pub critical_count: usize,
    pub original_plan: String,
    pub reviewed_plan: String,
}

/// Gate 回调类型 — 返回 "continue"/"pause" (Flow run) 或 "original"/"reviewed"/"cancelled" (PlanFirst)
pub type GateCallback = Arc<dyn Fn(GatePhase, GateInfo) -> String + Send + Sync>;

// ============================================================================
// Flow 流水线
// ============================================================================

/// Flow 流水线
pub struct FlowPipeline {
    implement_agent: Arc<ReActAgent>,
    review_agent: Arc<ReActAgent>,
    max_rounds: usize,
    gate_callback: Option<GateCallback>,
}

impl FlowPipeline {
    pub fn new(implement_agent: Arc<ReActAgent>, review_agent: Arc<ReActAgent>) -> Self {
        let max_rounds = implement_agent.config.max_flow_rounds;
        Self {
            implement_agent,
            review_agent,
            max_rounds,
            gate_callback: None,
        }
    }

    /// 设置 Gate 回调
    pub fn set_gate_callback(&mut self, callback: GateCallback) {
        self.gate_callback = Some(callback);
    }

    /// 运行 Flow 流水线
    pub async fn run(
        &self,
        session_id: &str,
        prompt: &str,
        workspace: &str,
    ) -> Result<FlowResult, AgentError> {
        self.run_with_options(session_id, prompt, workspace, &ChatOptions::default())
            .await
    }

    /// 运行 Flow，携带请求级选项
    pub async fn run_with_options(
        &self,
        session_id: &str,
        prompt: &str,
        workspace: &str,
        options: &ChatOptions,
    ) -> Result<FlowResult, AgentError> {
        info!("Starting Flow pipeline for session {}", session_id);

        // D3: 复用 load_rules，避免与 implement_agent.run 内部 load_rules 重复加载
        let (rules, _) = ReActAgent::load_rules(workspace);
        let project_ctx = rules.join("\n\n");

        // 第一轮：实现
        self.emit_thought(0, "[1] Implementation phase starting...").await;
        let mut impl_result = self
            .implement_agent
            .run_with_options(session_id, prompt, workspace, options)
            .await?;

        let mut changes_summary = Vec::new();
        let file_count = count_files_changed(workspace);
        changes_summary.push(format!("Implementation complete — {}", file_count));

        // 实现后自动验证
        self.emit_thought(0, "[2] Running verification...").await;
        let (verify_ok, verify_errors) = run_verification(workspace).await;
        if !verify_ok {
            self.emit_thought(0, &format!("  ⚠️ Verification failed — {} error(s)", verify_errors.len())).await;
            // 验证失败 → 作为 CRITICAL findings 直接进入修复
            let verify_findings: Vec<ReviewFinding> = verify_errors
                .iter()
                .take(10)
                .map(|e| ReviewFinding {
                    severity: "critical".to_string(),
                    file_path: "build".to_string(),
                    line: None,
                    description: e.clone(),
                    suggestion: "Fix this error".to_string(),
                })
                .collect();
            if !verify_findings.is_empty() {
                self.emit_thought(0, "  🔧 Auto-fixing verification errors...").await;
                impl_result = self.run_fix(session_id, prompt, &verify_findings, 0, workspace, options).await?;
            }
        } else {
            self.emit_thought(0, "  ✅ Verification passed").await;
        }

        // Gate: 实现完成后暂停
        if let Some(ref cb) = self.gate_callback {
            let proceed = cb(
                GatePhase::ImplComplete,
                GateInfo {
                    phase: GatePhase::ImplComplete,
                    changes: file_count.clone(),
                    verified: verify_ok,
                    verify_errors: verify_errors.clone(),
                    findings_count: 0,
                    critical_count: 0,
                    original_plan: String::new(),
                    reviewed_plan: String::new(),
                },
            );
            if proceed == "pause" {
                return Ok(FlowResult {
                    implementation: impl_result,
                    review: None,
                    findings_history: Vec::new(),
                    fix_rounds: 0,
                    passed: false,
                    final_answer: "Paused by user after implementation".to_string(),
                    acceptance_criteria: Vec::new(),
                    ac_satisfied: Vec::new(),
                    verification_passed: verify_ok,
                    verification_errors: verify_errors,
                    changes_summary,
                });
            }
        }

        // 生成验收标准
        self.emit_thought(0, "Generating acceptance criteria...").await;
        let acceptance_criteria = self.generate_acceptance_criteria(prompt, options).await;
        let ac_satisfied = vec![false; acceptance_criteria.len()];
        if !acceptance_criteria.is_empty() {
            self.emit_thought(0, &format!("  📋 {} acceptance criteria defined", acceptance_criteria.len())).await;
        }

        // 审查循环
        let mut findings_history: Vec<Vec<ReviewFinding>> = Vec::new();
        let mut all_ac_satisfied = ac_satisfied.clone();
        let mut passed = false;
        let mut last_review_text = String::new();
        let mut fix_rounds = 0;

        for round in 0..self.max_rounds {
            info!("Flow review round {}/{}", round + 1, self.max_rounds);
            self.emit_thought(round, &format!("[{}/3] Review phase starting...", round + 1, )).await;

            // 获取 git diff
            let diff = get_git_diff(workspace).await;
            let diff = if diff.is_empty() { "(no changes detected)".to_string() } else { diff };

            // 审查
            let review_prompt = self.build_review_prompt(
                prompt,
                &diff,
                &acceptance_criteria,
                &project_ctx,
            );

            let review_session_id = format!("{}-review-{}", session_id, round);
            let review_session = self
                .review_agent
                .conversation
                .create_session(Some(&review_session_id), Some(workspace))
                .await
                .map_err(|e| AgentError::Tool(e.to_string()))?;

            let review_result = self
                .review_agent
                .run_with_options(&review_session.id, &review_prompt, workspace, options)
                .await?;
            let review_text = review_result.answer.clone();
            last_review_text = review_text.clone();

            let findings = parse_review_output(&review_text);
            findings_history.push(findings.clone());

            // 解析 AC 结果
            if !acceptance_criteria.is_empty() {
                all_ac_satisfied = parse_ac_results(&review_text, acceptance_criteria.len());
            }

            if findings.is_empty() {
                self.emit_thought(round, &format!("Review round {}: PASS — no issues found", round + 1)).await;
                passed = true;
                fix_rounds = round;
                break;
            }

            let critical_count = findings
                .iter()
                .filter(|f| f.severity.eq_ignore_ascii_case("critical"))
                .count();
            let finding_summary = findings
                .iter()
                .map(|f| format!("[{}] {}", f.severity.to_uppercase(), f.file_path))
                .collect::<Vec<_>>()
                .join("; ");
            self.emit_thought(round, &format!("Review round {}: {} issue(s) — {}", round + 1, findings.len(), finding_summary)).await;

            if critical_count > 0 {
                self.emit_thought(round, &format!("  ⚠️ {} CRITICAL issue(s) detected — must fix", critical_count)).await;
            }

            // Gate: 审查发现 issues 后暂停
            if let Some(ref cb) = self.gate_callback {
                let proceed = cb(
                    GatePhase::ReviewFindings,
                    GateInfo {
                        phase: GatePhase::ReviewFindings,
                        changes: file_count.clone(),
                        verified: verify_ok,
                        verify_errors: verify_errors.clone(),
                        findings_count: findings.len(),
                        critical_count,
                        original_plan: String::new(),
                        reviewed_plan: String::new(),
                    },
                );
                if proceed == "pause" {
                    self.emit_thought(round, &format!("  ⏸️ User paused after review — skipping fix round {}", round + 1)).await;
                    fix_rounds = round;
                    break;
                }
            }

            // 最后一轮不需要再修复
            if round + 1 >= self.max_rounds {
                warn!("Flow did not pass after {} rounds", self.max_rounds);
                fix_rounds = self.max_rounds;
                break;
            }

            // 修复
            self.emit_thought(round, &format!("[{}/3] Fix phase starting...", round + 1)).await;
            impl_result = self.run_fix(session_id, prompt, &findings, round, workspace, options).await?;
            fix_rounds = round + 1;
        }

        // 检查是否有未解决的 CRITICAL issues
        let has_unresolved_critical = findings_history.iter().any(|findings| {
            findings.iter().any(|f| f.severity.eq_ignore_ascii_case("critical"))
        });
        if !passed && has_unresolved_critical {
            self.emit_thought(self.max_rounds, "  ⚠️ CRITICAL issues remain unresolved after max iterations — manual review required").await;
        }

        let summary = self.build_summary(passed, fix_rounds, &findings_history, &acceptance_criteria, &all_ac_satisfied);

        Ok(FlowResult {
            implementation: impl_result,
            review: Some(last_review_text),
            findings_history,
            fix_rounds,
            passed,
            final_answer: summary,
            acceptance_criteria,
            ac_satisfied: all_ac_satisfied,
            verification_passed: verify_ok,
            verification_errors: verify_errors,
            changes_summary,
        })
    }

    /// Plan-First Flow：生成计划 → 审查计划 → 用户选择 → YOLO 执行
    pub async fn run_plan_first(
        &self,
        session_id: &str,
        prompt: &str,
        workspace: &str,
    ) -> Result<PlanFirstResult, AgentError> {
        self.run_plan_first_with_options(session_id, prompt, workspace, &ChatOptions::default())
            .await
    }

    /// Plan-First Flow with request-level ChatOptions.
    pub async fn run_plan_first_with_options(
        &self,
        session_id: &str,
        prompt: &str,
        workspace: &str,
        options: &ChatOptions,
    ) -> Result<PlanFirstResult, AgentError> {
        info!("Starting Plan-First Flow for session {}", session_id);

        // 1. 生成计划
        self.emit_thought(0, "[1/3] Generating implementation plan...").await;
        let original_plan = self.generate_plan(prompt, options).await?;
        if original_plan.is_empty() {
            return Ok(PlanFirstResult {
                task: prompt.to_string(),
                original_plan: String::new(),
                reviewed_plan: String::new(),
                choice: String::new(),
                success: false,
                summary: "Plan generation failed".to_string(),
                verification_passed: true,
                verification_errors: Vec::new(),
            });
        }
        self.emit_thought(0, "  ✅ Plan generated").await;

        // 2. 审查计划
        self.emit_thought(0, "[2/3] Reviewing plan with stronger model...").await;
        let reviewed_plan = self.review_plan(prompt, &original_plan, options).await?;
        let reviewed_plan = if reviewed_plan.is_empty() {
            self.emit_thought(0, "  ⚠️ Plan review failed, using original plan").await;
            original_plan.clone()
        } else {
            self.emit_thought(0, "  ✅ Plan reviewed").await;
            reviewed_plan
        };

        // 3. Gate: 用户选择计划
        self.emit_thought(0, "[3/3] Waiting for user to choose plan...").await;
        let choice = if let Some(ref cb) = self.gate_callback {
            cb(
                GatePhase::PlanCompare,
                GateInfo {
                    phase: GatePhase::PlanCompare,
                    changes: String::new(),
                    verified: true,
                    verify_errors: Vec::new(),
                    findings_count: 0,
                    critical_count: 0,
                    original_plan: original_plan.clone(),
                    reviewed_plan: reviewed_plan.clone(),
                },
            )
        } else {
            "reviewed".to_string()
        };

        if choice == "cancelled" {
            return Ok(PlanFirstResult {
                task: prompt.to_string(),
                original_plan,
                reviewed_plan,
                choice,
                success: false,
                summary: "Cancelled by user".to_string(),
                verification_passed: true,
                verification_errors: Vec::new(),
            });
        }

        let chosen_plan = if choice == "original" { &original_plan } else { &reviewed_plan };
        let label = if choice == "original" { "原始计划" } else { "审查后计划" };
        self.emit_thought(0, &format!("  ▶️ 执行: {}", label)).await;

        // 4. YOLO 执行
        self.emit_thought(0, "Executing plan in YOLO mode...").await;
        self.implement_agent.set_mode(crate::security::approval::AgentMode::Yolo);
        let exec_prompt = format!(
            "Execute the following plan step by step. Do NOT skip any step.\n\n{}",
            chosen_plan
        );
        let result = self
            .implement_agent
            .run(session_id, &exec_prompt, workspace)
            .await;
        // 恢复模式
        self.implement_agent.set_mode(crate::security::approval::AgentMode::Flow);

        match result {
            Ok(_) => {
                self.emit_thought(0, "  ✅ Plan execution complete").await;
                // 5. 验证
                let (verify_ok, verify_errors) = run_verification(workspace).await;
                if !verify_ok {
                    self.emit_thought(0, &format!("  ⚠️ Verification issues: {} error(s)", verify_errors.len())).await;
                }
                Ok(PlanFirstResult {
                    task: prompt.to_string(),
                    original_plan,
                    reviewed_plan,
                    choice,
                    success: true,
                    summary: format!("Plan executed successfully ({})", label),
                    verification_passed: verify_ok,
                    verification_errors: verify_errors,
                })
            }
            Err(e) => {
                Ok(PlanFirstResult {
                    task: prompt.to_string(),
                    original_plan,
                    reviewed_plan,
                    choice,
                    success: false,
                    summary: format!("Execution failed: {}", e),
                    verification_passed: true,
                    verification_errors: Vec::new(),
                })
            }
        }
    }

    // ── 内部方法 ──

    /// 让实现 Agent 修复审查发现的问题
    async fn run_fix(
        &self,
        session_id: &str,
        task: &str,
        findings: &[ReviewFinding],
        _iteration: usize,
        workspace: &str,
        options: &ChatOptions,
    ) -> Result<AgentRunResult, AgentError> {
        let findings_text = findings
            .iter()
            .map(|f| {
                format!(
                    "[{}] {}:{} - {}\n  suggestion: {}",
                    f.severity.to_uppercase(),
                    f.file_path,
                    f.line.map(|l| l.to_string()).unwrap_or_else(|| "?".to_string()),
                    f.description,
                    f.suggestion
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // D3-T01b: 复用 load_rules，避免与 implement_agent.run 内部 load_rules 重复加载
        let (rules, _) = ReActAgent::load_rules(workspace);
        let project_ctx = rules.join("\n\n");
        let fix_prompt = format!(
            "## Original Task\n{}\n\n## Review Findings to Fix\n{}\n\n{}Fix each issue above by editing the affected files.",
            task,
            findings_text,
            if project_ctx.is_empty() { String::new() } else { format!("## Project Context\n{}\n", project_ctx) }
        );

        self.implement_agent
            .run_with_options(session_id, &fix_prompt, workspace, options)
            .await
    }

    /// 构建审查 prompt
    fn build_review_prompt(
        &self,
        task: &str,
        diff: &str,
        acceptance_criteria: &[String],
        project_ctx: &str,
    ) -> String {
        let ac_block = if acceptance_criteria.is_empty() {
            String::new()
        } else {
            let ac_lines = acceptance_criteria
                .iter()
                .map(|ac| format!("- {}", ac))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "\n## Acceptance Criteria\nVerify if the implementation satisfies each criterion:\n{}\n\n\
                 For each criterion, state PASS or FAIL in your output using:\n\
                 <ac-result>\n1. PASS - explanation\n2. FAIL - explanation\n...\n</ac-result>",
                ac_lines
            )
        };

        let ctx_block = if project_ctx.is_empty() {
            String::new()
        } else {
            format!("\n{}", project_ctx)
        };

        format!(
            "## Original Task\n{}\n\n## Code Changes (git diff)\n{}\n\n{}{}\n\n\
             Review the diff above. Output your findings in <findings> tags or <findings>PASS</findings> if clean.",
            task,
            &diff[..diff.len().min(5000)],
            ac_block,
            ctx_block
        )
    }

    /// 生成验收标准
    async fn generate_acceptance_criteria(&self, task: &str, options: &ChatOptions) -> Vec<String> {
        let prompt = format!(
            "Decompose the following task into 3-6 measurable acceptance criteria.\n\
             Each criterion must be verifiable (can be checked by reading code or running tests).\n\
             Output each criterion on a separate line starting with '- '.\n\
             Do NOT output anything else.\n\n\
             Task: {}",
            &task[..task.len().min(800)]
        );

        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: MessageContent::Text(prompt),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];

        match self.implement_agent.provider.chat(&messages, &[], options).await {
            Ok(resp) => {
                if let Some(choice) = resp.choices.into_iter().next() {
                    let text = match choice.message.content {
                        MessageContent::Text(t) => t,
                        MessageContent::Parts(parts) => parts
                            .iter()
                            .map(|p| match p {
                                crate::provider::provider::ContentPart::Text { text } => text.clone(),
                                crate::provider::provider::ContentPart::ImageUrl { image_url } => image_url.url.clone(),
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    };
                    let mut criteria = Vec::new();
                    for line in text.lines() {
                        let line = line.trim();
                        if line.starts_with("- ") {
                            let c = line[2..].trim().to_string();
                            if c.len() > 5 {
                                criteria.push(c);
                            }
                        } else if line.starts_with("* ") {
                            let c = line[2..].trim().to_string();
                            if c.len() > 5 {
                                criteria.push(c);
                            }
                        }
                    }
                    criteria.into_iter().take(6).collect()
                } else {
                    Vec::new()
                }
            }
            Err(e) => {
                warn!("Failed to generate acceptance criteria: {}", e);
                Vec::new()
            }
        }
    }

    /// 生成实现计划
    async fn generate_plan(&self, task: &str, options: &ChatOptions) -> Result<String, AgentError> {
        let prompt = format!(
            "Create a detailed implementation plan for the following task.\n\nTask: {}",
            task
        );
        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: MessageContent::Text(PLAN_GENERATION_PROMPT.to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: MessageContent::Text(prompt),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        match self.implement_agent.provider.chat(&messages, &[], options).await {
            Ok(resp) => Ok(extract_plan(&resp)),
            Err(e) => {
                warn!("Plan generation failed: {}", e);
                Ok(String::new())
            }
        }
    }

    /// 审查计划
    async fn review_plan(&self, task: &str, original_plan: &str, options: &ChatOptions) -> Result<String, AgentError> {
        let prompt = format!(
            "## Original Task\n{}\n\n## Original Plan\n{}\n\nReview the plan above and output an improved version.",
            task, original_plan
        );
        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: MessageContent::Text(PLAN_REVIEW_PROMPT.to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: MessageContent::Text(prompt),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        match self.review_agent.provider.chat(&messages, &[], options).await {
            Ok(resp) => Ok(extract_plan(&resp)),
            Err(e) => {
                warn!("Plan review failed: {}", e);
                Ok(String::new())
            }
        }
    }

    /// 构建 Flow 报告摘要
    fn build_summary(
        &self,
        success: bool,
        rounds: usize,
        findings_history: &[Vec<ReviewFinding>],
        ac: &[String],
        ac_satisfied: &[bool],
    ) -> String {
        let status = if success { "PASS" } else { "PARTIAL (max iterations)" };
        let icon = if success { "✅" } else { "❌" };
        let mut lines = vec![
            format!("{} Flow: {}", icon, status),
            format!("  Rounds: {}/{}", rounds, self.max_rounds),
        ];
        for (i, findings) in findings_history.iter().enumerate() {
            let count = findings.len();
            let mark = if count == 0 { "✅" } else { "" };
            lines.push(format!("  Round {}: {} finding(s) {}", i + 1, count, mark));
        }
        if !ac.is_empty() {
            let ac_pass = ac_satisfied.iter().filter(|&&b| b).count();
            lines.push(format!("  Acceptance Criteria: {}/{} ✅", ac_pass, ac.len()));
        }
        lines.join("\n")
    }

    /// 发射 Thought 事件
    async fn emit_thought(&self, step: usize, content: &str) {
        self.implement_agent
            .emit(crate::agent::types::AgentEvent::Thought {
                step,
                content: content.to_string(),
            })
            .await;
    }
}

// ============================================================================
// 公共函数
// ============================================================================

/// 获取 git diff（公共函数，供 react.rs mid-flow 审查使用）
pub async fn get_git_diff(workspace: &str) -> String {
    let output = tokio::process::Command::new("git")
        .args(&["diff", "--no-color"])
        .current_dir(workspace)
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let diff = String::from_utf8_lossy(&o.stdout).to_string();
            if diff.len() > 10000 {
                diff[..10000].to_string()
            } else {
                diff
            }
        }
        _ => String::new(),
    }
}

/// 统计变更文件数
fn count_files_changed(workspace: &str) -> String {
    let output = std::process::Command::new("git")
        .args(&["diff", "--name-only"])
        .current_dir(workspace)
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let count = String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .count();
            format!("{} file(s) changed", count)
        }
        _ => "no files changed".to_string(),
    }
}

/// 运行项目验证（编译/类型检查）
pub async fn run_verification(workspace: &str) -> (bool, Vec<String>) {
    let ws = Path::new(workspace);
    let mut errors: Vec<String> = Vec::new();

    if ws.join("Cargo.toml").exists() {
        // Rust: cargo check
        if let Ok(output) = tokio::process::Command::new("cargo")
            .args(&["check"])
            .current_dir(workspace)
            .output()
            .await
        {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                for line in stderr.lines() {
                    let line = line.trim();
                    if !line.is_empty() && (line.contains("error") || line.contains("warning")) {
                        errors.push(line.chars().take(200).collect());
                    }
                }
            }
        }
    } else if ws.join("package.json").exists() {
        // Node: npx tsc --noEmit
        if let Ok(output) = tokio::process::Command::new("npx")
            .args(&["tsc", "--noEmit"])
            .current_dir(workspace)
            .output()
            .await
        {
            if !output.status.success() {
                let combined = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                for line in combined.lines() {
                    let line = line.trim();
                    if !line.is_empty() && line.to_lowercase().contains("error") {
                        errors.push(line.chars().take(200).collect());
                    }
                }
            }
        }
    } else if ws.join("pyproject.toml").exists() || ws.join("setup.py").exists() {
        // Python: compileall
        if let Ok(output) = tokio::process::Command::new("python3")
            .args(&["-m", "compileall", "-q", workspace])
            .current_dir(workspace)
            .output()
            .await
        {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                for line in stderr.lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        errors.push(line.chars().take(200).collect());
                    }
                }
            }
        }
    }

    (errors.is_empty(), errors.into_iter().take(50).collect())
}

/// 解析审查输出（移植自 Python parse_review_output）
pub fn parse_review_output(output: &str) -> Vec<ReviewFinding> {
    // 先尝试从 <findings> 标签中提取
    if let Some(tag_content) = extract_tag_content(output, "findings") {
        let content = tag_content.trim();
        if content.eq_ignore_ascii_case("PASS") {
            return Vec::new();
        }
        let findings = parse_findings_from_text(content);
        if !findings.is_empty() {
            return findings;
        }
    }
    // fallback: 直接从文本解析
    parse_findings_from_text(output)
}

/// 从文本中解析 findings
fn parse_findings_from_text(text: &str) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    // Pattern: [SEVERITY] path:line - description
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if let Some(finding) = parse_finding_line(line) {
            // 尝试从下一行提取 suggestion
            if i + 1 < lines.len() {
                let next = lines[i + 1].trim();
                if next.starts_with("suggestion:") || next.starts_with("建议:") {
                    let sugg = next.splitn(2, ':').nth(1).unwrap_or("").trim();
                    let mut f = finding;
                    f.suggestion = sugg.to_string();
                    findings.push(f);
                    continue;
                }
            }
            findings.push(finding);
        }
    }
    findings
}

/// 解析单行 finding
fn parse_finding_line(line: &str) -> Option<ReviewFinding> {
    // [CRITICAL] path/file.py:42 - description
    let line = line.trim();
    if !line.starts_with('[') {
        return None;
    }
    let close = line.find(']')?;
    let severity = line[1..close].trim().to_lowercase();
    if !matches!(severity.as_str(), "critical" | "high" | "medium" | "low") {
        return None;
    }
    let rest = line[close + 1..].trim();

    // 尝试分离 file_path:line 和 description
    let (path_line, description) = if let Some(dash_pos) = rest.find(" - ") {
        (rest[..dash_pos].trim().to_string(), rest[dash_pos + 3..].trim().to_string())
    } else if let Some(dash_pos) = rest.find(" — ") {
        (rest[..dash_pos].trim().to_string(), rest[dash_pos + 3..].trim().to_string())
    } else {
        (rest.to_string(), String::new())
    };

    // 分离 file_path 和 line number
    let (file_path, line_num) = if let Some(colon_pos) = path_line.rfind(':') {
        let before = &path_line[..colon_pos];
        let after = &path_line[colon_pos + 1..];
        if let Ok(n) = after.parse::<usize>() {
            (before.trim().to_string(), Some(n))
        } else {
            (path_line.clone(), None)
        }
    } else {
        (path_line.clone(), None)
    };

    if file_path.is_empty() && description.is_empty() {
        return None;
    }

    Some(ReviewFinding {
        severity,
        file_path,
        line: line_num,
        description,
        suggestion: String::new(),
    })
}

/// 从文本中提取 XML 标签内容
fn extract_tag_content(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = text.to_lowercase().find(&open.to_lowercase())?;
    let content_start = start + open.len();
    let end = text.to_lowercase()[content_start..].find(&close.to_lowercase())?;
    Some(text[content_start..content_start + end].to_string())
}

/// 解析 AC 结果（移植自 Python _parse_ac_results）
pub fn parse_ac_results(output: &str, ac_count: usize) -> Vec<bool> {
    let content = match extract_tag_content(output, "ac-result") {
        Some(c) => c,
        None => return vec![false; ac_count],
    };

    let mut results = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 格式: "1. PASS - explanation" 或 "1. FAIL - explanation"
        if let Some(rest) = line.split('.').nth(1) {
            let rest = rest.trim();
            let is_pass = rest.to_uppercase().starts_with("PASS");
            results.push(is_pass);
        }
    }

    if results.is_empty() {
        return vec![false; ac_count];
    }
    results.into_iter().take(ac_count).collect::<Vec<_>>()
}

/// 从 LLM 响应中提取计划
fn extract_plan(resp: &ChatResponse) -> String {
    if let Some(choice) = resp.choices.first() {
        let text = match &choice.message.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    crate::provider::provider::ContentPart::Text { text } => text.clone(),
                    crate::provider::provider::ContentPart::ImageUrl { image_url } => image_url.url.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        // 尝试从 <plan> 标签提取
        if let Some(plan) = extract_tag_content(&text, "plan") {
            return plan.trim().to_string();
        }
        return text.trim().to_string();
    }
    String::new()
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_review_output ──

    #[test]
    fn parse_review_pass() {
        let output = "<findings>PASS</findings>";
        assert!(parse_review_output(output).is_empty());
    }

    #[test]
    fn parse_review_findings() {
        let output = r#"<findings>
[CRITICAL] src/main.rs:42 - buffer overflow
  suggestion: use checked arithmetic
[HIGH] src/lib.rs:88 - missing error handling
  suggestion: add match arm
</findings>"#;
        let findings = parse_review_output(output);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].severity, "critical");
        assert_eq!(findings[0].file_path, "src/main.rs");
        assert_eq!(findings[0].line, Some(42));
        assert_eq!(findings[0].description, "buffer overflow");
        assert_eq!(findings[0].suggestion, "use checked arithmetic");
        assert_eq!(findings[1].severity, "high");
        assert_eq!(findings[1].line, Some(88));
    }

    #[test]
    fn parse_review_no_tags() {
        let output = "[MEDIUM] utils.py:10 - bad naming\n  suggestion: rename to something better";
        let findings = parse_review_output(output);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "medium");
        assert_eq!(findings[0].suggestion, "rename to something better");
    }

    #[test]
    fn parse_review_empty() {
        assert!(parse_review_output("").is_empty());
        assert!(parse_review_output("no findings here").is_empty());
    }

    // ── parse_ac_results ──

    #[test]
    fn parse_ac_all_pass() {
        let output = r#"<ac-result>
1. PASS - feature implemented
2. PASS - tests written
3. PASS - docs updated
</ac-result>"#;
        let results = parse_ac_results(output, 3);
        assert_eq!(results, vec![true, true, true]);
    }

    #[test]
    fn parse_ac_mixed() {
        let output = r#"<ac-result>
1. PASS - ok
2. FAIL - missing test
</ac-result>"#;
        let results = parse_ac_results(output, 2);
        assert_eq!(results, vec![true, false]);
    }

    #[test]
    fn parse_ac_no_tag() {
        let results = parse_ac_results("no ac-result tag", 3);
        assert_eq!(results, vec![false, false, false]);
    }

    // ── extract_tag_content ──

    #[test]
    fn extract_tag_basic() {
        let text = "before <findings>content</findings> after";
        assert_eq!(extract_tag_content(text, "findings"), Some("content".to_string()));
    }

    #[test]
    fn extract_tag_case_insensitive() {
        let text = "<FINDINGS>content</FINDINGS>";
        assert_eq!(extract_tag_content(text, "findings"), Some("content".to_string()));
    }

    #[test]
    fn extract_tag_missing() {
        assert_eq!(extract_tag_content("no tags here", "findings"), None);
    }

    // ── parse_finding_line ──

    #[test]
    fn parse_finding_with_line_number() {
        let f = parse_finding_line("[CRITICAL] src/main.rs:42 - overflow").unwrap();
        assert_eq!(f.severity, "critical");
        assert_eq!(f.file_path, "src/main.rs");
        assert_eq!(f.line, Some(42));
        assert_eq!(f.description, "overflow");
    }

    #[test]
    fn parse_finding_without_line_number() {
        let f = parse_finding_line("[LOW] README.md - typo").unwrap();
        assert_eq!(f.severity, "low");
        assert_eq!(f.file_path, "README.md");
        assert_eq!(f.line, None);
    }

    #[test]
    fn parse_finding_invalid_severity() {
        assert!(parse_finding_line("[BOGUS] file.rs - something").is_none());
    }
}
