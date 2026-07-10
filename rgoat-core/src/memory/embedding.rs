//! 嵌入模型接口 — 文本向量化
//!
//! 提供嵌入模型抽象层：
//! - Candle 本地推理（纯 Rust）
//! - ONNX Runtime（本地）
//! - API 调用（OpenAI/Cohere 等）
//!
//! Note: When Candle crate is not available, uses a hash-based mock embedding
//! so the rest of the system can compile and test.

use async_trait::async_trait;

/// 嵌入结果
#[derive(Debug, Clone)]
pub struct Embedding {
    pub vector: Vec<f32>,
    pub dimension: usize,
    pub tokens_used: Option<usize>,
}

/// 嵌入模型 trait
#[async_trait]
pub trait Embedder: Send + Sync {
    /// 生成文本嵌入向量
    async fn embed(&self, text: &str) -> Result<Embedding, EmbedError>;

    /// 批量生成嵌入向量
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError>;

    /// 获取嵌入维度
    fn dimension(&self) -> usize;

    /// 获取模型名称
    fn model_name(&self) -> &str;
}

// ============================================================================
// Mock Embedder（开发用 fallback）
// ============================================================================

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Mock embedder using hash-based vectors
///
/// NOT for production use. Generates deterministic pseudo-vectors
/// for testing the system flow without a real embedding model.
pub struct MockEmbedder {
    dimension: usize,
}

impl MockEmbedder {
    pub fn new(dimension: usize) -> Self {
        Self { dimension }
    }

    fn text_to_vector(&self, text: &str) -> Vec<f32> {
        // Hash-based pseudo embedding: deterministic, same text → same vector
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let base_hash = hasher.finish();

        let mut vector = Vec::with_capacity(self.dimension);
        for i in 0..self.dimension {
            let mut h = DefaultHasher::new();
            base_hash.hash(&mut h);
            i.hash(&mut h);
            let val = (h.finish() as f32) / (u64::MAX as f32);
            // Normalize to [-1, 1]
            vector.push(val * 2.0 - 1.0);
        }
        vector
    }
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding, EmbedError> {
        let vector = self.text_to_vector(text);
        Ok(Embedding {
            vector,
            dimension: self.dimension,
            tokens_used: None,
        })
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            embeddings.push(self.embed(text).await?);
        }
        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        "mock-embedder"
    }
}

// ============================================================================
// API Embedder（OpenAI 兼容）
// ============================================================================

/// OpenAI-compatible embedding API embedder
pub struct ApiEmbedder {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl ApiEmbedder {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Embedder for ApiEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding, EmbedError> {
        let response = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "input": text,
            }))
            .send()
            .await
            .map_err(|e| EmbedError::Api(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(EmbedError::Api(format!(
                "API error: {} {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let body: serde_json::Value = response.json().await
            .map_err(|e| EmbedError::Api(format!("Parse error: {}", e)))?;

        let vector: Vec<f32> = body["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| EmbedError::Api("No embedding in response".to_string()))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(Embedding {
            dimension: vector.len(),
            vector,
            tokens_used: body["usage"]["total_tokens"].as_u64().map(|t| t as usize),
        })
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            embeddings.push(self.embed(text).await?);
        }
        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        // Common dimensions: text-embedding-3-small=1536, text-embedding-3-large=3072
        // We'll return 0 as unknown until first embed
        0
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

// ============================================================================
// 错误类型
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Model error: {0}")]
    Model(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
