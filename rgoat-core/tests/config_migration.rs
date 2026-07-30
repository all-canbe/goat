//! 配置文件集成测试 — 验证原版 Goat 配置迁移
//!
//! 运行：cargo test -p rgoat-core --test config_migration -- --nocapture

use rgoat_core::core::config::Settings;
use serde_json::json;
use std::path::PathBuf;

/// Write `contents` to a `setting.json` inside a fresh TempDir and return
/// `(tempdir, path)`. The caller must keep the TempDir alive for the test
/// duration so the file is not cleaned up mid-assertion. Using a fixture
/// avoids reading the real user `~/.goat/setting.json`, which made these
/// tests environment-dependent.
fn write_settings_fixture(contents: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("setting.json");
    std::fs::write(&path, contents).expect("write fixture");
    (dir, path)
}

/// 测试原版 Goat setting.json 能被正确加载和迁移
#[test]
fn test_load_original_goat_config() {
    // 旧版格式：顶层 base_url（含 xiaomimimo）+ api_key（tp- 前缀）+ model，
    // 无 providers 数组；provider 字段为类型名 "openai_compatible"，
    // 迁移应从 URL 提取有意义的 provider 名称 → "mimo"。
    let (_dir, path) = write_settings_fixture(
        r#"{
            "provider": "openai_compatible",
            "base_url": "https://api.xiaomimimo.cloud/v1",
            "api_key": "tp-abcdef123456",
            "model": "mimo-v2.5-pro"
        }"#,
    );

    let settings = Settings::load_from_path(&path).expect("load_from_path");

    // ── 迁移后 providers 不应为空 ──
    assert!(
        !settings.providers.is_empty(),
        "After migration, providers should have at least 1 entry"
    );

    let p = &settings.providers[0];

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
    // 旧版格式同时携带 review/sub 模型、hooks 与 workspace 等附加字段。
    // 迁移应将 base_url/api_key/model 移入 providers，但保留这些顶层附加字段。
    let (_dir, path) = write_settings_fixture(
        r#"{
            "provider": "openai_compatible",
            "base_url": "https://api.xiaomimimo.cloud/v1",
            "api_key": "tp-main-key-123",
            "model": "mimo-v2.5-pro",
            "sub_model": "mimo-v2.5-flash",
            "review_model": "mimo-v2.5-pro",
            "review_api_key": "tp-review-key-456",
            "review_base_url": "https://api.xiaomimimo.cloud/v1",
            "max_concurrency": 3,
            "max_depth": 3,
            "hooks": {"tool": ["echo hi"]},
            "workspace": "/tmp/goat-ws"
        }"#,
    );

    let settings = Settings::load_from_path(&path).expect("load_from_path");

    assert!(settings.sub_model.is_some(), "sub_model should be preserved");
    assert_eq!(settings.sub_model.as_deref().unwrap(), "mimo-v2.5-flash");
    assert!(settings.review_model.is_some(), "review_model should be preserved");
    assert_eq!(settings.review_model.as_deref().unwrap(), "mimo-v2.5-pro");
    assert!(settings.review_api_key.is_some(), "review_api_key should be preserved");
    assert!(
        settings.review_api_key.as_deref().unwrap().starts_with("tp-"),
        "review_api_key should keep its value"
    );
    assert!(settings.review_base_url.is_some(), "review_base_url should be preserved");
    assert_eq!(settings.max_concurrency, 3);
    assert_eq!(settings.max_depth, 3);
    assert!(settings.hooks.is_some(), "hooks should be preserved");
    assert!(settings.workspace.is_some(), "workspace should be preserved");
    assert_eq!(settings.workspace.as_deref().unwrap(), "/tmp/goat-ws");
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
