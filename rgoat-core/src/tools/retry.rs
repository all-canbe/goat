//! 重试机制 — 指数退避 + LLM 错误可重试判定（对齐 OpenCode SessionRetry）

use std::time::Duration;
use tracing::warn;

/// 重试配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(8),
        }
    }
}

impl RetryConfig {
    /// 快速重试（网络请求）
    pub fn fast() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(4),
        }
    }

    /// LLM 调用重试（生产默认：最多 3 次 attempt，指数退避）
    pub fn llm() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(8),
        }
    }

    /// 第 `attempt` 次失败后的等待时间（attempt 从 1 起）
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let exp = attempt.saturating_sub(1).min(16) as u32;
        let delay = self.base_delay.saturating_mul(2u32.saturating_pow(exp));
        std::cmp::min(delay, self.max_delay)
    }
}

/// LLM 错误是否值得重试（OpenCode 风格：仅 transient）
///
/// 不重试：400 参数/鉴权/上下文溢出等永久错误  
/// 重试：5xx、429、超时、网络抖动、overloaded
pub fn is_llm_error_retryable(error: &str) -> bool {
    let lower = error.to_lowercase();

    // 永久错误：参数/鉴权/配额类 — fail-fast
    if lower.contains("invalid_parameter")
        || lower.contains("invalid_request")
        || lower.contains("function.arguments")
        || lower.contains("must be in json format")
        || lower.contains("context overflow")
        || lower.contains("context_length")
        || lower.contains("maximum context")
        || lower.contains("authentication")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("api key")
        || lower.contains("invalid_api_key")
    {
        return false;
    }

    // HTTP 状态：4xx（除 408/429）一般不重试
    if let Some(status) = extract_http_status(&lower) {
        if status == 408 || status == 429 || (500..600).contains(&status) {
            return true;
        }
        if (400..500).contains(&status) {
            return false;
        }
    }

    // 瞬时错误关键词
    lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("rate limit")
        || lower.contains("too many request")
        || lower.contains("overloaded")
        || lower.contains("temporarily unavailable")
        || lower.contains("connection")
        || lower.contains("econnreset")
        || lower.contains("network")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("500")
        || lower.contains("429")
}

fn extract_http_status(error_lower: &str) -> Option<u16> {
    // 匹配 "API error: 400" / "status: 429" / "error: 503"
    for marker in ["api error: ", "status: ", "status=", "http "] {
        if let Some(pos) = error_lower.find(marker) {
            let rest = &error_lower[pos + marker.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = digits.parse::<u16>() {
                if (100..600).contains(&code) {
                    return Some(code);
                }
            }
        }
    }
    None
}

/// 执行异步操作并重试（仅当 `retry_if` 为 true）
pub async fn retry_async<F, T, E>(
    config: &RetryConfig,
    operation: F,
) -> Result<T, E>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send + 'static>>,
    E: std::fmt::Display,
{
    retry_async_if(config, |_| true, operation).await
}

/// 带可重试判定的异步重试；不可重试错误立即返回
pub async fn retry_async_if<F, P, T, E>(
    config: &RetryConfig,
    mut retry_if: P,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send + 'static>>,
    P: FnMut(&E) -> bool,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;
                if attempt >= config.max_retries || !retry_if(&e) {
                    return Err(e);
                }
                let delay = config.delay_for_attempt(attempt);
                warn!(
                    "Retry attempt {}/{}, error: {}. Waiting {:?}",
                    attempt, config.max_retries, e, delay
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_400_not_retryable() {
        let err = r#"LLM error: API error: 400, body: {"error":{"code":"invalid_parameter_error","message":"The \"function.arguments\" parameter of the code model must be in JSON format."}}"#;
        assert!(!is_llm_error_retryable(err));
    }

    #[test]
    fn rate_limit_retryable() {
        assert!(is_llm_error_retryable("API error: 429, body: rate limit exceeded"));
        assert!(is_llm_error_retryable("Provider is overloaded"));
    }

    #[test]
    fn server_errors_retryable() {
        assert!(is_llm_error_retryable("API error: 503, body: unavailable"));
        assert!(is_llm_error_retryable("HTTP error: connection reset"));
        assert!(is_llm_error_retryable("timeout waiting for response"));
    }

    #[test]
    fn auth_not_retryable() {
        assert!(!is_llm_error_retryable("API error: 401, body: unauthorized"));
        assert!(!is_llm_error_retryable("API error: 403, body: forbidden"));
    }

    #[test]
    fn delay_grows_with_backoff() {
        let cfg = RetryConfig::llm();
        assert_eq!(cfg.delay_for_attempt(1), Duration::from_secs(1));
        assert_eq!(cfg.delay_for_attempt(2), Duration::from_secs(2));
        assert_eq!(cfg.delay_for_attempt(3), Duration::from_secs(4));
        assert_eq!(cfg.delay_for_attempt(10), Duration::from_secs(8)); // capped
    }
}
