//! LLM Provider 抽象层
//!
//! 统一支持 OpenAI 兼容 API (DeepSeek/智谱等) + Anthropic Claude

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// Provider 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderType {
    /// OpenAI 兼容 API (DeepSeek, 智谱, OpenAI, etc.)
    OpenAICompatible,
    /// Anthropic Claude
    Anthropic,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderType::OpenAICompatible => write!(f, "OpenAI Compatible"),
            ProviderType::Anthropic => write!(f, "Anthropic"),
        }
    }
}

impl ProviderType {
    /// 转为配置文件中使用的字符串
    pub fn to_api_string(&self) -> String {
        match self {
            ProviderType::OpenAICompatible => "openai_compatible".to_string(),
            ProviderType::Anthropic => "anthropic".to_string(),
        }
    }

    /// 从配置字符串解析
    pub fn from_api_string(s: &str) -> Self {
        match s {
            "anthropic" => ProviderType::Anthropic,
            _ => ProviderType::OpenAICompatible,
        }
    }
}

/// Provider 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider_type: ProviderType,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    /// Extra headers for API calls
    pub extra_headers: Option<std::collections::HashMap<String, String>>,
}

impl ProviderConfig {
    pub fn new(
        provider_type: ProviderType,
        name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            provider_type,
            name: name.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: Some(4096),
            temperature: Some(0.3),
            extra_headers: None,
        }
    }

    pub fn deepseek(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::new(
            ProviderType::OpenAICompatible,
            "deepseek",
            "https://api.deepseek.com/v1",
            api_key,
            model,
        )
    }

    pub fn openai(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::new(
            ProviderType::OpenAICompatible,
            "openai",
            "https://api.openai.com/v1",
            api_key,
            model,
        )
    }

    pub fn zhipu(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::new(
            ProviderType::OpenAICompatible,
            "zhipu",
            "https://open.bigmodel.cn/api/paas/v4",
            api_key,
            model,
        )
    }

    pub fn anthropic(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::new(
            ProviderType::Anthropic,
            "anthropic",
            "https://api.anthropic.com/v1",
            api_key,
            model,
        )
    }
}

/// 消息角色
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 消息内容
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    pub detail: Option<String>,
}

/// 聊天消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDef>>,
}

impl Default for ChatMessage {
    fn default() -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(String::new()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }
}

/// 工具调用定义（模型输出的工具调用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDef {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON string
}

/// 工具定义（传给模型的工具 schema）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

/// 聊天完成请求
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// 流式块
#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunk {
    #[serde(default)]
    pub choices: Vec<StreamChoice>,
    /// Optional usage info emitted in the final chunk (Anthropic message_delta, OpenAI last chunk)
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    #[serde(default)]
    pub delta: StreamDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamDelta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamToolCallDelta {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamFunctionDelta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

/// 聊天完成响应
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub message: ChatMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// LLM 错误
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {status}, body: {body}")]
    Api { status: u16, body: String },
    #[error("Rate limited")]
    RateLimited,
    #[error("Config error: {0}")]
    Config(String),
    #[error("Stream error: {0}")]
    Stream(String),
}

/// LLM 输出流类型
pub type LlmStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>;

/// LLM Provider trait
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 发送聊天完成请求（非流式）
    async fn chat(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> Result<ChatResponse, LlmError>;

    /// 发送聊天完成请求（流式）
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<LlmStream, LlmError>;

    /// 获取 provider 名称
    fn name(&self) -> &str;

    /// 获取模型名称
    fn model(&self) -> &str;

    /// 获取 provider 类型
    fn provider_type(&self) -> ProviderType;

    /// 获取 API base URL（用于 /model 命令列出可用模型）
    fn base_url(&self) -> &str { "" }

    /// 获取 API key（用于 /model 命令认证）
    fn api_key(&self) -> &str { "" }
}
