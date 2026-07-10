//! 重试机制 — 指数退避重试

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
}

/// 执行异步操作并重试
pub async fn retry_async<F, T, E>(
    config: &RetryConfig,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send + 'static>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;
                if attempt >= config.max_retries {
                    return Err(e);
                }
                let delay = config.base_delay * 2u32.pow(attempt as u32 - 1);
                let delay = std::cmp::min(delay, config.max_delay);
                warn!(
                    "Retry attempt {}/{}, error: {}. Waiting {:?}",
                    attempt, config.max_retries, e, delay
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}
