//! 持久化后台任务
//!
//! 支持：
//! - 后台索引代码
//! - 后台总结对话
//! - 后台同步向量记忆
//! - 并发数限制

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Semaphore};

use crate::core::cancellation::CancellationToken;

/// 任务类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    CodeIndex,
    MemorySummarize,
    VectorSync,
    Custom,
}

/// 任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// 任务定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDef {
    pub id: String,
    pub kind: TaskKind,
    pub name: String,
    pub payload: serde_json::Value,
}

/// 任务结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub output: Option<String>,
    pub error: Option<String>,
}

/// 可执行任务 trait
#[async_trait]
pub trait TaskHandler: Send + Sync {
    fn kind(&self) -> TaskKind;
    async fn run(&self, task: &TaskDef, token: CancellationToken) -> Result<String, String>;
}

/// 共享的任务处理器
pub type SharedTaskHandler = Arc<dyn TaskHandler>;

/// 后台任务调度器
pub struct TaskScheduler {
    sender: mpsc::UnboundedSender<TaskDef>,
    results: Arc<Mutex<HashMap<String, TaskResult>>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl TaskScheduler {
    pub fn new(handlers: Vec<SharedTaskHandler>, max_concurrent: usize) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<TaskDef>();
        let results: Arc<Mutex<HashMap<String, TaskResult>>> = Arc::new(Mutex::new(HashMap::new()));
        let results_clone = results.clone();
        let semaphore = Arc::new(Semaphore::new(max_concurrent));

        let handle = tokio::spawn(async move {
            while let Some(task) = receiver.recv().await {
                let handler = handlers.iter()
                    .find(|h| h.kind() == task.kind)
                    .cloned();

                if handler.is_none() {
                    let mut map = results_clone.lock().await;
                    map.insert(task.id.clone(), TaskResult {
                        task_id: task.id,
                        status: TaskStatus::Failed,
                        output: None,
                        error: Some(format!("No handler for task kind {:?}", task.kind)),
                    });
                    continue;
                }

                let semaphore = semaphore.clone();
                let results_clone = results_clone.clone();
                let handler = handler.unwrap();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await;
                    {
                        let mut map = results_clone.lock().await;
                        map.insert(task.id.clone(), TaskResult {
                            task_id: task.id.clone(),
                            status: TaskStatus::Running,
                            output: None,
                            error: None,
                        });
                    }

                    let token = CancellationToken::new();
                    let result = match handler.run(&task, token).await {
                        Ok(output) => TaskResult {
                            task_id: task.id.clone(),
                            status: TaskStatus::Completed,
                            output: Some(output),
                            error: None,
                        },
                        Err(err) => TaskResult {
                            task_id: task.id.clone(),
                            status: TaskStatus::Failed,
                            output: None,
                            error: Some(err),
                        },
                    };

                    let mut map = results_clone.lock().await;
                    map.insert(task.id, result);
                });
            }
        });

        Self {
            sender,
            results,
            _handle: handle,
        }
    }

    pub fn submit(&self, task: TaskDef) -> Result<(), String> {
        self.sender.send(task).map_err(|e| e.to_string())
    }

    pub async fn get_result(&self, task_id: &str) -> Option<TaskResult> {
        self.results.lock().await.get(task_id).cloned()
    }

    pub async fn list_results(&self) -> Vec<TaskResult> {
        self.results.lock().await.values().cloned().collect()
    }
}
