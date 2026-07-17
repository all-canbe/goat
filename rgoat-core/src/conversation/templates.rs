//! 提示词模板引擎
//!
//! 管理各种模式的 System Prompt，支持项目规则注入、Skill 指令注入

use crate::security::approval::AgentMode;

/// 默认项目规则：当磁盘上没有规则文件时注入 system prompt
pub const DEFAULT_PROJECT_RULES: &str = r#"## Task Checklist System
- For complex tasks, first output `## 计划` listing the steps (each line starting with `-`)
- After completing each step, output `## 进度: N` (N = step number, 1-based)
- Simple tasks (Q&A, single file reading) don't need a checklist — answer directly
- When all steps are complete, provide a summary

## Tool Usage
- Use tools to read files, search code, execute commands, and modify files.
- Before writing or executing, confirm with the user unless in YOLO mode.
- When editing files, always read the file first.
- Never skip hooks or bypass safety checks.

## Code Quality
- Follow existing code style and conventions.
- Write clean, well-documented code.
- Handle errors gracefully.
- Write tests when appropriate.
"#;

/// 各模式精简描述
pub const AGENT_MODE_DESCRIPTION: &str = "You are Goat, an AI coding assistant. Read/search/write files and run shell commands to fulfill the user's request. Use tools efficiently; confirm destructive operations unless YOLO mode is active.";

pub const PLAN_MODE_DESCRIPTION: &str = "You are Goat in planning mode. Only read and analyze; do not write files or run commands. Produce a structured plan and ask for approval before execution.";

pub const FLOW_MODE_DESCRIPTION: &str = "You are Goat in flow mode. Follow the configured pipeline steps and emit checkpoint events. Prefer deterministic, repeatable actions.";

pub const ACCEPT_EDITS_DESCRIPTION: &str = "You are Goat in edit-review mode. Present each proposed edit as a diff and wait for user confirmation.";

pub const YOLO_MODE_DESCRIPTION: &str = "You are Goat in YOLO mode. You may execute file edits and shell commands without per-action approval. Still avoid irreversible destruction.";

/// 生成 System Prompt
///
/// 5 段结构：Identity+Mode / Workspace / Tools / Project Context / Active Skills
///
/// Project Context 段行为：
/// - `rules_loaded == false`（磁盘无规则文件）→ 注入 `DEFAULT_PROJECT_RULES` 全文
/// - `rules_loaded == true`（磁盘有规则文件）→ 仅注入每个规则文件前 5 行摘要
pub fn build_system_prompt(
    mode: AgentMode,
    workspace: &str,
    rules: &[String],
    rules_loaded: bool,
    skills: &[String],
    tools_description: &str,
    tech_stack: Option<&str>,
) -> String {
    let mode_desc = match mode {
        AgentMode::Agent => AGENT_MODE_DESCRIPTION,
        AgentMode::Plan => PLAN_MODE_DESCRIPTION,
        AgentMode::Flow => FLOW_MODE_DESCRIPTION,
        AgentMode::AcceptEdits => ACCEPT_EDITS_DESCRIPTION,
        AgentMode::Yolo => YOLO_MODE_DESCRIPTION,
    };

    // ── 段 1：Identity + Mode ──
    let mut prompt = format!(
        "{}\n\
         \n\
         ## Workspace\n\
         Current workspace: {}\n",
        mode_desc, workspace,
    );

    // P0: 段 2（保留）：技术栈约束作为 CRITICAL 系统级规则注入
    if let Some(stack) = tech_stack {
        prompt.push_str(&format!(
            "\n\
             ## ⚠ CRITICAL — Technology Stack Constraint\n\
             {}\n\
             - You MUST use the exact tech stack specified above.\n\
             - If the output does not contain the required framework files \
               (e.g. package.json with the correct dependencies, framework config files), \
               you have FAILED and must rewrite.\n\
             - NEVER substitute with vanilla HTML/CSS/JS when a framework is required.\n\
             - Before claiming completion, verify that the required project skeleton exists.\n",
            stack,
        ));
    }

    // ── 段 3：Tools ──
    prompt.push_str(&format!(
        "\n\
         ## Tools\n\
         {}\n",
        tools_description,
    ));

    // ── 段 4：Project Context ──
    if !rules_loaded {
        // 磁盘无规则文件 → 注入默认规则全文
        prompt.push_str("\n## Project Context (Default Rules)\n");
        prompt.push_str(DEFAULT_PROJECT_RULES);
    } else if !rules.is_empty() {
        // 磁盘有规则文件 → 仅注入每个规则文件前 5 行摘要
        prompt.push_str("\n## Project Context (Rules Summary)\n");
        prompt.push_str(
            "The following rule files are loaded from disk. Full content is injected as a separate context message.\n",
        );
        for (idx, rule) in rules.iter().enumerate() {
            let first_5_lines: Vec<&str> = rule.lines().take(5).collect();
            prompt.push_str(&format!("\n### Rule File {} (summary, first 5 lines):\n", idx + 1));
            for line in &first_5_lines {
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.push_str("... (truncated, see full rules in context message)\n");
        }
    }

    // ── 段 5：Active Skills（M5+ 渐进加载：name + 1 行描述）──
    if !skills.is_empty() {
        prompt.push_str(&format!("\n## Available Skills ({} total)\n", skills.len()));
        prompt.push_str("(Use `skills read <name>` to load the full detailed instructions for any skill.)\n");
        for skill in skills {
            prompt.push_str(&format!("{}\n", skill));
        }
    }

    prompt
}

/// P0: 从用户 prompt 中提取技术栈约束关键词。
///
/// 返回一个格式化后的约束字符串，用于注入 system prompt。
/// 如果未检测到已知技术栈关键词，返回 `None`。
pub fn extract_tech_stack_constraints(user_prompt: &str) -> Option<String> {
    let lower = user_prompt.to_lowercase();

    // Known tech-stack keywords — each maps to a constraint description
    let mut found: Vec<&str> = Vec::new();

    // Build tools
    if lower.contains("vite") { found.push("Vite"); }
    if lower.contains("webpack") && !lower.contains("vite") { found.push("Webpack"); }
    if lower.contains("next.js") || lower.contains("nextjs") { found.push("Next.js"); }
    if lower.contains("nuxt") { found.push("Nuxt"); }

    // Frontend frameworks
    if lower.contains("react") { found.push("React"); }
    if lower.contains("vue") && !lower.contains("preview") { found.push("Vue"); }
    if lower.contains("angular") { found.push("Angular"); }
    if lower.contains("svelte") { found.push("Svelte"); }

    // UI libraries
    if lower.contains("mui") || lower.contains("material-ui") || lower.contains("material ui") {
        found.push("MUI (Material-UI)");
    }
    if lower.contains("ant-design") || lower.contains("antd") { found.push("Ant Design"); }
    if lower.contains("shadcn") { found.push("shadcn/ui"); }
    if lower.contains("chakra") { found.push("Chakra UI"); }
    if lower.contains("bootstrap") { found.push("Bootstrap"); }

    // CSS frameworks
    if lower.contains("tailwind") || lower.contains("tailwindcss") ||
       lower.contains("tailwind css") { found.push("Tailwind CSS"); }

    // Languages
    if lower.contains("typescript") || lower.contains(" ts " ) ||
       lower.ends_with(" ts") || lower.contains("tsx") { found.push("TypeScript"); }

    if found.is_empty() {
        return None;
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<&&str> = found.iter().filter(|s| seen.insert(**s)).collect();

    let constraint = format!(
        "The user has explicitly requested the following technology stack: {}.\n\
         Every file you create MUST use these technologies. \
         Do NOT fall back to plain HTML/CSS/JS unless explicitly allowed.",
        unique.iter().map(|s| **s).collect::<Vec<_>>().join(" + "),
    );

    Some(constraint)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 近似 token 数估算：用单词数（split_whitespace count）作为简单近似
    fn approx_token_count(text: &str) -> usize {
        text.split_whitespace().count()
    }

    /// 无磁盘规则时，Agent 模式 prompt 近似 token 数应 ≤ 800
    #[test]
    fn test_agent_mode_prompt_under_800_tokens_without_rules() {
        let prompt = build_system_prompt(
            AgentMode::Agent,
            "/tmp",
            &[],
            false,
            &[],
            "read_file: read a file\nwrite_file: write a file",
            None,
        );
        let tokens = approx_token_count(&prompt);
        assert!(
            tokens <= 800,
            "Agent mode prompt without rules should be <= 800 tokens (approx), got {}. Prompt:\n{}",
            tokens,
            prompt
        );
    }

    /// 有磁盘规则时，Agent 模式 prompt 近似 token 数应 ≤ 600
    #[test]
    fn test_agent_mode_prompt_under_600_tokens_with_rules() {
        let rules = vec!["# My Rules\nline2\nline3\nline4\nline5\nline6\nline7".to_string()];
        let prompt = build_system_prompt(
            AgentMode::Agent,
            "/tmp",
            &rules,
            true,
            &[],
            "read_file: read a file\nwrite_file: write a file",
            None,
        );
        let tokens = approx_token_count(&prompt);
        assert!(
            tokens <= 600,
            "Agent mode prompt with rules summary should be <= 600 tokens (approx), got {}. Prompt:\n{}",
            tokens,
            prompt
        );
    }

    /// rules_loaded=false 时，应注入 DEFAULT_PROJECT_RULES（含 "Task Checklist System"）
    #[test]
    fn test_default_rules_injected_when_no_disk_rules() {
        let prompt = build_system_prompt(
            AgentMode::Agent,
            "/tmp",
            &[],
            false,
            &[],
            "read_file: read a file",
            None,
        );
        assert!(
            prompt.contains("Task Checklist System"),
            "When rules_loaded=false, prompt should inject DEFAULT_PROJECT_RULES containing 'Task Checklist System'.\nGot:\n{}",
            prompt
        );
    }

    /// rules_loaded=true 时，应注入规则前 5 行摘要，但不应包含第 6 行及之后内容
    #[test]
    fn test_rules_summary_injected_when_disk_rules_loaded() {
        let rules = vec!["# My Rules\nline2\nline3\nline4\nline5\nline6\nline7".to_string()];
        let prompt = build_system_prompt(
            AgentMode::Agent,
            "/tmp",
            &rules,
            true,
            &[],
            "read_file: read a file",
            None,
        );
        // 前 5 行应出现在摘要中
        assert!(prompt.contains("# My Rules"), "摘要应包含第 1 行");
        assert!(prompt.contains("line2"), "摘要应包含第 2 行");
        assert!(prompt.contains("line5"), "摘要应包含第 5 行");
        // 第 6 行及之后不应出现（验证只注入摘要，不注入全文）
        assert!(
            !prompt.contains("line6"),
            "rules_loaded=true 时不应包含第 6 行（line6），但找到了。Prompt:\n{}",
            prompt
        );
        assert!(
            !prompt.contains("line7"),
            "rules_loaded=true 时不应包含第 7 行（line7），但找到了。Prompt:\n{}",
            prompt
        );
    }
}
