//! 规划器 — C1/C2 强制规划阶段 + 动态重规划
//!
//! 在 Agent 进入 ReAct 主循环前，先调用 LLM 生成结构化计划，
//! 后续每轮将计划作为上下文注入。当连续工具失败 ≥ 2 次时触发重规划。

use serde::{Deserialize, Serialize};

/// 计划步骤的动作类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Read,
    Write,
    Search,
    Verify,
    Delegate,
}

/// 任务复杂度评估
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
    Low,
    Medium,
    High,
}

/// 单个计划步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub description: String,
    pub action_type: ActionType,
    pub depends_on: Vec<String>,
    pub acceptance_criteria: String,
}

/// 执行计划
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
    pub estimated_complexity: Complexity,
}

/// 规划器
pub struct Planner;

impl Planner {
    /// 生成规划提示 — 引导 LLM 用 ## 计划 格式输出结构化计划
    pub fn planning_prompt(user_prompt: &str, workspace: &str) -> String {
        format!(
            "你正在工作区 `{}` 中工作。\n\
             用户请求：{}\n\n\
             在开始执行前，请先制定一个结构化计划。使用以下格式：\n\n\
             ## 计划\n\
             - 步骤1描述\n\
             - 步骤2描述\n\
             - ...\n\n\
             要求：\n\
             1. 每个步骤应该是原子操作（读文件、写文件、搜索、验证等）\n\
             2. 步骤顺序应该合理，有依赖关系的步骤按正确顺序排列\n\
             3. 最后一步应该是验证/测试步骤\n\
             4. 不要超过 10 个步骤\n\n\
             制定计划后，立即开始执行第一步。",
            workspace, user_prompt
        )
    }

    /// 从 LLM 输出中解析计划（复用 StepsTracker 的 ## 计划 格式）
    pub fn parse_plan(content: &str) -> Option<ExecutionPlan> {
        if !content.contains("## 计划") {
            return None;
        }

        let plan_start = content.find("## 计划")?;
        let after_plan = &content[plan_start + "## 计划".len()..];
        let plan_end = after_plan.find("\n##")
            .or_else(|| after_plan.find("\n```"))
            .unwrap_or(after_plan.len());
        let plan_body = after_plan[..plan_end].trim();

        let steps: Vec<PlanStep> = plan_body
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                let desc = trimmed.strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("* "))?;
                if desc.is_empty() {
                    return None;
                }
                let uuid_str = uuid::Uuid::new_v4().to_string();
                Some(PlanStep {
                    id: format!("step-{}", &uuid_str[..8]),
                    description: desc.to_string(),
                    action_type: ActionType::Read, // 默认，LLM 不指定时用 Read
                    depends_on: Vec::new(),
                    acceptance_criteria: String::new(),
                })
            })
            .collect();

        if steps.is_empty() {
            return None;
        }

        let complexity = if steps.len() <= 3 { Complexity::Low }
                        else if steps.len() <= 6 { Complexity::Medium }
                        else { Complexity::High };

        Some(ExecutionPlan {
            goal: user_prompt_extract(content),
            steps,
            estimated_complexity: complexity,
        })
    }
}

/// 尝试从内容中提取用户目标（简化版）
fn user_prompt_extract(content: &str) -> String {
    // 取 ## 计划 前的文本作为目标（如果有）
    if let Some(plan_start) = content.find("## 计划") {
        let before = content[..plan_start].trim();
        if !before.is_empty() {
            return before.chars().take(200).collect();
        }
    }
    "未指定目标".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_planning_prompt_format() {
        let prompt = Planner::planning_prompt("实现一个 todo app", "/workspace");
        assert!(prompt.contains("## 计划"));
        assert!(prompt.contains("实现一个 todo app"));
        assert!(prompt.contains("/workspace"));
    }

    #[test]
    fn test_parse_plan_success() {
        let content = "我来分析需求并制定计划。\n\n## 计划\n- 读取现有代码\n- 修改配置文件\n- 运行测试验证\n\n现在开始执行。";
        let plan = Planner::parse_plan(content).unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.estimated_complexity, Complexity::Low);
    }

    #[test]
    fn test_parse_plan_no_plan() {
        let content = "这是普通回复，没有计划。";
        assert!(Planner::parse_plan(content).is_none());
    }

    #[test]
    fn test_parse_plan_complexity() {
        let content = "## 计划\n- s1\n- s2\n- s3\n- s4\n- s5\n- s6\n- s7";
        let plan = Planner::parse_plan(content).unwrap();
        assert_eq!(plan.steps.len(), 7);
        assert_eq!(plan.estimated_complexity, Complexity::High);
    }
}
