//! 步骤追踪器 — 移植自 Python 版 `tools/steps_tracker.py`
//!
//! 跟踪 Agent 的多步骤清单计划（`## 计划`/`## 进度: N` 格式），
//! 在 System Prompt 中向模型提供进度可见性，并在无工具调用时
//! 作为比 `looks_like_final_answer` 更可靠的完成判定（主检查 > 启发式兜底）。
//!
//! ## 使用方式
//!
//! 1. Agent 每轮从 LLM 输出文本中调用 `try_parse(content)` 解析 `## 计划` / `## 进度: N`。
//! 2. 在"无工具调用"分支中，优先调用 `is_all_done()` 判定是否所有步骤已标记完成。
//! 3. 通过 `progress_injection()` 生成注入到后续消息中的进度状态行，
//!    供模型了解当前进度（追加为独立 SystemMessage 以保护 prefix cache）。

/// 单个计划步骤。
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanStep {
    index: usize,
    description: String,
}

/// 步骤追踪器 — 每轮 Agent `run()` 独立实例。
#[derive(Debug, Clone)]
pub struct StepsTracker {
    steps: Vec<PlanStep>,
    completed: Vec<bool>,
    has_plan: bool,
    current_step: Option<usize>,
}

impl StepsTracker {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            completed: Vec::new(),
            has_plan: false,
            current_step: None,
        }
    }

    /// 是否已制定计划（用于 System Prompt 注入判断）。
    pub fn has_plan(&self) -> bool {
        self.has_plan
    }

    /// C1: 返回当前计划的步骤数量（未制定计划时为 0）。
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// 从 LLM 文本输出中尝试解析 `## 计划` 和 `## 进度: N`。
    ///
    /// 匹配模式：
    /// - `## 计划\n- step1\n- step2\n...` → 更新步骤列表
    /// - `## 进度: N` → 标记步骤 N 为已完成
    ///
    /// 返回 `true` 表示本次解析到了新的计划或进度更新。
    pub fn try_parse(&mut self, content: &str) -> bool {
        let mut updated = false;

        // ── 解析 ## 计划 ──
        if let Some(plan_start) = content.find("## 计划") {
            // 从 "## 计划" 之后开始搜索，以 \n## 或 \n``` 或 EOF 为结束
            let after_plan = &content[plan_start + "## 计划".len()..];
            let plan_end = after_plan.find("\n##")
                .or_else(|| after_plan.find("\n```"))
                .unwrap_or(after_plan.len());
            let plan_body = &after_plan[..plan_end].trim();

            let new_steps: Vec<String> = plan_body
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim();
                    // 以 "- " 或 "* " 或 数字 "." 开头的行视为步骤
                    if let Some(rest) = trimmed.strip_prefix("- ")
                        .or_else(|| trimmed.strip_prefix("* "))
                        .or_else(|| {
                            let s = trimmed;
                            // 匹配 "1. xxx" 格式
                            let rest = s.splitn(2, ". ").nth(1)?;
                            if rest.is_empty() { None } else { Some(rest) }
                        })
                    {
                        if !rest.is_empty() {
                            return Some(rest.to_string());
                        }
                    }
                    None
                })
                .collect();

            if !new_steps.is_empty() {
                self.steps = new_steps
                    .into_iter()
                    .enumerate()
                    .map(|(i, desc)| PlanStep { index: i, description: desc })
                    .collect();
                self.completed = vec![false; self.steps.len()];
                self.has_plan = true;
                self.current_step = None;
                updated = true;
            }
        }

        // ── 解析 ## 进度: N ──
        // 同时支持 "## 进度: N" 和 "## 进度：N"（全角冒号）
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("## 进度")
                .or_else(|| trimmed.strip_prefix("## 进度"))
            {
                // rest 可能是 ": N" 或 "：N" 或 ": N" 等格式
                let rest = rest.trim_start_matches(|c: char| c == ':' || c == '：' || c.is_whitespace());
                if let Ok(n) = rest.parse::<usize>() {
                    let idx = n.saturating_sub(1); // 1-based → 0-based
                    if idx < self.completed.len() {
                        self.completed[idx] = true;
                        updated = true;
                    }
                }
            }
        }

        updated
    }

    /// 所有步骤是否已完成。
    ///
    /// **注意**：未制定计划时（`has_plan == false`）返回 `false`，
    /// 表示不走清单判定路径，应 fallback 到 `looks_like_final_answer`。
    pub fn is_all_done(&self) -> bool {
        if !self.has_plan || self.steps.is_empty() {
            return false;
        }
        self.completed.iter().all(|&done| done)
    }

    /// 生成注入到消息中的进度状态行（追加为独立 SystemMessage）。
    ///
    /// B7 增强：包含进度统计、当前执行步骤标记（🔄）、剩余步骤概要。
    /// 例如：
    /// ```text
    /// ## 当前计划进度
    /// **进度: 1/3**
    ///
    /// - ✅ 步骤 1: 需求分析
    /// - 🔄 步骤 2: 代码实现
    /// - ⬜ 步骤 3: 测试验证
    ///
    /// **剩余 2 步**: 代码实现 → 测试验证
    /// ```
    pub fn progress_injection(&self) -> Option<String> {
        if !self.has_plan || self.steps.is_empty() {
            return None;
        }
        let mut lines = vec!["## 当前计划进度".to_string()];

        // 统计
        let total = self.steps.len();
        let done_count = self.completed.iter().filter(|&&d| d).count();
        lines.push(format!("**进度: {}/{}**\n", done_count, total));

        for step in &self.steps {
            let status = if self.completed[step.index] { "✅" }
                         else if self.current_step == Some(step.index) { "🔄" }
                         else { "⬜" };
            lines.push(format!("- {} 步骤 {}: {}", status, step.index + 1, step.description));
        }

        // 剩余步骤概要
        let remaining: Vec<&String> = self.steps.iter()
            .filter(|s| !self.completed[s.index])
            .map(|s| &s.description)
            .collect();
        if !remaining.is_empty() {
            lines.push(format!("\n**剩余 {} 步**: {}", remaining.len(), remaining.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" → ")));
        }

        Some(lines.join("\n"))
    }

    /// B7: 设置当前正在执行的步骤索引。
    pub fn set_current_step(&mut self, index: usize) {
        if index < self.steps.len() {
            self.current_step = Some(index);
        }
    }

    /// B7: 获取当前正在执行的步骤索引。
    pub fn current_step_index(&self) -> Option<usize> {
        self.current_step
    }

    /// B7: 获取下一个未完成步骤的索引并设为当前步骤。
    pub fn advance_current_step(&mut self) {
        let next = (0..self.steps.len())
            .find(|&i| !self.completed[i]);
        self.current_step = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plan_and_progress() {
        let mut tracker = StepsTracker::new();
        assert!(!tracker.has_plan);

        let content = "## 计划\n- 分析需求\n- 实现代码\n- 写测试\n\n## 进度: 1\n一些其他文字";
        assert!(tracker.try_parse(content));
        assert!(tracker.has_plan);
        assert_eq!(tracker.steps.len(), 3);
        assert!(tracker.completed[0]);
        assert!(!tracker.completed[1]);
        assert!(!tracker.completed[2]);
        assert!(!tracker.is_all_done());

        // 继续标记
        let content2 = "## 进度: 2\n## 进度：3"; // 全角冒号
        assert!(tracker.try_parse(content2));
        assert!(tracker.completed[1]);
        assert!(tracker.completed[2]);
        assert!(tracker.is_all_done());
    }

    #[test]
    fn test_no_plan_is_all_done_false() {
        let tracker = StepsTracker::new();
        assert!(!tracker.is_all_done()); // 无计划 → false，走启发式
    }

    #[test]
    fn test_progress_injection_format() {
        let mut tracker = StepsTracker::new();
        tracker.try_parse("## 计划\n- 第一步\n- 第二步");
        tracker.try_parse("## 进度: 1");

        let injection = tracker.progress_injection().unwrap();
        // B7: 进度统计行
        assert!(injection.contains("进度: 1/2"));
        // 步骤状态
        assert!(injection.contains("✅ 步骤 1"));
        assert!(injection.contains("⬜ 步骤 2"));
        // B7: 剩余步骤概要
        assert!(injection.contains("剩余 1 步"));
        assert!(injection.contains("第二步"));
    }

    #[test]
    fn test_progress_injection_current_step_marker() {
        // B7: 当前步骤应标记为 🔄
        let mut tracker = StepsTracker::new();
        tracker.try_parse("## 计划\n- 第一步\n- 第二步\n- 第三步");
        tracker.try_parse("## 进度: 1");
        tracker.advance_current_step();

        let injection = tracker.progress_injection().unwrap();
        assert!(injection.contains("✅ 步骤 1"));
        assert!(injection.contains("🔄 步骤 2"), "当前步骤应标记为 🔄");
        assert!(injection.contains("⬜ 步骤 3"));
        assert!(injection.contains("剩余 2 步"));
    }

    #[test]
    fn test_advance_current_step() {
        // B7: advance_current_step 应将 current_step 设为第一个未完成步骤
        let mut tracker = StepsTracker::new();
        tracker.try_parse("## 计划\n- A\n- B\n- C");
        assert_eq!(tracker.current_step_index(), None);

        tracker.try_parse("## 进度: 1");
        tracker.advance_current_step();
        assert_eq!(tracker.current_step_index(), Some(1), "步骤1完成后，当前应为步骤2");

        tracker.try_parse("## 进度: 2");
        tracker.advance_current_step();
        assert_eq!(tracker.current_step_index(), Some(2), "步骤2完成后，当前应为步骤3");

        tracker.try_parse("## 进度: 3");
        tracker.advance_current_step();
        assert_eq!(tracker.current_step_index(), None, "全部完成后，current_step 应为 None");
    }

    #[test]
    fn test_parse_numbered_steps() {
        let mut tracker = StepsTracker::new();
        tracker.try_parse("## 计划\n1. 需求分析\n2. 代码实现");
        assert_eq!(tracker.steps.len(), 2);
        assert_eq!(tracker.steps[0].description, "需求分析");
        assert_eq!(tracker.steps[1].description, "代码实现");
    }

    #[test]
    fn test_replan_resets() {
        let mut tracker = StepsTracker::new();
        tracker.try_parse("## 计划\n- 第一步");
        tracker.try_parse("## 进度: 1");
        assert!(tracker.is_all_done());

        // 重新制定计划应重置
        tracker.try_parse("## 计划\n- 新的第一步\n- 新的第二步");
        assert!(!tracker.is_all_done());
        assert_eq!(tracker.steps.len(), 2);
    }
}
