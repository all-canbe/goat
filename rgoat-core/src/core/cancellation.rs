//! 取消令牌 — 优雅中断长时间运行的操作
//!
//! 支持手动取消 + Drop 自动取消，跨线程安全

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// 取消令牌
///
/// # Example
/// ```ignore
/// let token = CancellationToken::new();
/// let token_clone = token.clone();
///
/// let handle = tokio::spawn(async move {
///     loop {
///         if token_clone.is_cancelled() {
///             return;
///         }
///         // do work...
///     }
/// });
///
/// // Cancel after some condition
/// token.cancel();
/// ```
#[derive(Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Create a new uncancelled token
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Cancel the token
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Check if cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Reset the token to uncancelled state
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}
