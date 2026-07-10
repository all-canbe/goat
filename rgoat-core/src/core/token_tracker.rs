//! Token 用量追踪与成本估算
//!
//! 追踪 LLM API 调用的 Token 用量和成本

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Provider-specific pricing per 1M tokens (USD)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPricing {
    /// Price per 1M input tokens
    pub input_per_m: f64,
    /// Price per 1M output tokens
    pub output_per_m: f64,
}

impl ProviderPricing {
    pub fn deepseek() -> Self {
        Self {
            input_per_m: 0.27,   // $0.27/1M input
            output_per_m: 1.10,  // $1.10/1M output
        }
    }

    pub fn openai_gpt4o() -> Self {
        Self {
            input_per_m: 2.50,
            output_per_m: 10.00,
        }
    }

    pub fn claude_sonnet() -> Self {
        Self {
            input_per_m: 3.00,
            output_per_m: 15.00,
        }
    }

    pub fn free() -> Self {
        Self {
            input_per_m: 0.0,
            output_per_m: 0.0,
        }
    }
}

/// 单次调用的用量记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub model: String,
    pub provider: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl TokenUsage {
    /// Calculate cost based on provider pricing
    pub fn cost(&self, pricing: &ProviderPricing) -> f64 {
        (self.input_tokens as f64 / 1_000_000.0) * pricing.input_per_m
            + (self.output_tokens as f64 / 1_000_000.0) * pricing.output_per_m
    }
}

/// 聚合统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenStats {
    pub total_input: u64,
    pub total_output: u64,
    pub total_calls: u64,
    pub total_cost_usd: f64,
}

/// Token 追踪器（线程安全）
pub struct TokenTracker {
    pricing: HashMap<String, ProviderPricing>,
    usages: Mutex<Vec<TokenUsage>>,
}

impl TokenTracker {
    /// Create with default pricing for common providers
    pub fn new() -> Self {
        let mut pricing = HashMap::new();
        pricing.insert("deepseek".to_string(), ProviderPricing::deepseek());
        pricing.insert("openai".to_string(), ProviderPricing::openai_gpt4o());
        pricing.insert("anthropic".to_string(), ProviderPricing::claude_sonnet());
        pricing.insert("local".to_string(), ProviderPricing::free());

        Self {
            pricing,
            usages: Mutex::new(Vec::new()),
        }
    }

    /// Register a custom provider pricing
    pub fn register_pricing(&mut self, provider: &str, pricing: ProviderPricing) {
        self.pricing.insert(provider.to_string(), pricing);
    }

    /// Record a token usage
    pub fn record(&self, input: u64, output: u64, model: &str, provider: &str) {
        let usage = TokenUsage {
            input_tokens: input,
            output_tokens: output,
            model: model.to_string(),
            provider: provider.to_string(),
            timestamp: chrono::Utc::now(),
        };
        self.usages.lock().unwrap().push(usage);
    }

    /// Get total stats
    pub fn stats(&self) -> TokenStats {
        let usages = self.usages.lock().unwrap();
        let mut stats = TokenStats::default();

        for usage in usages.iter() {
            stats.total_input += usage.input_tokens;
            stats.total_output += usage.output_tokens;
            stats.total_calls += 1;

            let pricing = self
                .pricing
                .get(&usage.provider)
                .cloned()
                .unwrap_or_else(ProviderPricing::free);
            stats.total_cost_usd += usage.cost(&pricing);
        }

        stats
    }

    /// Get recent usages
    pub fn recent_usages(&self, count: usize) -> Vec<TokenUsage> {
        let usages = self.usages.lock().unwrap();
        usages.iter().rev().take(count).cloned().collect()
    }

    /// Reset all tracking
    pub fn reset(&self) {
        self.usages.lock().unwrap().clear();
    }
}

impl Default for TokenTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// 格式化 Token 数量（2.5k, 1.2M）
pub fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// 格式化成本
pub fn format_cost(usd: f64) -> String {
    if usd >= 1.0 {
        format!("${:.2}", usd)
    } else if usd >= 0.01 {
        format!("${:.4}", usd)
    } else {
        format!("${:.6}", usd)
    }
}
