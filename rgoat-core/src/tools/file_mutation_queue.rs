//! 文件变更队列 — 按 canonical path 分桶序列化文件写入
//!
//! 并发的 Write/Edit 同一文件会通过此队列串行化，避免竞态。
//! canonicalize 失败（文件不存在）时回退到 path 本身作为 key，
//! 与 pi `file-mutation-queue.ts` 行为一致。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 文件变更序列化队列
///
/// 内部用 `Arc<Mutex<HashMap<...>>>` 包装，自身可被 `Clone` 后注入多个工具，
/// 共享同一组分桶锁。
#[derive(Clone)]
pub struct FileMutationQueue {
    buckets: Arc<std::sync::Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>>,
}

impl FileMutationQueue {
    /// 创建空队列
    pub fn new() -> Self {
        Self {
            buckets: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// 获取或创建 path 对应的锁。
    /// canonicalize 失败（文件不存在）时回退用 path 本身作为 key。
    fn get_or_create_key(&self, path: &Path) -> Arc<tokio::sync::Mutex<()>> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut buckets = self
            .buckets
            .lock()
            .expect("FileMutationQueue mutex poisoned");
        buckets
            .entry(canonical)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// 异步获取该 path 的锁，持有锁执行 future，返回结果。
    ///
    /// 同一 canonical path 的并发调用会串行执行，避免写竞态。
    pub async fn run<F, T>(&self, path: &Path, future: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let lock = self.get_or_create_key(path);
        let _guard = lock.lock().await;
        future.await
    }
}

impl Default for FileMutationQueue {
    fn default() -> Self {
        Self::new()
    }
}
