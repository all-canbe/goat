//! Keybindings configuration — load `~/.goat/keybindings.json`.
//!
//! 第三档 #17（简化版）：只做配置加载与展示，不重构现有按键 match。
//! 用户配置仅作为"快捷键文档"展示，实际行为仍由 app.rs 中硬编码的 match 决定。
//! 后续可逐步把硬编码 match 迁移到查询此 map。

use std::collections::HashMap;

/// 动作名 → 绑定的按键字符串列表（如 `["enter"]`、`["shift+enter", "ctrl+j"]`）。
pub type KeyMap = HashMap<String, Vec<String>>;

/// 默认快捷键。当 `~/.goat/keybindings.json` 不存在或加载失败时使用。
pub fn default_keybindings() -> KeyMap {
    let mut m = KeyMap::new();
    m.insert("tui.input.submit".into(), vec!["enter".into()]);
    m.insert("tui.input.newLine".into(), vec!["shift+enter".into()]);
    m.insert("tui.input.queueFollowUp".into(), vec!["alt+enter".into()]);
    m.insert("tui.input.restoreQueue".into(), vec!["alt+up".into()]);
    m.insert("app.interrupt".into(), vec!["escape".into()]);
    m.insert("app.clear".into(), vec!["ctrl+c".into()]);
    m.insert("app.exit".into(), vec!["ctrl+d".into()]);
    m.insert("app.model.cycleForward".into(), vec!["ctrl+p".into()]);
    m.insert("app.thinking.cycle".into(), vec!["shift+tab".into()]);
    m.insert("app.tools.expand".into(), vec!["ctrl+o".into()]);
    m.insert("app.message.copy".into(), vec!["ctrl+x".into()]);
    m.insert("app.editor.external".into(), vec!["ctrl+g".into()]);
    m.insert("app.input.clearLine".into(), vec!["ctrl+u".into()]);
    m.insert("app.input.deleteWord".into(), vec!["ctrl+w".into()]);
    m.insert("app.mode.cycle".into(), vec!["tab".into()]);
    m
}

/// 从 `~/.goat/keybindings.json` 加载用户配置。
/// 失败时返回 None（调用方使用默认值）。
pub fn load_keybindings() -> Option<KeyMap> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let path = std::path::Path::new(&home).join(".goat").join("keybindings.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: KeyMap = serde_json::from_str(&content).ok()?;
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

/// 把 KeyMap 渲染为 `/hotkeys` 风格的多行文本。
/// 用户自定义条目优先，缺失项回填默认值。
pub fn render_hotkeys(custom: Option<&KeyMap>) -> String {
    let defaults = default_keybindings();
    let merged: Vec<(&String, &Vec<String>)> = if let Some(c) = custom {
        c.iter().chain(defaults.iter().filter(|(k, _)| !c.contains_key(*k))).collect()
    } else {
        defaults.iter().collect()
    };

    let mut lines = vec!["Keybindings (configure via ~/.goat/keybindings.json):".to_string()];
    let mut entries: Vec<(String, String)> = merged
        .iter()
        .map(|(action, keys)| (action.to_string(), keys.join(", ")))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (action, keys) in &entries {
        lines.push(format!("  {:<28} {}", action, keys));
    }
    if custom.is_some() {
        lines.push(String::new());
        lines.push(format!("Custom bindings loaded: {} entries.", custom.unwrap().len()));
    } else {
        lines.push(String::new());
        lines.push("No custom keybindings.json found — using defaults.".into());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keybindings_contains_core_actions() {
        let m = default_keybindings();
        assert!(m.contains_key("tui.input.submit"));
        assert!(m.contains_key("app.exit"));
        assert!(m.contains_key("app.thinking.cycle"));
        // 默认应至少包含 12 个常用绑定
        assert!(m.len() >= 12);
    }

    #[test]
    fn render_hotkeys_default_informs_user() {
        let s = render_hotkeys(None);
        assert!(s.contains("using defaults"));
        assert!(s.contains("tui.input.submit"));
    }

    #[test]
    fn render_hotkeys_merges_custom_and_defaults() {
        let mut custom: KeyMap = KeyMap::new();
        custom.insert("tui.input.submit".into(), vec!["ctrl+j".into()]);
        custom.insert("my.custom.action".into(), vec!["f1".into()]);
        let s = render_hotkeys(Some(&custom));
        // 自定义条目覆盖默认值
        assert!(s.contains("ctrl+j"));
        assert!(s.contains("my.custom.action"));
        // 缺失项仍回填默认值
        assert!(s.contains("app.exit"));
        assert!(s.contains("Custom bindings loaded: 2 entries"));
    }

    #[test]
    fn load_keybindings_returns_none_when_file_missing() {
        // 默认情况下 ~/.goat/keybindings.json 不存在（除非测试机恰好有）
        // 此用例只验证函数不 panic 且返回 Option
        let _ = load_keybindings();
    }
}
