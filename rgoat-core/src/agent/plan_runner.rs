//! Plan 模式专用执行器 — 独立于 Agent 循环的两个阶段：Explore → Plan。
//!
//! Phase 1 (Explore): 只读探索代码库，LLM 自然输出文本分析，完成后显式标记 "## 进入规划"
//! Phase 2 (Plan): 生成执行计划，完成后调用 save_plan_doc 或输出 "## 计划完成"
//! Phase 3 (Review): 由调用方（TUI）处理用户审批，不在本模块内完成
//!
//! 对齐 Python plan_runner.py

use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use crate::agent::react::ReActAgent;
use crate::agent::types::{AgentError, AgentEvent};
use crate::provider::provider::{ChatMessage, ChatOptions, MessageContent, Role, ToolDef, ToolCallDef};

// ============================================================================
// 常量：阶段系统提示词 + 完成标记（移植自 Python plan_runner.py）
// ============================================================================

/// 探索阶段系统提示词
const EXPLORE_SYSTEM_PROMPT: &str = r#"## Plan 模式 — 探索阶段

你正在以【只读规划模式】探索代码库。你的任务是深入理解用户需求并收集足够信息来制定计划。

行为规则：
- 每个回复中**至少调用一个工具**（read_file / search_code / list_files）
- 可以自由输出文本进行分析和总结 — 文本输出是正常的探索行为，不会导致退出
- 当你的分析产生阶段性结论时，可以用文字表达，但**继续调用工具深入探索**
- 收集到的信息足以支撑完整计划时，在回复末尾输出 **## 进入规划** 来切换阶段
- 你也可以在分析过程中调用 save_plan_doc 保存探索笔记到 .goat/doc/

不要急于进入规划阶段 — 确保理解了：
- 项目架构和模块关系
- 涉及的关键文件和数据流
- 用户需求的完整范围"#;

/// 规划阶段系统提示词
const PLAN_SYSTEM_PROMPT: &str = r#"## Plan 模式 — 规划阶段

你现在进入【规划阶段】。基于探索阶段的结果，请制定一份完整的执行计划。

计划文档结构：
## 需求理解
  - 用户目标是什么
  - 涉及的功能范围
## 涉及模块
  - 列出需要修改的文件及原因
## 执行步骤
  - 每一步：操作描述、涉及文件、预期结果
## 风险与注意事项
## 验证方案

行为规则：
- 可以自由输出文本草案和完善计划 — 文本输出不会导致退出
- 可以调用 read_file / search_code 补充确认细节
- 计划完善后，**调用 save_plan_doc 工具**保存到 .goat/doc/
- 或输出 **## 计划完成** 标记来结束规划"#;

/// 探索阶段完成标记
const EXPLORE_COMPLETE_MARKERS: &[&str] = &[
    "## 进入规划",
    "## 开始规划",
    "## 规划阶段",
    "## 开始制定计划",
    "准备进入规划阶段",
];

/// 规划阶段完成标记
const PLAN_COMPLETE_MARKERS: &[&str] = &["## 计划完成", "## 规划完成"];

/// 最大轮次
const MAX_EXPLORE_TURNS: usize = 20;
const MAX_PLAN_TURNS: usize = 10;
const MAX_CONTINUATION: usize = 6;

// ============================================================================
// 数据结构
// ============================================================================

/// Plan 模式执行结果
#[derive(Debug, Clone)]
pub struct PlanResult {
    pub plan_path: Option<PathBuf>,
    pub plan_content: String,
    pub phase: PlanPhase,
}

/// Plan 阶段
#[derive(Debug, Clone, PartialEq)]
pub enum PlanPhase {
    ExploreComplete,
    PlanComplete,
    Cancelled,
    Error,
}

// ============================================================================
// PlanRunner
// ============================================================================

/// Plan 模式独立循环执行器
pub struct PlanRunner {
    agent: Arc<ReActAgent>,
    workspace: String,
    session_id: String,
}

impl PlanRunner {
    pub fn new(agent: Arc<ReActAgent>, workspace: String, session_id: String) -> Self {
        Self {
            agent,
            workspace,
            session_id,
        }
    }

    /// 运行完整 Plan 生命周期 (Explore + Plan)
    pub async fn run(&self, task: &str) -> Result<PlanResult, AgentError> {
        self.run_with_options(task, &ChatOptions::default()).await
    }

    /// 运行 Plan，携带请求级选项
    pub async fn run_with_options(
        &self,
        task: &str,
        options: &ChatOptions,
    ) -> Result<PlanResult, AgentError> {
        info!("PlanRunner starting for task: {}", &task[..task.len().min(80)]);

        // 添加用户消息
        self.agent
            .conversation
            .add_message(&self.session_id, "user", task, None, None)
            .await
            .map_err(|e| AgentError::Tool(e.to_string()))?;

        // Phase 1: Explore
        let explore_content = match self.run_explore_phase(options).await {
            Ok(c) => c,
            Err(_e) => {
                return Ok(PlanResult {
                    plan_path: None,
                    plan_content: String::new(),
                    phase: PlanPhase::Error,
                });
            }
        };

        // 检查取消
        if self.agent.cancellation.is_cancelled() {
            return Ok(PlanResult {
                plan_path: None,
                plan_content: explore_content,
                phase: PlanPhase::Cancelled,
            });
        }

        // Phase 2: Plan
        let (plan_path, plan_content) = self.run_plan_phase(options).await?;

        if self.agent.cancellation.is_cancelled() {
            return Ok(PlanResult {
                plan_path,
                plan_content,
                phase: PlanPhase::Cancelled,
            });
        }

        Ok(PlanResult {
            plan_path,
            plan_content,
            phase: PlanPhase::PlanComplete,
        })
    }

    // ── Phase 1: 探索 ──

    async fn run_explore_phase(&self, options: &ChatOptions) -> Result<String, AgentError> {
        self.emit_system("🔍 Plan 模式 — 开始探索代码库").await;

        let tool_defs = self.get_tool_defs();
        let mut continuation_count = 0usize;

        for turn in 0..MAX_EXPLORE_TURNS {
            if self.agent.cancellation.is_cancelled() {
                return Ok(String::new());
            }

            // 构建消息
            let messages = self.build_messages(EXPLORE_SYSTEM_PROMPT).await?;

            // 调用 LLM
            let response = self
                .agent
                .provider
                .chat(&messages, &tool_defs, options)
                .await
                .map_err(|e| AgentError::Llm(e.to_string()))?;

            let choice = response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| AgentError::Llm("No response from LLM".to_string()))?;

            let content = match &choice.message.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Parts(parts) => parts
                    .iter()
                    .map(|p| match p {
                        crate::provider::provider::ContentPart::Text { text } => text.clone(),
                        crate::provider::provider::ContentPart::ImageUrl { image_url } => {
                            image_url.url.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };

            // 流式输出
            if !content.is_empty() {
                self.emit_thought(turn, &content).await;
            }

            // 检查阶段完成标记
            if has_marker(&content, EXPLORE_COMPLETE_MARKERS) {
                self.add_assistant_message(&content, &choice.message.tool_calls).await;
                self.emit_system("📋 探索完成，进入规划阶段").await;
                return Ok(content);
            }

            let tool_calls = choice.message.tool_calls.unwrap_or_default();

            if tool_calls.is_empty() {
                // 无工具调用 → 续推机制
                continuation_count += 1;

                if looks_like_exploring(&content) {
                    self.add_assistant_message(&content, &None).await;
                    let nudge = explore_nudge(continuation_count);
                    self.add_user_message(nudge).await;
                    self.emit_system(&format!(
                        "探索中… 自动继续（{}/{}）",
                        continuation_count, MAX_CONTINUATION
                    ))
                    .await;
                    continue;
                }

                // 长文本无完成标记 → 可能是分析输出，继续
                if content.trim().len() > 80 {
                    self.add_assistant_message(&content, &None).await;
                    self.add_user_message(
                        "请继续探索相关代码，或输出 ## 进入规划 切换到规划阶段。",
                    )
                    .await;
                    continue;
                }

                // 短文本无标记 → 催促
                if continuation_count <= MAX_CONTINUATION {
                    self.add_assistant_message(&content, &None).await;
                    let nudge = explore_nudge(continuation_count);
                    self.add_user_message(nudge).await;
                    continue;
                }
            }

            // 有工具调用 → 重置续推计数
            continuation_count = 0;
            self.add_assistant_message(&content, &Some(tool_calls.clone())).await;

            // 处理工具调用
            self.process_tool_calls(&tool_calls).await?;
        }

        // 超出最大轮次 → 强制进入规划
        self.add_user_message(
            "探索轮次已达上限。请基于已收集信息进入规划阶段，输出 ## 进入规划。",
        )
        .await;

        // 再给一轮机会
        let messages = self.build_messages(EXPLORE_SYSTEM_PROMPT).await?;
        let response = self
            .agent
            .provider
            .chat(&messages, &tool_defs, options)
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;

        let content = response
            .choices
            .into_iter()
            .next()
            .map(|c| match c.message.content {
                MessageContent::Text(t) => t,
                MessageContent::Parts(parts) => parts
                    .iter()
                    .map(|p| match p {
                        crate::provider::provider::ContentPart::Text { text } => text.clone(),
                        crate::provider::provider::ContentPart::ImageUrl { image_url } => {
                            image_url.url.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
            .unwrap_or_default();

        self.add_assistant_message(&content, &None).await;
        Ok(content)
    }

    // ── Phase 2: 规划 ──

    async fn run_plan_phase(&self, options: &ChatOptions) -> Result<(Option<PathBuf>, String), AgentError> {
        self.emit_system("📝 Plan 模式 — 开始制定计划").await;

        let tool_defs = self.get_tool_defs();

        // 注入过渡提示
        self.add_user_message("请基于上述探索结果，制定完整的执行计划。完成后调用 save_plan_doc 保存。")
            .await;

        let mut plan_content_parts: Vec<String> = Vec::new();

        for turn in 0..MAX_PLAN_TURNS {
            if self.agent.cancellation.is_cancelled() {
                return Ok((None, plan_content_parts.join("")));
            }

            let messages = self.build_messages(PLAN_SYSTEM_PROMPT).await?;
            let response = self
                .agent
                .provider
                .chat(&messages, &tool_defs, options)
                .await
                .map_err(|e| AgentError::Llm(e.to_string()))?;

            let choice = response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| AgentError::Llm("No response from LLM".to_string()))?;

            let content = match &choice.message.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Parts(parts) => parts
                    .iter()
                    .map(|p| match p {
                        crate::provider::provider::ContentPart::Text { text } => text.clone(),
                        crate::provider::provider::ContentPart::ImageUrl { image_url } => {
                            image_url.url.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            plan_content_parts.push(content.clone());

            if !content.is_empty() {
                self.emit_thought(turn, &content).await;
            }

            let tool_calls = choice.message.tool_calls.unwrap_or_default();

            // 检查完成标记
            if has_marker(&content, PLAN_COMPLETE_MARKERS) {
                self.add_assistant_message(&content, &None).await;
                let full = plan_content_parts.join("");
                let plan_path = self.save_plan_file(&full).await;
                if let Some(ref p) = plan_path {
                    self.emit_system(&format!("✅ 计划已保存: {}", p.display())).await;
                }
                return Ok((plan_path, full));
            }

            // 检查 save_plan_doc 工具调用
            for tc in &tool_calls {
                if tc.function.name == "save_plan_doc" {
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                    let filename = args
                        .get("filename")
                        .and_then(|v| v.as_str())
                        .unwrap_or("plan.md");
                    let full_plan = plan_content_parts.join("");
                    let file_content = args
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&full_plan);
                    let plan_path = self.save_plan_doc(filename, file_content).await;
                    self.add_assistant_message(&content, &Some(tool_calls.clone())).await;
                    // 添加工具结果消息
                    self.add_tool_message(
                        &tc.id,
                        &format!("计划已保存到: {}", plan_path.display()),
                    )
                    .await;
                    self.emit_system(&format!("✅ 计划已保存: {}", plan_path.display())).await;
                    return Ok((Some(plan_path), file_content.to_string()));
                }
            }

            if tool_calls.is_empty() {
                // 规划阶段文本输出是正常的
                self.add_assistant_message(&content, &None).await;
                self.add_user_message(
                    "请继续完善计划，完成后调用 save_plan_doc 保存或输出 ## 计划完成。",
                )
                .await;
                continue;
            }

            self.add_assistant_message(&content, &Some(tool_calls.clone())).await;
            self.process_tool_calls(&tool_calls).await?;
        }

        // 超出最大轮次 → 保存已有内容
        let full = plan_content_parts.join("");
        let plan_path = if !full.trim().is_empty() {
            let p = self.save_plan_file(&full).await;
            if let Some(ref p) = p {
                self.emit_system(&format!("⚠️ 规划轮次达上限，已自动保存: {}", p.display())).await;
            }
            p
        } else {
            None
        };
        Ok((plan_path, full))
    }

    // ── 工具调用处理 ──

    async fn process_tool_calls(
        &self,
        tool_calls: &[ToolCallDef],
    ) -> Result<(), AgentError> {
        for tc in tool_calls {
            if self.agent.cancellation.is_cancelled() {
                break;
            }

            let tool_name = &tc.function.name;
            self.emit_system(&format!("🔧 {}", tool_name)).await;

            // 执行工具
            let args: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::json!({}));

            let result = self.agent.tools.execute(tool_name, args.clone()).await;
            let result_str = if result.success {
                if result.output.len() > 5000 { &result.output[..5000] } else { &result.output }
            } else {
                &result.output
            };
            self.add_tool_message(&tc.id, result_str).await;
        }
        Ok(())
    }

    // ── 辅助方法 ──

    async fn build_messages(&self, system_prompt: &str) -> Result<Vec<ChatMessage>, AgentError> {
        let history = self
            .agent
            .conversation
            .get_messages(&self.session_id)
            .await
            .map_err(|e| AgentError::Tool(e.to_string()))?;

        let mut messages = vec![ChatMessage {
            role: Role::System,
            content: MessageContent::Text(system_prompt.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];

        for msg in history {
            let role = match msg.role.as_str() {
                "system" => continue, // 系统消息已单独添加
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::User,
            };
            // tool_calls 存储为 JSON 字符串，需要反序列化
            let tool_calls: Option<Vec<ToolCallDef>> = msg
                .tool_calls
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            messages.push(ChatMessage {
                role,
                content: MessageContent::Text(msg.content),
                name: None,
                tool_call_id: msg.tool_call_id,
                tool_calls,
            });
        }

        Ok(messages)
    }

    fn get_tool_defs(&self) -> Vec<ToolDef> {
        let mut defs = self.agent.tools.all_tool_defs();

        // 添加 save_plan_doc 工具定义（PlanRunner 内部处理，不在 ToolRegistry 中注册）
        defs.push(ToolDef {
            tool_type: "function".to_string(),
            function: crate::provider::provider::FunctionDef {
                name: "save_plan_doc".to_string(),
                description: "保存计划文档到 .goat/doc/ 目录。在规划阶段完成时调用此工具保存计划。".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "filename": {
                            "type": "string",
                            "description": "文件名（不含路径），会自动保存到 .goat/doc/ 目录"
                        },
                        "content": {
                            "type": "string",
                            "description": "计划文档内容（Markdown 格式）"
                        }
                    },
                    "required": ["content"]
                }),
            },
        });

        defs
    }

    async fn add_assistant_message(
        &self,
        content: &str,
        tool_calls: &Option<Vec<ToolCallDef>>,
    ) {
        // tool_calls 需要序列化为 JSON 字符串存储
        let tool_calls_str: Option<String> = tool_calls
            .as_ref()
            .map(|tc| serde_json::to_string(tc).unwrap_or_default());
        let tc_ref = tool_calls_str.as_deref();
        let _ = self
            .agent
            .conversation
            .add_message(&self.session_id, "assistant", content, None, tc_ref)
            .await;
    }

    async fn add_user_message(&self, content: &str) {
        let _ = self
            .agent
            .conversation
            .add_message(&self.session_id, "user", content, None, None)
            .await;
    }

    async fn add_tool_message(&self, tool_call_id: &str, content: &str) {
        let _ = self
            .agent
            .conversation
            .add_message(
                &self.session_id,
                "tool",
                content,
                Some(tool_call_id),
                None,
            )
            .await;
    }

    async fn save_plan_doc(&self, filename: &str, content: &str) -> PathBuf {
        let safe_name = std::path::Path::new(filename)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "plan.md".to_string());
        let safe_name = if safe_name.ends_with(".md") {
            safe_name
        } else {
            format!("{}.md", safe_name)
        };

        let doc_dir = std::path::Path::new(&self.workspace)
            .join(".goat")
            .join("doc");
        let _ = tokio::fs::create_dir_all(&doc_dir).await;

        let target = doc_dir.join(&safe_name);
        let _ = tokio::fs::write(&target, content).await;
        target
    }

    async fn save_plan_file(&self, content: &str) -> Option<PathBuf> {
        if content.trim().is_empty() {
            return None;
        }
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        Some(self.save_plan_doc(&format!("plan_{}.md", timestamp), content).await)
    }

    async fn emit_system(&self, message: &str) {
        self.agent
            .emit(AgentEvent::Thought {
                step: 0,
                content: message.to_string(),
            })
            .await;
    }

    async fn emit_thought(&self, step: usize, content: &str) {
        self.agent
            .emit(AgentEvent::Thought {
                step,
                content: content.to_string(),
            })
            .await;
    }
}

// ============================================================================
// 辅助函数（移植自 Python plan_runner.py）
// ============================================================================

/// 检查内容是否包含任意完成标记
fn has_marker(content: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| content.contains(m))
}

/// 检查文本是否像探索中的过渡输出（非最终答案）
fn looks_like_exploring(content: &str) -> bool {
    let stripped = content.trim();
    if stripped.is_empty() {
        return true;
    }
    if stripped.len() < 8 {
        return true;
    }
    let phrases = [
        "让我", "先看", "查看", "找到", "搜索", "定位", "需要", "了解", "深入", "分析",
        "确认", "检查", "探索", "还不", "还需要", "进一步",
    ];
    phrases.iter().any(|p| stripped.contains(p))
}

/// 探索阶段的渐进式催促
fn explore_nudge(count: usize) -> &'static str {
    if count <= 2 {
        "请继续探索代码库，调用 read_file 或 search_code 了解更多相关代码。"
    } else if count <= 4 {
        "请深入分析关键模块和依赖关系，调用工具继续探索。充分理解后再进入规划。"
    } else {
        "如果已收集足够信息，请输出 ## 进入规划 切换到规划阶段。"
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_marker() {
        assert!(has_marker("## 进入规划", EXPLORE_COMPLETE_MARKERS));
        assert!(has_marker("some text\n## 开始规划\n", EXPLORE_COMPLETE_MARKERS));
        assert!(!has_marker("no marker here", EXPLORE_COMPLETE_MARKERS));
    }

    #[test]
    fn test_looks_like_exploring() {
        assert!(looks_like_exploring("让我看看这个文件"));
        assert!(looks_like_exploring("需要了解更多信息"));
        assert!(looks_like_exploring("深入"));
        assert!(looks_like_exploring(""));
        assert!(!looks_like_exploring("## 进入规划"));
        assert!(!looks_like_exploring("Implementation complete"));
    }

    #[test]
    fn test_explore_nudge() {
        assert!(explore_nudge(1).contains("继续探索"));
        assert!(explore_nudge(3).contains("深入分析"));
        assert!(explore_nudge(5).contains("## 进入规划"));
        assert!(explore_nudge(10).contains("## 进入规划"));
    }
}
