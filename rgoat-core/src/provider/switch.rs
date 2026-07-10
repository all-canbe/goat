//! LLM Provider 运行时切换
//!
//! ProviderSwitch 包装多个 LlmProvider，支持运行时无感切换。
//! 对上层（Agent/TUI）透明——调用方无需感知切换。

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::provider::*;

/// 在多个 Provider 之间切换的包装器
///
/// 所有内部状态均使用异步锁保护，因此支持从多线程/异步上下文运行时注册。
///
/// # Example
/// ```ignore
/// let switch = ProviderSwitch::new();
/// switch.register(deepseek_provider).await;
/// switch.register(claude_provider).await;
/// switch.select("deepseek").await;
/// // All subsequent chat() calls go to deepseek
/// ```
pub struct ProviderSwitch {
    providers: RwLock<HashMap<String, Arc<dyn LlmProvider>>>,
    current: RwLock<String>,
    /// 所有可切换的 provider 名称（保持插入顺序）
    names: RwLock<Vec<String>>,
}

impl ProviderSwitch {
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            current: RwLock::new(String::new()),
            names: RwLock::new(Vec::new()),
        }
    }

    /// 注册一个 provider（异步版本，支持运行时调用）
    pub async fn register(&self, provider: Arc<dyn LlmProvider>) {
        let name = provider.name().to_string();
        {
            let mut names = self.names.write().await;
            if !names.contains(&name) {
                names.push(name.clone());
            }
        }
        {
            let mut providers = self.providers.write().await;
            providers.insert(name.clone(), provider);
        }
        tracing::info!("Provider registered: {}", name);
    }

    /// 注册一个 provider（同步版本，用于初始化阶段）
    pub fn register_blocking(&mut self, provider: Arc<dyn LlmProvider>) {
        let name = provider.name().to_string();
        self.names.get_mut().push(name.clone());
        self.providers.get_mut().insert(name, provider);
    }

    /// 切换到指定 provider（按名称）
    pub async fn select(&self, name: &str) -> Result<(), String> {
        let providers = self.providers.read().await;
        if providers.contains_key(name) {
            *self.current.write().await = name.to_string();
            tracing::info!("Provider switched to: {}", name);
            Ok(())
        } else {
            let available: Vec<_> = providers.keys().cloned().collect();
            Err(format!("Provider '{}' not found. Available: {:?}", name, available))
        }
    }

    /// 列出所有注册的 provider 名称
    pub async fn list_names(&self) -> Vec<String> {
        self.names.read().await.clone()
    }

    /// 列出所有注册的 provider 名称（同步版本，用于初始化阶段）
    pub fn list_names_blocking(&self) -> Vec<String> {
        self.names.try_read().map(|n| n.clone()).unwrap_or_default()
    }

    /// 列出所有 provider 详情
    pub async fn list_details(&self) -> Vec<ProviderDetail> {
        let names = self.names.read().await;
        let current = self.current.read().await;
        let providers = self.providers.read().await;
        names.iter().map(|name| {
            let p = providers.get(name).unwrap();
            ProviderDetail {
                name: p.name().to_string(),
                model: p.model().to_string(),
                provider_type: format!("{:?}", p.provider_type()),
                is_current: *name == *current,
                base_url: p.base_url().to_string(),
                api_key: p.api_key().to_string(),
            }
        }).collect()
    }

    /// 获取当前活跃的 provider 名称
    pub async fn current_name(&self) -> String {
        self.current.read().await.clone()
    }

    async fn get_current_async(&self) -> Option<Arc<dyn LlmProvider>> {
        let current = self.current.read().await;
        self.providers.read().await.get(current.as_str()).cloned()
    }
}

#[derive(Debug, Clone)]
pub struct ProviderDetail {
    pub name: String,
    pub model: String,
    pub provider_type: String,
    pub is_current: bool,
    pub base_url: String,
    pub api_key: String,
}

#[async_trait]
impl LlmProvider for ProviderSwitch {
    async fn chat(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> Result<ChatResponse, LlmError> {
        match self.get_current_async().await {
            Some(p) => p.chat(messages, tools).await,
            None => Err(LlmError::Config("No provider selected. Use /provider <name>".to_string())),
        }
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<LlmStream, LlmError> {
        match self.get_current_async().await {
            Some(p) => p.chat_stream(messages, tools).await,
            None => Err(LlmError::Config("No provider selected".to_string())),
        }
    }

    fn name(&self) -> &str {
        // Best-effort: if we can read current, delegate; else return placeholder
        "provider-switch"
    }

    fn model(&self) -> &str {
        "switch"
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAICompatible // default fallback
    }
}
