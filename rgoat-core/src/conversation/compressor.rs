//! 上下文压缩器 — 四层渐进式压缩
//!
//! 压缩等级：
//! - Micro: 保留最近 5 轮，其余概略
//! - ContextCollapse: 折叠上下文，保留函数签名
//! - SessionMemory: 只保留记忆摘要
//! - Full: 全量（不压缩）

use serde::{Deserialize, Serialize};

/// 压缩等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionLevel {
    /// 微小压缩：保留最近 5 轮
    Micro,
    /// 上下文折叠：折叠早期消息
    ContextCollapse,
    /// 会话记忆：仅保留摘要
    SessionMemory,
    /// 全量：不做任何压缩
    Full,
}

/// 压缩配置
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// 最大 token 数（估算）
    pub max_tokens: usize,
    /// Micro 压缩保留的最近消息数
    pub micro_keep_recent: usize,
    /// ContextCollapse 保留的最近消息数
    pub collapse_keep_recent: usize,
    /// SessionMemory 保留的摘要最大长度
    pub memory_max_chars: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            max_tokens: 128_000,      // 128K context window
            micro_keep_recent: 10,     // Keep last 10 messages
            collapse_keep_recent: 20,  // Keep last 20 messages
            memory_max_chars: 2000,    // Max 2000 chars for summary
        }
    }
}

/// 压缩后的消息
#[derive(Debug, Clone)]
pub struct CompressedMessages {
    /// 是否还有更多被压缩的消息
    pub has_more: bool,
    /// 被压缩的消息数量
    pub compressed_count: usize,
    /// 压缩后的摘要/预览
    pub summary: Option<String>,
    /// 保留的最近消息（不压缩部分）
    pub recent_messages: Vec<CompressedMessage>,
    /// 总 token 估算
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct CompressedMessage {
    pub role: String,
    pub content: String,
    pub is_summary: bool,
}

/// 里程碑摘要 — 压缩时保留已完成步骤的关键信息
///
/// 当 StepsTracker 标记步骤完成时，将该步骤的描述写入此结构，
/// 压缩时随摘要一起保留，避免上下文压缩导致 agent "失忆"。
#[derive(Debug, Clone, Default)]
pub struct MilestoneSummary {
    /// 已完成的步骤描述列表
    pub completed_steps: Vec<String>,
    /// 当前进度描述
    pub current_status: String,
    /// 关键决策记录
    pub key_decisions: Vec<String>,
}

impl MilestoneSummary {
    /// 是否为空（无任何里程碑信息）
    pub fn is_empty(&self) -> bool {
        self.completed_steps.is_empty()
            && self.current_status.is_empty()
            && self.key_decisions.is_empty()
    }

    /// 渲染为可注入的 SystemMessage 文本
    pub fn render(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut out = String::from("[里程碑摘要]\n");
        if !self.completed_steps.is_empty() {
            out.push_str("已完成步骤:\n");
            for (i, step) in self.completed_steps.iter().enumerate() {
                out.push_str(&format!("  {}. {}\n", i + 1, step));
            }
        }
        if !self.current_status.is_empty() {
            out.push_str(&format!("当前状态: {}\n", self.current_status));
        }
        if !self.key_decisions.is_empty() {
            out.push_str("关键决策:\n");
            for d in &self.key_decisions {
                out.push_str(&format!("  - {}\n", d));
            }
        }
        out
    }
}

/// 上下文压缩器
pub struct ContextCompressor {
    config: CompressionConfig,
}

impl ContextCompressor {
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// 检查和选择压缩等级
    pub fn select_level(&self, _message_count: usize, estimated_tokens: usize) -> CompressionLevel {
        if estimated_tokens < self.config.max_tokens / 2 {
            return CompressionLevel::Full;
        }
        if estimated_tokens < self.config.max_tokens * 3 / 4 {
            return CompressionLevel::Micro;
        }
        if estimated_tokens < self.config.max_tokens {
            return CompressionLevel::ContextCollapse;
        }
        CompressionLevel::SessionMemory
    }

    /// 执行压缩
    ///
    /// 仅生成统计摘要（消息计数与主题）。里程碑信息不在此处注入，
    /// 而是由调用方通过独立 SystemMessage 注入（见 react.rs），
    /// 避免里程碑内容被双重注入。
    pub fn compress(
        &self,
        messages: &[CompressedMessage],
        level: CompressionLevel,
    ) -> CompressedMessages {
        let total = messages.len();
        let (keep_count, summary_needed) = match level {
            CompressionLevel::Full => (total, false),
            CompressionLevel::Micro => (self.config.micro_keep_recent, total > self.config.micro_keep_recent),
            CompressionLevel::ContextCollapse => {
                (self.config.collapse_keep_recent, total > self.config.collapse_keep_recent)
            }
            CompressionLevel::SessionMemory => (0, true),
        };

        let split_at = if total > keep_count {
            total - keep_count
        } else {
            0
        };

        let compressed_count = split_at;
        let recent = messages[split_at..].to_vec();

        let summary = if summary_needed && compressed_count > 0 {
            Some(self.generate_summary(&messages[..split_at]))
        } else {
            None
        };

        let estimated_tokens = self.estimate_tokens(&recent, summary.as_deref());

        CompressedMessages {
            has_more: compressed_count > 0,
            compressed_count,
            summary,
            recent_messages: recent,
            estimated_tokens,
        }
    }

    /// Generate summary of compressed messages
    ///
    /// 仅生成消息统计与主题摘要。里程碑信息由调用方通过独立 SystemMessage 注入，
    /// 不在此处内嵌，避免同一次压缩中里程碑内容出现两份。
    fn generate_summary(
        &self,
        messages: &[CompressedMessage],
    ) -> String {
        let mut summary = String::from("[Earlier conversation summary]\n");

        let mut user_count = 0;
        let mut assistant_count = 0;
        let mut tool_count = 0;

        for msg in messages {
            match msg.role.as_str() {
                "user" => user_count += 1,
                "assistant" => assistant_count += 1,
                "tool" => tool_count += 1,
                _ => {}
            }
        }

        summary.push_str(&format!(
            "{} earlier messages: {} user messages, {} assistant responses, {} tool calls.\n",
            messages.len(),
            user_count,
            assistant_count,
            tool_count
        ));

        // Extract key topics from first character of each user message
        if user_count > 0 {
            let topics: Vec<String> = messages
                .iter()
                .filter(|m| m.role == "user")
                .take(5)
                .map(|m| m.content.chars().take(100).collect())
                .collect();
            if !topics.is_empty() {
                summary.push_str("Topics discussed: ");
                summary.push_str(&topics.join(" | "));
            }
        }

        // Truncate to max chars, respecting UTF-8 character boundaries
        // Using chars() count avoids panicking on CJK/multi-byte text
        if summary.chars().count() > self.config.memory_max_chars {
            let truncated: String = summary
                .chars()
                .take(self.config.memory_max_chars.saturating_sub(3))
                .collect();
            summary = truncated;
            summary.push_str("...");
        }

        summary
    }

    /// Rough token estimation
    fn estimate_tokens(
        &self,
        messages: &[CompressedMessage],
        summary: Option<&str>,
    ) -> usize {
        let mut tokens: usize = 0;
        for msg in messages {
            // Rough: 1 token ≈ 4 chars for English, ≈ 2 chars for Chinese
            tokens += msg.content.chars().count() / 3;
        }
        if let Some(s) = summary {
            tokens += s.chars().count() / 3;
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MilestoneSummary: is_empty ──

    #[test]
    fn milestone_summary_default_is_empty() {
        let m = MilestoneSummary::default();
        assert!(m.is_empty());
    }

    #[test]
    fn milestone_summary_with_completed_steps_not_empty() {
        let m = MilestoneSummary {
            completed_steps: vec!["step1".to_string()],
            ..Default::default()
        };
        assert!(!m.is_empty());
    }

    #[test]
    fn milestone_summary_with_current_status_not_empty() {
        let m = MilestoneSummary {
            current_status: "正在测试".to_string(),
            ..Default::default()
        };
        assert!(!m.is_empty());
    }

    #[test]
    fn milestone_summary_with_key_decisions_not_empty() {
        let m = MilestoneSummary {
            key_decisions: vec!["使用 Rust".to_string()],
            ..Default::default()
        };
        assert!(!m.is_empty());
    }

    // ── MilestoneSummary: render ──

    #[test]
    fn milestone_summary_render_empty_is_empty_string() {
        let m = MilestoneSummary::default();
        assert_eq!(m.render(), "");
    }

    #[test]
    fn milestone_summary_render_includes_completed_steps() {
        let m = MilestoneSummary {
            completed_steps: vec!["需求分析".to_string(), "代码实现".to_string()],
            ..Default::default()
        };
        let rendered = m.render();
        assert!(rendered.contains("需求分析"));
        assert!(rendered.contains("代码实现"));
    }

    #[test]
    fn milestone_summary_render_includes_current_status() {
        let m = MilestoneSummary {
            current_status: "正在测试".to_string(),
            ..Default::default()
        };
        assert!(m.render().contains("正在测试"));
    }

    #[test]
    fn milestone_summary_render_includes_key_decisions() {
        let m = MilestoneSummary {
            key_decisions: vec!["使用 Rust".to_string()],
            ..Default::default()
        };
        assert!(m.render().contains("使用 Rust"));
    }

    // ── compress() ──

    fn make_messages(n: usize) -> Vec<CompressedMessage> {
        (0..n)
            .map(|i| CompressedMessage {
                role: "user".to_string(),
                content: format!("msg {}", i),
                is_summary: false,
            })
            .collect()
    }

    #[test]
    fn compress_micro_includes_summary() {
        let compressor = ContextCompressor::new(CompressionConfig::default());
        let messages = make_messages(15);
        let result = compressor.compress(&messages, CompressionLevel::Micro);
        assert!(result.has_more);
        let summary = result.summary.expect("should have summary");
        // 摘要包含统计信息（里程碑信息不再内嵌于摘要，由调用方独立注入）
        assert!(summary.contains("user messages"));
    }

    #[test]
    fn compress_full_level_never_compresses() {
        let compressor = ContextCompressor::new(CompressionConfig::default());
        let messages = make_messages(15);
        let result = compressor.compress(&messages, CompressionLevel::Full);
        assert!(!result.has_more);
        assert!(result.summary.is_none());
    }
}
