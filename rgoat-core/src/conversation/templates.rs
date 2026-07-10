//! 提示词模板引擎
//!
//! 管理各种模式的 System Prompt，支持项目规则注入、Skill 指令注入

use crate::security::approval::AgentMode;

/// 生成 System Prompt
pub fn build_system_prompt(
    mode: AgentMode,
    workspace: &str,
    rules: &[String],
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

    let mut prompt = format!(
        "{}\n\
         \n\
         ## Workspace\n\
         Current workspace: {}\n",
        mode_desc, workspace,
    );

    // P0: Inject tech-stack constraint as a CRITICAL system-level rule
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

    prompt.push_str(&format!(
        "\n\
         ## Tools\n\
         {}\n",
        tools_description,
    ));

    // Inject project rules
    if !rules.is_empty() {
        prompt.push_str("\n## Project Rules\n");
        for rule in rules {
            prompt.push_str(&format!("- {}\n", rule));
        }
    }

    // Inject skill instructions — M5+ 渐进加载: 首轮只注入 name + 1 行描述
    if !skills.is_empty() {
        prompt.push_str(&format!("\n## Available Skills ({} total)\n", skills.len()));
        prompt.push_str("(Use `skills read <name>` to load the full detailed instructions for any skill.)\n");
        for skill in skills {
            prompt.push_str(&format!("{}\n", skill));
        }
    }

    prompt
}

pub const AGENT_MODE_DESCRIPTION: &str = r#"
You are Goat, a powerful AI coding assistant.

## Core Principles
- Be genuinely helpful and direct. Skip filler words.
- Have opinions based on evidence and reasoning.
- Be resourceful: try to figure things out before asking.
- Earn trust through competence.

## Task Checklist System
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

pub const PLAN_MODE_DESCRIPTION: &str = r#"
You are Goat in PLAN mode.

## Plan Mode Rules
- Read and explore the codebase thoroughly.
- Do NOT modify any files.
- Do NOT execute any commands that modify state.
- Generate a detailed plan with steps, files to change, and expected outcomes.
- Present the plan for user approval before any execution.

## Output Format
1. **Analysis**: What you found
2. **Plan**: Step-by-step approach
3. **Files**: Specific files to change
4. **Risks**: Potential issues
"#;

pub const FLOW_MODE_DESCRIPTION: &str = r#"
You are Goat in FLOW mode.

## Flow Mode Rules
- Implement the requested changes directly. Tool calls are auto-approved (non-destructive).
- After implementation, the system will automatically:
  1. Run verification (compile/type-check)
  2. Generate acceptance criteria
  3. Compute git diff
  4. Run an adversarial code review
  5. Apply fixes if needed (up to 3 rounds)
- Make focused, clean changes.
- Each round of fixes should address review feedback with minimal changes.
- If the review passes, stop.
"#;

pub const ACCEPT_EDITS_DESCRIPTION: &str = r#"
You are Goat in ACCEPT_EDITS mode.

## Accept Edits Mode Rules
- File edits are automatically accepted.
- Shell commands and network operations still require confirmation.
- Be more aggressive with file modifications since they'll be auto-approved.
"#;

pub const YOLO_MODE_DESCRIPTION: &str = r#"
You are Goat in YOLO mode.

## YOLO Mode Rules
- All operations are automatically approved.
- EXCEPTION: Destructive operations (rm, format, shutdown) are still blocked.
- Be efficient and don't ask for confirmations.
- Still maintain code quality and safety.
"#;

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
