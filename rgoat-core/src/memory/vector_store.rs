//! 向量记忆存储 — Zvec 封装层
//!
//! 基于 Zvec 嵌入式向量数据库，提供：
//! - 对话记忆的语义存储与检索
//! - 代码片段的语义索引
//! - 知识库的向量化存储
//!
//! Note: When Zvec Rust crate is not available (crates.io / git),
//! this module provides a trait-based abstraction + in-memory fallback
//! so the rest of the system can compile and run without it.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
// use tracing::warn;

use crate::core::workspace::get_data_dir;

// ============================================================================
// 数据模型
// ============================================================================

/// 向量条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    pub id: String,
    pub vector: Vec<f32>,
    pub metadata: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// 检索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub metadata: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// 集合（Collection）元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionInfo {
    pub name: String,
    pub dimension: usize,
    pub entry_count: usize,
}

// ============================================================================
// 向量库 trait
// ============================================================================

/// 向量存储抽象
///
/// When Zvec crate is available, implement this trait for Zvec Store.
/// When not available, use InMemoryVectorStore as fallback.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// 创建或打开集合
    async fn get_or_create_collection(&self, name: &str, dimension: usize) -> Result<(), VectorStoreError>;

    /// 插入向量
    async fn insert(
        &self,
        collection: &str,
        vector: Vec<f32>,
        metadata: serde_json::Value,
    ) -> Result<String, VectorStoreError>;

    /// 批量插入向量
    async fn insert_batch(
        &self,
        collection: &str,
        entries: Vec<(Vec<f32>, serde_json::Value)>,
    ) -> Result<Vec<String>, VectorStoreError>;

    /// 语义搜索
    async fn search(
        &self,
        collection: &str,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>, VectorStoreError>;

    /// 删除条目
    async fn delete(&self, collection: &str, id: &str) -> Result<(), VectorStoreError>;

    /// 获取集合信息
    async fn collection_info(&self, name: &str) -> Result<CollectionInfo, VectorStoreError>;

    /// 列出所有集合
    async fn list_collections(&self) -> Result<Vec<String>, VectorStoreError>;

    /// 删除集合
    async fn drop_collection(&self, name: &str) -> Result<(), VectorStoreError>;
}

// ============================================================================
// 内存向量库（开发用 fallback）
// ============================================================================

use std::sync::RwLock;
use uuid::Uuid;

/// In-memory vector store (fallback when Zvec is not available)
pub struct InMemoryVectorStore {
    collections: RwLock<HashMap<String, Collection>>,
}

struct Collection {
    dimension: usize,
    entries: Vec<VectorEntry>,
}

impl InMemoryVectorStore {
    pub fn new() -> Self {
        Self {
            collections: RwLock::new(HashMap::new()),
        }
    }

    /// Cosine similarity between two vectors
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        dot / (norm_a * norm_b)
    }
}

#[async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn get_or_create_collection(&self, name: &str, dimension: usize) -> Result<(), VectorStoreError> {
        let mut collections = self.collections.write().unwrap();
        collections.entry(name.to_string()).or_insert_with(|| Collection {
            dimension,
            entries: Vec::new(),
        });
        Ok(())
    }

    async fn insert(
        &self,
        collection: &str,
        vector: Vec<f32>,
        metadata: serde_json::Value,
    ) -> Result<String, VectorStoreError> {
        let mut collections = self.collections.write().unwrap();
        let col = collections.get_mut(collection)
            .ok_or_else(|| VectorStoreError::CollectionNotFound(collection.to_string()))?;

        let id = Uuid::new_v4().to_string();
        col.entries.push(VectorEntry {
            id: id.clone(),
            vector,
            metadata,
            timestamp: chrono::Utc::now(),
        });
        Ok(id)
    }

    async fn insert_batch(
        &self,
        collection: &str,
        entries: Vec<(Vec<f32>, serde_json::Value)>,
    ) -> Result<Vec<String>, VectorStoreError> {
        let mut ids = Vec::new();
        for (vector, metadata) in entries {
            let id = self.insert(collection, vector, metadata).await?;
            ids.push(id);
        }
        Ok(ids)
    }

    async fn search(
        &self,
        collection: &str,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        let collections = self.collections.read().unwrap();
        let col = collections.get(collection)
            .ok_or_else(|| VectorStoreError::CollectionNotFound(collection.to_string()))?;

        let mut scored: Vec<(f32, &VectorEntry)> = col
            .entries
            .iter()
            .map(|entry| {
                let score = Self::cosine_similarity(query_vector, &entry.vector);
                (score, entry)
            })
            .collect();

        // Sort by score descending
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let results: Vec<SearchResult> = scored
            .into_iter()
            .take(limit)
            .map(|(score, entry)| SearchResult {
                id: entry.id.clone(),
                score,
                metadata: entry.metadata.clone(),
                timestamp: entry.timestamp,
            })
            .collect();

        Ok(results)
    }

    async fn delete(&self, collection: &str, id: &str) -> Result<(), VectorStoreError> {
        let mut collections = self.collections.write().unwrap();
        let col = collections.get_mut(collection)
            .ok_or_else(|| VectorStoreError::CollectionNotFound(collection.to_string()))?;
        col.entries.retain(|e| e.id != id);
        Ok(())
    }

    async fn collection_info(&self, name: &str) -> Result<CollectionInfo, VectorStoreError> {
        let collections = self.collections.read().unwrap();
        let col = collections.get(name)
            .ok_or_else(|| VectorStoreError::CollectionNotFound(name.to_string()))?;
        Ok(CollectionInfo {
            name: name.to_string(),
            dimension: col.dimension,
            entry_count: col.entries.len(),
        })
    }

    async fn list_collections(&self) -> Result<Vec<String>, VectorStoreError> {
        let collections = self.collections.read().unwrap();
        Ok(collections.keys().cloned().collect())
    }

    async fn drop_collection(&self, name: &str) -> Result<(), VectorStoreError> {
        let mut collections = self.collections.write().unwrap();
        collections.remove(name);
        Ok(())
    }
}

// ============================================================================
// 向量记忆管理器
// ============================================================================

/// 高层向量记忆 API
///
/// 封装 VectorStore + Embedder，提供业务级 API：
/// - 记住对话摘要
/// - 语义检索历史
/// - 管理记忆生命周期
pub struct VectorMemory {
    store: Arc<dyn VectorStore>,
    #[allow(dead_code)]
    data_dir: PathBuf,
}

impl VectorMemory {
    /// 创建向量记忆管理器
    pub fn new(store: Arc<dyn VectorStore>) -> Self {
        let data_dir = get_data_dir().join("vectors");
        let _ = std::fs::create_dir_all(&data_dir);

        Self { store, data_dir }
    }

    /// 使用内存 fallback 创建（开发用）
    pub fn new_in_memory() -> Self {
        Self::new(Arc::new(InMemoryVectorStore::new()))
    }

    /// 初始化记忆集合
    pub async fn initialize(&self) -> Result<(), VectorStoreError> {
        let collections = vec![
            ("conversation_memory", 768),  // 对话记忆
            ("code_index", 768),            // 代码索引
            ("knowledge", 768),             // 知识库
        ];

        for (name, dim) in collections {
            self.store.get_or_create_collection(name, dim).await?;
        }
        Ok(())
    }

    /// 记住一条对话摘要
    pub async fn remember_conversation(
        &self,
        embedding: Vec<f32>,
        session_id: &str,
        summary: &str,
    ) -> Result<String, VectorStoreError> {
        self.store
            .insert(
                "conversation_memory",
                embedding,
                serde_json::json!({
                    "type": "conversation",
                    "session_id": session_id,
                    "summary": summary,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                }),
            )
            .await
    }

    /// 语义检索历史记忆
    pub async fn recall_memories(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        self.store
            .search("conversation_memory", query_embedding, limit)
            .await
    }

    /// 索引代码片段
    pub async fn index_code(
        &self,
        embedding: Vec<f32>,
        file: &str,
        name: &str,
        kind: &str,
        content_preview: &str,
    ) -> Result<String, VectorStoreError> {
        self.store
            .insert(
                "code_index",
                embedding,
                serde_json::json!({
                    "type": "code",
                    "file": file,
                    "name": name,
                    "kind": kind,
                    "content_preview": content_preview,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                }),
            )
            .await
    }

    /// 语义搜索代码
    pub async fn search_code(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        self.store.search("code_index", query_embedding, limit).await
    }

    /// 存入知识
    pub async fn add_knowledge(
        &self,
        embedding: Vec<f32>,
        title: &str,
        content: &str,
        source: &str,
    ) -> Result<String, VectorStoreError> {
        self.store
            .insert(
                "knowledge",
                embedding,
                serde_json::json!({
                    "type": "knowledge",
                    "title": title,
                    "content": content,
                    "source": source,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                }),
            )
            .await
    }

    /// 搜索知识库
    pub async fn search_knowledge(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        self.store.search("knowledge", query_embedding, limit).await
    }

    /// 获取统计信息
    pub async fn stats(&self) -> Result<MemoryStats, VectorStoreError> {
        let conv = self.store.collection_info("conversation_memory").await?;
        let code = self.store.collection_info("code_index").await?;
        let knowledge = self.store.collection_info("knowledge").await?;

        Ok(MemoryStats {
            conversation_count: conv.entry_count,
            code_count: code.entry_count,
            knowledge_count: knowledge.entry_count,
        })
    }
}

/// 记忆统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub conversation_count: usize,
    pub code_count: usize,
    pub knowledge_count: usize,
}

// ============================================================================
// 错误类型
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum VectorStoreError {
    #[error("Collection not found: {0}")]
    CollectionNotFound(String),
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Storage error: {0}")]
    Storage(String),
}
