//! 配置文件集成测试 — 验证原版 Goat 配置迁移
//!
//! 运行：cargo test -p rgoat-core --test config_migration -- --nocapture

use rgoat_core::core::config::Settings;
use serde_json::json;

/// 测试原版 Goat setting.json 能被正确加载和迁移
#[test]
fn test_load_original_goat_config() {
    let settings = Settings::load().expect("Failed to load settings.json");

    // ── 迁移后 providers 不应为空 ──
    assert!(
        !settings.providers.is_empty(),
        "After migration, providers should have at least 1 entry"
    );

    let p = &settings.providers[0];
    println!("Provider name: {}", p.name);
    println!("Provider model: {:?}", p.models);
    println!("Provider type: {:?}", p.provider_type);

    // ── provider name 应从 URL 提取（xiaomimimo → mimo）──
    assert_eq!(p.name, "mimo", "Provider name should be extracted from URL");

    // ── base_url 应迁移到 providers[0] ──
    assert!(p.base_url.is_some(), "Base URL should be in providers[0]");
    assert!(
        p.base_url.as_deref().unwrap().contains("xiaomimimo"),
        "Base URL should match original"
    );

    // ── api_key 应迁移到 providers[0] ──
    assert!(p.api_key.is_some(), "API key should be in providers[0]");
    assert!(
        p.api_key.as_deref().unwrap().starts_with("tp-"),
        "API key should be valid"
    );

    // ── model 应迁移到 providers[0].models[0] ──
    assert!(p.models.is_some(), "Models should be in providers[0]");
    assert_eq!(
        p.models.as_ref().unwrap().first().unwrap(),
        "mimo-v2.5-pro",
        "Model should match original"
    );

    // ── 旧格式顶层字段应被清除 ──
    assert!(settings.base_url.is_none(), "Old base_url should be cleared");
    assert!(settings.api_key.is_none(), "Old api_key should be cleared");

    // ── provider 字段应更新为提取的名称 ──
    assert_eq!(settings.provider, "mimo", "Top-level provider should be 'mimo'");
}

/// 测试附加字段被保留
#[test]
fn test_extra_fields_preserved() {
    let settings = Settings::load().expect("Failed to load settings");

    assert!(settings.sub_model.is_some(), "sub_model should be preserved");
    assert!(settings.review_model.is_some(), "review_model should be preserved");
    assert!(settings.review_api_key.is_some(), "review_api_key should be preserved");
    assert!(settings.review_base_url.is_some(), "review_base_url should be preserved");
    assert_eq!(settings.max_concurrency, 3);
    assert_eq!(settings.max_depth, 3);
    assert!(settings.hooks.is_some(), "hooks should be preserved");
    assert!(settings.workspace.is_some(), "workspace should be preserved");

    println!("sub_model: {}", settings.sub_model.unwrap());
    println!("review_model: {}", settings.review_model.unwrap());
    println!("workspace: {}", settings.workspace.unwrap());
}

/// 旧版 JSON 未包含 enabled 字段时，Provider 默认启用。
#[test]
fn test_provider_enabled_defaults_to_true_for_old_json() {
    let settings: Settings = serde_json::from_value(json!({
        "providers": [{
            "name": "legacy",
            "base_url": "https://example.com/v1",
            "api_key": "key"
        }]
    }))
    .expect("old provider JSON should deserialize");

    assert!(settings.providers[0].enabled);
}
