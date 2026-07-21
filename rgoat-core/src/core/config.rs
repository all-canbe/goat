//! 配置管理 — 读取/写入 `~/.goat/setting.json` 和环境变量
//!
//! 兼容两种配置格式：
//! 1. 新版 rgoat 多 provider 格式（`providers` 数组）
//! 2. 旧版原 Goat 单 provider 格式（`base_url` + `api_key` 直接写在顶层）
//!
//! 加载时自动检测并迁移旧格式到新版。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::workspace::get_data_dir;
use crate::provider::provider::ProviderType;

/// 全局配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    // ── Provider 选择 ──
    /// 默认 provider 名称
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Provider 列表（新版多 provider 格式）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderSettings>,

    /// 默认模型
    #[serde(default = "default_model")]
    pub model: String,

    // ── 旧版 Goat 兼容字段（单 provider 格式）──
    /// [旧版] base_url — 加载后自动迁移到 providers[0]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// [旧版] api_key — 加载后自动迁移到 providers[0]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    // ── SubAgent / Flow 模型 ──
    /// 子 Agent 使用的模型（复用主 provider 的 base_url/api_key）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_model: Option<String>,

    /// Flow 审查模型
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_model: Option<String>,

    /// Flow 审查 API key（若不提供则复用主 provider）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_api_key: Option<String>,

    /// Flow 审查 base_url
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_base_url: Option<String>,

    /// Flow 审查 provider 类型
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_provider: Option<String>,

    // ── 并发与深度 ──
    /// 最大并发数（对应旧版 max_concurrent）
    #[serde(default = "default_max_concurrency")] // 如果旧版文件也有这个字段，serde 自动映射
    pub max_concurrency: usize,

    /// 子 Agent 最大深度（对应旧版 max_depth）
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// 最大 Agent turns
    #[serde(default = "default_max_turns")]
    pub max_agent_turns: usize,

    // ── 扩展 ──
    /// MCP 服务器配置路径
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_config_path: Option<String>,

    /// 自定义 rules 目录
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules_dir: Option<String>,

    /// Hook 配置（原版 Goat 格式，当前为只读保存，功能待实现）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<serde_json::Value>,

    /// 默认工作空间路径
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    pub name: String,
    #[serde(default = "default_provider_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    /// Provider type hint (openai_compatible / anthropic). Auto-detected if not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
}

// ============================================================================
// 默认值
// ============================================================================

fn default_provider() -> String {
    "main".to_string()
}

fn default_provider_enabled() -> bool {
    true
}

fn default_model() -> String {
    "gpt-4o".to_string()
}

fn default_max_turns() -> usize {
    50
}

fn default_max_concurrency() -> usize {
    3
}

fn default_max_depth() -> usize {
    3
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            providers: Vec::new(),
            model: default_model(),
            base_url: None,
            api_key: None,
            sub_model: None,
            review_model: None,
            review_api_key: None,
            review_base_url: None,
            review_provider: None,
            max_concurrency: default_max_concurrency(),
            max_depth: default_max_depth(),
            max_agent_turns: default_max_turns(),
            mcp_config_path: None,
            rules_dir: None,
            hooks: None,
            workspace: None,
        }
    }
}

impl Settings {
    /// 从 `setting.json` 加载配置，自动迁移旧格式并保存
    pub fn load() -> Result<Self, ConfigError> {
        let path = settings_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let mut settings: Settings = serde_json::from_str(&content)?;
            let migrated = settings.migrate_if_needed();
            if migrated {
                // 立即持久化迁移结果
                let _ = settings.save();
            }
            Ok(settings)
        } else {
            let settings = Settings::default();
            settings.save()?;
            Ok(settings)
        }
    }

    /// 加载配置，如果文件不存在则返回空配置（不自动创建默认 provider）
    pub fn load_or_empty() -> Result<Self, ConfigError> {
        let path = settings_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let mut settings: Settings = serde_json::from_str(&content)?;
            settings.migrate_if_needed();
            Ok(settings)
        } else {
            Ok(Settings {
                provider: String::new(),
                providers: Vec::new(),
                model: String::new(),
                base_url: None,
                api_key: None,
                sub_model: None,
                review_model: None,
                review_api_key: None,
                review_base_url: None,
                review_provider: None,
                max_concurrency: default_max_concurrency(),
                max_depth: default_max_depth(),
                max_agent_turns: default_max_turns(),
                mcp_config_path: None,
                rules_dir: None,
                hooks: None,
                workspace: None,
            })
        }
    }

    /// 检测并迁移旧版 Goat 配置格式 → 新版多 provider 格式
    ///
    /// 旧版特征：顶层有 `base_url` + `api_key`，且 `providers` 列表为空
    fn migrate_if_needed(&mut self) -> bool {
        if !self.providers.is_empty() {
            return false; // 已是新版格式
        }

        let has_old_fields = self.base_url.is_some() && self.api_key.is_some();
        if !has_old_fields {
            return false;
        }

        let base_url = self.base_url.take().unwrap_or_default();
        let api_key = self.api_key.take().unwrap_or_default();
        let model = self.model.clone();

        // 确定 provider 名称和类型
        let provider_type = Self::detect_provider_type(&base_url);
        // 如果顶层 provider 字段是类型名（如 "openai_compatible"），换成有意义的名称
        let name = if self.provider.is_empty()
            || self.provider == "openai_compatible"
            || self.provider == "anthropic"
        {
            // 从 base_url 域名部分提取名称
            Self::extract_provider_name(&base_url)
        } else {
            self.provider.clone()
        };

        let ps = ProviderSettings {
            name: name.clone(),
            enabled: true,
            base_url: Some(base_url),
            api_key: Some(api_key),
            models: Some(vec![model]),
            provider_type: Some(provider_type.to_api_string()),
        };

        self.providers.push(ps);
        let migrated_name = name.clone();
        self.provider = name;

        tracing::info!("Migrated old-format settings to multi-provider format: provider='{}'", migrated_name);
        true
    }

    /// 从 base_url 提取有意义的 provider 名称
    fn extract_provider_name(base_url: &str) -> String {
        let lower = base_url.to_lowercase();
        if lower.contains("deepseek") {
            "deepseek".to_string()
        } else if lower.contains("openai") {
            "openai".to_string()
        } else if lower.contains("anthropic") {
            "anthropic".to_string()
        } else if lower.contains("dashscope") || lower.contains("qwen") {
            "qwen".to_string()
        } else if lower.contains("xiaomimimo") {
            "mimo".to_string()
        } else {
            // 从 URL 中提取 host
            base_url
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or("custom")
                .split('.')
                .next()
                .unwrap_or("custom")
                .to_string()
        }
    }

    /// 检查是否有已配置的 provider（有 api_key 或可从环境变量读取）
    pub fn has_configured_provider(&self) -> bool {
        self.providers.iter().any(|p| {
            p.api_key.as_ref().map_or(false, |k| !k.is_empty())
        }) || self.has_env_api_key()
    }

    /// 检查环境变量中是否有可用的 API key
    pub fn has_env_api_key(&self) -> bool {
        std::env::var("DEEPSEEK_API_KEY").is_ok()
            || std::env::var("OPENAI_API_KEY").is_ok()
            || std::env::var("ANTHROPIC_API_KEY").is_ok()
    }

    /// 添加或更新一个 provider
    pub fn add_or_update_provider(
        &mut self,
        name: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
    ) {
        let provider_type = Self::detect_provider_type(base_url);
        if let Some(existing) = self.providers.iter_mut().find(|p| p.name == name) {
            existing.base_url = Some(base_url.to_string());
            existing.api_key = if api_key.is_empty() { existing.api_key.clone() } else { Some(api_key.to_string()) };
            existing.models = Some(vec![model.to_string()]);
            existing.provider_type = Some(provider_type.to_api_string());
            existing.enabled = true;
        } else {
            self.providers.push(ProviderSettings {
                name: name.to_string(),
                enabled: true,
                base_url: Some(base_url.to_string()),
                api_key: if api_key.is_empty() { None } else { Some(api_key.to_string()) },
                models: Some(vec![model.to_string()]),
                provider_type: Some(provider_type.to_api_string()),
            });
        }
    }

    /// 从 URL 自动检测 Provider 类型
    pub fn detect_provider_type(base_url: &str) -> ProviderType {
        let lower = base_url.to_lowercase();
        if lower.contains("anthropic") {
            ProviderType::Anthropic
        } else {
            ProviderType::OpenAICompatible
        }
    }

    /// 保存配置到 `setting.json`
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        // Atomic write: write to temp file then rename
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// 获取指定 provider 的 API key（优先环境变量）
    pub fn get_api_key(&self, provider: &str) -> Option<String> {
        // Check environment variable first
        let env_key = match provider {
            "deepseek" => std::env::var("DEEPSEEK_API_KEY").ok(),
            "openai" => std::env::var("OPENAI_API_KEY").ok(),
            "anthropic" => std::env::var("ANTHROPIC_API_KEY").ok(),
            _ => None,
        };
        if env_key.is_some() {
            return env_key;
        }

        // Check settings
        self.providers
            .iter()
            .find(|p| p.name == provider)
            .and_then(|p| p.api_key.clone())
    }

    /// 获取指定 provider 的 base_url
    pub fn get_base_url(&self, provider: &str) -> Option<String> {
        self.providers
            .iter()
            .find(|p| p.name == provider)
            .and_then(|p| p.base_url.clone())
    }

    /// 获取第一个已配置 provider 的详情（name, base_url, api_key）
    pub fn primary_provider_detail(&self) -> Option<(&str, &str, &str)> {
        let ps = self.providers.iter().find(|p| p.api_key.is_some())?;
        let base_url = ps.base_url.as_deref().unwrap_or("");
        let api_key = ps.api_key.as_deref().unwrap_or("");
        let _model = ps.models.as_ref()
            .and_then(|m| m.first())
            .map(|s| s.as_str())
            .unwrap_or(&self.model);
        Some((ps.name.as_str(), base_url, api_key))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

fn settings_path() -> PathBuf {
    get_data_dir().join("setting.json")
}
