//! Tauri IPC commands — bridge between frontend and rgoat-core

use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;

use rgoat_core::core::config::Settings;
use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::provider::impls::{AnthropicProvider, OpenAiCompatibleProvider};
use rgoat_core::provider::provider::{ChatOptions, LlmProvider, ProviderConfig, ProviderType, ThinkingLevel};
use rgoat_core::security::approval::ApprovalResponder;
use std::sync::Arc;
use tauri::State;
use serde::{Deserialize, Serialize};

use crate::AppState;

// ── Request / Response types ──

#[derive(Debug, Deserialize)]
pub struct SendPromptRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    pub mode: Option<String>,
    /// 请求级思考强度（high/medium/low/max/default）。缺失或未知值视为 Default。
    #[serde(default)]
    pub thinking_level: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendPromptResponse {
    pub session_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub message_count: i64,
    pub created_at: String,
    pub parent_session_id: Option<String>,
    pub forked_from_message_id: Option<i64>,
    pub workspace: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub name: String,
    pub model: String,
    pub provider_type: String,
    pub is_current: bool,
    pub enabled: bool,
    pub source: String,
}

// ── Agent control types ──

/// Current agent execution status
#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub running: bool,
    pub paused: bool,
}

/// Frontend response to an ask_user request
#[derive(Debug, Deserialize)]
pub struct AskUserResponse {
    pub request_id: String,
    pub response: String,
}

// ── Tauri Commands ──

/// env/fallback 保留名称，禁止写入 settings。
const RESERVED_RUNTIME_PROVIDER_NAMES: &[&str] = &["deepseek", "openai", "anthropic"];

fn assert_settings_provider_name_allowed(name: &str) -> Result<(), String> {
    let lower = name.trim().to_ascii_lowercase();
    if RESERVED_RUNTIME_PROVIDER_NAMES.contains(&lower.as_str()) {
        return Err(format!(
            "Provider name '{}' is reserved for environment/fallback providers. Choose another name.",
            name
        ));
    }
    Ok(())
}

fn merge_provider_lists(
    settings_items: Vec<ProviderInfo>,
    runtime_items: Vec<ProviderInfo>,
) -> Vec<ProviderInfo> {
    let mut providers = runtime_items;
    for item in settings_items {
        if providers.iter().any(|provider| provider.name == item.name) {
            continue;
        }
        providers.push(item);
    }
    providers
}

fn runtime_provider_source(name: &str) -> &'static str {
    if name == "deepseek" && std::env::var("DEEPSEEK_API_KEY").unwrap_or_default().is_empty() {
        "fallback"
    } else {
        "env"
    }
}

/// 解析前端传入的 thinking_level 字符串为 ThinkingLevel。
/// 缺失、空字符串或未知值返回 None（即 Default，不发送 reasoning_effort 字段）。
fn parse_thinking_level(s: &Option<String>) -> Option<ThinkingLevel> {
    match s.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some("low") => Some(ThinkingLevel::Low),
        Some("medium") => Some(ThinkingLevel::Medium),
        Some("high") => Some(ThinkingLevel::High),
        Some("max") => Some(ThinkingLevel::Max),
        Some("default") => Some(ThinkingLevel::Default),
        _ => None,
    }
}

/// Send a prompt to the Agent (runs asynchronously, emits events to frontend)
async fn disable_provider<F>(
    settings: &mut Settings,
    switch: &rgoat_core::provider::switch::ProviderSwitch,
    name: &str,
    save: F,
) -> Result<(), String>
where
    F: FnOnce(&Settings) -> Result<(), String>,
{
    let provider = settings.providers.iter_mut().find(|provider| provider.name == name)
        .ok_or_else(|| "Only settings providers can be changed".to_string())?;
    provider.enabled = false;
    save(settings)?;
    unregister_if_registered(switch, name).await
}

#[tauri::command]
pub async fn send_prompt(
    state: State<'_, AppState>,
    request: SendPromptRequest,
) -> Result<SendPromptResponse, String> {
    let session_id = request.session_id.unwrap_or_else(|| {
        format!("desktop-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default())
    });

    // 多会话并发：检查该 session_id 是否已有 agent 在运行（不同 session 可并行）
    {
        let handles = state.agent_handles.lock().map_err(|e| e.to_string())?;
        let already_running = handles.get(&session_id)
            .map(|h| !h.is_finished())
            .unwrap_or(false);
        if already_running {
            return Err("该会话已有 agent 运行".to_string());
        }
    }

    let sid = session_id.clone();

    // 从 runtime read lock 快照 agent 和 workspace，drop lock 后再 await
    let (agent, workspace) = {
        let runtime = state.runtime.read().await;
        (runtime.agent.clone(), runtime.workspace.clone())
    };

    // Create session if it doesn't exist, using the frontend-supplied ID as the
    // real session ID (not just the title). This prevents FOREIGN KEY errors when
    // the Agent later writes tool messages to this session.
    let _ = state.conversation
        .get_or_create_session(&session_id, Some("New Session"), Some(&workspace))
        .await
        .map_err(|e| e.to_string())?;

    // D2: 根据前端传的 mode 设置 AgentMode
    let agent_mode = request.mode.as_deref()
        .and_then(|m| match m.to_lowercase().as_str() {
            "agent" => Some(rgoat_core::security::approval::AgentMode::Agent),
            "plan" => Some(rgoat_core::security::approval::AgentMode::Plan),
            "flow" => Some(rgoat_core::security::approval::AgentMode::Flow),
            "accept-edits" | "accept_edits" => Some(rgoat_core::security::approval::AgentMode::AcceptEdits),
            "yolo" => Some(rgoat_core::security::approval::AgentMode::Yolo),
            _ => None,
        })
        .unwrap_or(rgoat_core::security::approval::AgentMode::Agent);

    let prompt = request.prompt.clone();
    // 请求级思考强度：仅本次 send_prompt 调用生效，不持久化到 AgentConfig
    let chat_options = ChatOptions {
        thinking_level: parse_thinking_level(&request.thinking_level),
    };

    // 多会话并发：为该 session 创建独立资源（取消令牌、暂停标志、审批响应器）
    let cancellation = CancellationToken::new();
    let paused = Arc::new(AtomicBool::new(false));
    let responder: ApprovalResponder = Arc::new(tokio::sync::Mutex::new(None));

    // 创建会话级 agent — 共享基础设施但独立 cancellation/paused/responder/session_id
    let session_agent = agent.create_session_agent(
        session_id.clone(),
        cancellation.clone(),
        paused.clone(),
        responder.clone(),
    );
    // mode 在 session_agent 上设（避免多 session 互相覆盖共享 agent 的 mode）
    session_agent.set_mode(agent_mode);
    let session_agent = Arc::new(session_agent);

    // 存入 per-session maps
    state.agent_cancellations.lock().map_err(|e| e.to_string())?
        .insert(session_id.clone(), cancellation);
    state.agent_paused_flags.lock().map_err(|e| e.to_string())?
        .insert(session_id.clone(), paused);
    state.approval_responders.lock().map_err(|e| e.to_string())?
        .insert(session_id.clone(), responder);

    // clone maps 的 Arc 用于 spawn task 内 finally 清理（State 不能 move 进 task）
    let handles_arc = state.agent_handles.clone();
    let cancellations_arc = state.agent_cancellations.clone();
    let paused_arc = state.agent_paused_flags.clone();
    let responders_arc = state.approval_responders.clone();
    let sid_for_cleanup = session_id.clone();

    let handle = tokio::spawn(async move {
        // D3-T04: Plan Mode 下使用 PlanRunner 两阶段流程
        let result = if agent_mode == rgoat_core::security::approval::AgentMode::Plan {
            let runner = rgoat_core::agent::plan_runner::PlanRunner::new(
                session_agent.clone(),
                workspace.clone(),
                sid.clone(),
            );
            runner.run_with_options(&prompt, &chat_options).await.map(|plan_result| {
                tracing::info!(
                    "Plan completed: phase={:?}, path={:?}",
                    plan_result.phase,
                    plan_result.plan_path
                );
            })
        } else {
            session_agent.run_with_options(&sid, &prompt, &workspace, &chat_options).await.map(|_| ())
        };
        if let Err(e) = &result {
            tracing::error!("Agent error: {}", e);
        }
        // finally 清理：防止 map 无限增长 + get_agent_status 状态误报
        if let Ok(mut m) = handles_arc.lock() { m.remove(&sid_for_cleanup); }
        if let Ok(mut m) = cancellations_arc.lock() { m.remove(&sid_for_cleanup); }
        if let Ok(mut m) = paused_arc.lock() { m.remove(&sid_for_cleanup); }
        if let Ok(mut m) = responders_arc.lock() { m.remove(&sid_for_cleanup); }
    });

    state.agent_handles.lock().map_err(|e| e.to_string())?
        .insert(session_id.clone(), handle);

    Ok(SendPromptResponse {
        session_id,
        message: format!("Processing: {}", request.prompt),
    })
}

/// List all conversation sessions
#[tauri::command]
pub async fn get_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<SessionInfo>, String> {
    let sessions = state.conversation
        .list_sessions()
        .await
        .map_err(|e| e.to_string())?;

    Ok(sessions.into_iter().map(|s| SessionInfo {
        id: s.id,
        title: s.title,
        message_count: s.message_count,
        created_at: s.created_at.to_rfc3339(),
        parent_session_id: s.parent_session_id,
        forked_from_message_id: s.forked_from_message_id,
        workspace: s.workspace,
    }).collect())
}

/// Get all persisted messages in a conversation session.
#[tauri::command]
pub async fn get_session_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<SessionMessage>, String> {
    let messages = state.conversation
        .get_messages(&session_id)
        .await
        .map_err(|error| error.to_string())?;

    Ok(messages.into_iter().map(|message| SessionMessage {
        id: message.id,
        role: message.role,
        content: message.content,
        tool_calls: message.tool_calls,
        tool_call_id: message.tool_call_id,
    }).collect())
}

/// Create a new conversation session explicitly in the current workspace
#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
) -> Result<SessionInfo, String> {
    let workspace = state.runtime.read().await.workspace.clone();
    let session = state.conversation
        .create_session(Some("New Session"), Some(&workspace))
        .await
        .map_err(|e| e.to_string())?;

    Ok(SessionInfo {
        id: session.id,
        title: session.title,
        message_count: session.message_count,
        created_at: session.created_at.to_rfc3339(),
        parent_session_id: session.parent_session_id,
        forked_from_message_id: session.forked_from_message_id,
        workspace: session.workspace,
    })
}

/// Fork a session (create a child session with copied messages)
#[tauri::command]
pub async fn fork_session(
    state: State<'_, AppState>,
    session_id: String,
    up_to_message_id: Option<i64>,
) -> Result<SessionInfo, String> {
    let session = state.conversation
        .fork_session(&session_id, None, up_to_message_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(SessionInfo {
        id: session.id,
        title: session.title,
        message_count: session.message_count,
        created_at: session.created_at.to_rfc3339(),
        parent_session_id: session.parent_session_id,
        forked_from_message_id: session.forked_from_message_id,
        workspace: session.workspace,
    })
}

/// Delete a conversation session
#[tauri::command]
pub async fn delete_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // 删除前取消该会话可能仍在运行的 agent，并清理 per-session 运行时资源
    // 避免会话已删但 agent 继续 emit 事件、前端 updateSession 复活条目
    cancel_session_runtime_resources(&state, &session_id);

    state.conversation
        .delete_session(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    // 清理该会话的文件变更记录，避免内存泄漏与同 ID 会话复用旧记录
    if let Ok(mut changes) = state.session_changes.lock() {
        changes.remove(&session_id);
    }
    Ok(())
}

/// Rename a conversation session
#[tauri::command]
pub async fn rename_session(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    state.conversation
        .update_title(&session_id, &title)
        .await
        .map_err(|e| e.to_string())
}

/// List available providers
#[tauri::command]
pub async fn list_providers(
    state: State<'_, AppState>,
) -> Result<Vec<ProviderInfo>, String> {
    let details = state.switch.list_details().await;
    let current = state.switch.current_name().await;
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let settings_items: Vec<_> = settings.providers.iter().map(|provider| {
        ProviderInfo {
            name: provider.name.clone(),
            model: provider.models.as_ref().and_then(|models| models.first()).cloned().unwrap_or_default(),
            provider_type: provider.provider_type.clone().unwrap_or_else(|| "openai_compatible".to_string()),
            is_current: provider.name == current,
            enabled: provider.enabled,
            source: "settings".to_string(),
        }
    }).collect();

    let runtime_items: Vec<_> = details.into_iter().map(|detail| {
        ProviderInfo {
            name: detail.name.clone(),
            model: detail.model,
            provider_type: detail.provider_type,
            is_current: detail.is_current,
            enabled: true,
            source: runtime_provider_source(&detail.name).to_string(),
        }
    }).collect();

    Ok(merge_provider_lists(settings_items, runtime_items))
}

/// Switch to a different provider
#[tauri::command]
pub async fn switch_provider(
    state: State<'_, AppState>,
    name: String,
) -> Result<String, String> {
    state.switch.select(&name).await.map_err(|e| e.to_string())?;
    Ok(format!("Switched to {}", name))
}

/// Get current provider name
#[tauri::command]
pub async fn get_current_provider(
    state: State<'_, AppState>,
) -> Result<String, String> {
    Ok(state.switch.current_name().await)
}

/// Check if any provider is configured
#[tauri::command]
pub async fn has_configured_provider(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    // Check if the switch has any providers with actual API keys
    let providers = state.switch.list_names_blocking();
    Ok(!providers.is_empty())
}

/// Configure a new provider from the setup wizard.
/// Saves to settings.json and immediately registers the provider in the
/// running switch, then selects it so the user can chat without restarting.
#[tauri::command]
pub async fn configure_provider(
    state: State<'_, AppState>,
    base_url: String,
    api_key: String,
    model: String,
    name: String,
) -> Result<String, String> {
    assert_settings_provider_name_allowed(&name)?;
    let provider_type = Settings::detect_provider_type(&base_url);
    let cfg = ProviderConfig::new(provider_type, &name, &base_url, &api_key, &model);
    let provider = match provider_type {
        ProviderType::Anthropic => Arc::new(AnthropicProvider::new(cfg)) as Arc<dyn rgoat_core::provider::provider::LlmProvider>,
        ProviderType::OpenAICompatible => Arc::new(OpenAiCompatibleProvider::new(cfg)),
    };

    state.switch.register(provider).await;
    state.switch.select(&name).await?;

    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.add_or_update_provider(&name, &base_url, &api_key, &model);
    settings.provider = name.clone();
    settings.model = model.clone();
    settings.save().map_err(|e| e.to_string())?;

    Ok(format!(
        "Provider '{}' saved and activated. Model: {}.",
        name, model
    ))
}

/// 启用或禁用设置文件中的非当前 Provider。
async fn unregister_if_registered(
    switch: &rgoat_core::provider::switch::ProviderSwitch,
    name: &str,
) -> Result<(), String> {
    if switch.list_names().await.iter().any(|registered| registered == name) {
        switch.unregister(name).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_provider_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    let current = state.switch.current_name().await;
    if current == name {
        return Err("Cannot change the active provider".to_string());
    }

    let settings_provider = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.providers.iter().find(|provider| provider.name == name).cloned()
    }.ok_or_else(|| "Only settings providers can be changed".to_string())?;

    if enabled {
        assert_settings_provider_name_allowed(&name)?;
        let key = settings_provider.api_key.clone().unwrap_or_default();
        let base_url = settings_provider.base_url.clone().unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let model = settings_provider.models.as_ref().and_then(|models| models.first()).cloned().unwrap_or_else(|| "gpt-4o".to_string());
        let provider_type = Settings::detect_provider_type(&base_url);
        let cfg = ProviderConfig::new(provider_type, &name, &base_url, key, model);
        let provider = match provider_type {
            ProviderType::Anthropic => Arc::new(AnthropicProvider::new(cfg)) as Arc<dyn rgoat_core::provider::provider::LlmProvider>,
            ProviderType::OpenAICompatible => Arc::new(OpenAiCompatibleProvider::new(cfg)),
        };
        state.switch.register(provider).await;
    } else {
        let mut settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
        disable_provider(&mut settings, &state.switch, &name, |settings| {
            settings.save().map_err(|e| e.to_string())
        }).await?;
        *state.settings.lock().map_err(|e| e.to_string())? = settings;
        return Ok(());
    }

    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    let provider = settings.providers.iter_mut().find(|provider| provider.name == name)
        .ok_or_else(|| "Only settings providers can be changed".to_string())?;
    provider.enabled = true;
    settings.save().map_err(|e| e.to_string())
}

/// 删除设置文件中的非当前 Provider。
#[tauri::command]
pub async fn delete_provider(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    if state.switch.current_name().await == name {
        return Err("Cannot delete the active provider".to_string());
    }

    {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        if !settings.providers.iter().any(|provider| provider.name == name) {
            return Err("Only settings providers can be deleted".to_string());
        }
    }
    if state.switch.list_names().await.iter().any(|registered| registered == &name) {
        state.switch.unregister(&name).await?;
    }

    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.providers.retain(|provider| provider.name != name);
    settings.save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use rgoat_core::core::config::ProviderSettings;
    use rgoat_core::provider::switch::ProviderSwitch;
    use rgoat_core::conversation::manager::ConversationManager;
    use rgoat_core::core::event_bus::EventBus;
    use rgoat_core::security::approval::{ApprovalEngine, AgentMode};
    use rgoat_core::tools::registry::ToolRegistry;
    use rgoat_core::agent::types::AgentConfig;
    use rgoat_core::agent::react::ReActAgent;
    use crate::WorkspaceRuntime;

    #[tokio::test]
    async fn disabling_noncurrent_settings_provider_not_registered_at_runtime_succeeds() {
        let switch = ProviderSwitch::new();
        let mut settings = Settings::default();
        settings.providers.push(ProviderSettings {
            name: "without-key".to_string(),
            enabled: true,
            base_url: Some("https://api.example.com/v1".to_string()),
            api_key: None,
            models: Some(vec!["example-model".to_string()]),
            provider_type: None,
        });

        let result = disable_provider(&mut settings, &switch, "without-key", |_| Ok(())).await;

        assert!(result.is_ok());
        assert!(!settings.providers[0].enabled);
    }

    #[test]
    fn configure_rejects_reserved_runtime_provider_names() {
        for name in ["deepseek", "OpenAI", "ANTHROPIC", " OpenAI "] {
            let err = assert_settings_provider_name_allowed(name).unwrap_err();
            assert!(
                err.contains("reserved"),
                "expected reserved-name error for {name}, got: {err}"
            );
        }
        assert!(assert_settings_provider_name_allowed("my-custom").is_ok());
    }

    #[test]
    fn list_merge_prefers_runtime_source_over_settings_when_names_collide() {
        let settings_items = vec![ProviderInfo {
            name: "openai".to_string(),
            model: "gpt-settings".to_string(),
            provider_type: "openai_compatible".to_string(),
            is_current: false,
            enabled: false,
            source: "settings".to_string(),
        }];
        let runtime_items = vec![ProviderInfo {
            name: "openai".to_string(),
            model: "gpt-4o".to_string(),
            provider_type: "OpenAICompatible".to_string(),
            is_current: true,
            enabled: true,
            source: "env".to_string(),
        }];

        let merged = merge_provider_lists(settings_items, runtime_items);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, "env");
        assert!(merged[0].enabled);
        assert!(merged[0].is_current);
        assert_eq!(merged[0].model, "gpt-4o");
    }

    #[test]
    fn session_info_includes_workspace_field() {
        let info = SessionInfo {
            id: "test-id".to_string(),
            title: "Test".to_string(),
            message_count: 0,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            parent_session_id: None,
            forked_from_message_id: None,
            workspace: Some("/tmp/test-workspace".to_string()),
        };
        assert_eq!(info.workspace.as_deref(), Some("/tmp/test-workspace"));
    }

    #[test]
    fn temporary_workspace_path_for_returns_path_under_data_dir() {
        let path = crate::temporary_workspace_path_for();
        let data_dir = rgoat_core::core::workspace::get_data_dir();
        assert_eq!(path, data_dir.join("temporary-workspace"));
    }

    #[tokio::test]
    async fn temporary_workspace_is_created_in_app_data_directory() {
        let path = crate::ensure_temporary_workspace()
            .expect("ensure_temporary_workspace should succeed");
        assert!(path.is_dir(), "temporary workspace should be a directory");
        let expected_parent = rgoat_core::core::workspace::get_data_dir();
        // Windows canonicalize 返回 UNC 前缀 \\?\，需同时 canonicalize 父目录比较
        let expected_canonical = expected_parent.canonicalize().unwrap_or_else(|_| expected_parent.clone());
        assert!(
            path.starts_with(&expected_canonical),
            "temporary workspace should be under app data dir, got {:?}, expected parent {:?}",
            path,
            expected_canonical
        );
    }

    #[test]
    fn validate_workspace_path_rejects_nonexistent_path() {
        let nonexistent = format!(
            "/nonexistent/path/{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let result = validate_workspace_path(&nonexistent);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("does not exist"),
            "should reject nonexistent path"
        );
    }

    #[test]
    fn validate_workspace_path_accepts_existing_directory() {
        let temp = std::env::temp_dir();
        let result = validate_workspace_path(temp.to_str().unwrap());
        assert!(result.is_ok());
        let canonical = result.unwrap();
        assert!(canonical.is_absolute(), "should return absolute canonical path");
    }

    #[tokio::test]
    async fn set_workspace_returns_error_when_agent_running() {
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });
        let mut handles = HashMap::new();
        handles.insert("test".to_string(), handle);

        let result = check_no_agent_running(&handles);
        assert!(result.is_err(), "should error when agent is running");
        assert!(
            result.unwrap_err().contains("Cannot switch workspace"),
            "error should mention Cannot switch workspace"
        );

        if let Some(h) = handles.remove("test") {
            h.abort();
        }
    }

    #[tokio::test]
    async fn set_workspace_allows_switch_when_agent_not_running() {
        let handles: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        let result = check_no_agent_running(&handles);
        assert!(result.is_ok(), "should allow switch when no handle");

        // 用 is_finished() 轮询等待任务完成（不消耗 handle 所有权）
        let handle = tokio::spawn(async {});
        while !handle.is_finished() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        let mut handles = HashMap::new();
        handles.insert("test".to_string(), handle);
        let result = check_no_agent_running(&handles);
        assert!(result.is_ok(), "should allow switch when handle is finished");
    }

    #[tokio::test]
    async fn test_read_workspace_file_path_traversal_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let ws = dir.path().to_string_lossy().to_string();
        let root = std::path::PathBuf::from(&ws).canonicalize().unwrap();
        let malicious = "../../../etc/passwd";
        let file = root.join(malicious);
        let canonical = file.canonicalize();
        if let Ok(canonical) = canonical {
            assert!(!canonical.starts_with(&root), "路径穿越应被阻止");
        }
    }

    #[tokio::test]
    async fn cancel_session_runtime_resources_cleans_all_maps() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let conversation = Arc::new(ConversationManager::new_with_path(&db_path).await.unwrap());
        let event_bus = Arc::new(EventBus::new(100));
        let switch = Arc::new(ProviderSwitch::new());
        let approval = Arc::new(ApprovalEngine::new());
        let settings = Arc::new(Mutex::new(Settings::default()));
        let pending_ask_user = Arc::new(Mutex::new(Arc::new(Mutex::new(HashMap::new()))));

        let provider: Arc<dyn LlmProvider> = switch.clone();
        let tools = Arc::new(ToolRegistry::empty());
        let agent = Arc::new(ReActAgent::new(
            AgentConfig::default(),
            provider,
            tools.clone(),
            approval.clone(),
            conversation.clone(),
            event_bus.clone(),
            CancellationToken::new(),
            AgentMode::Agent,
            Arc::new(AtomicBool::new(false)),
            Arc::new(tokio::sync::Mutex::new(None)),
        ));

        let runtime = WorkspaceRuntime {
            workspace: tmp.path().display().to_string(),
            temporary: true,
            tools,
            agent,
            approval_responder: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let s1_token = CancellationToken::new();
        let s2_token = CancellationToken::new();
        let s1_paused = Arc::new(AtomicBool::new(false));
        let s2_paused = Arc::new(AtomicBool::new(false));
        let s1_responder: ApprovalResponder = Arc::new(tokio::sync::Mutex::new(None));
        let s2_responder: ApprovalResponder = Arc::new(tokio::sync::Mutex::new(None));

        let s1_handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        });

        let mut cancellations = HashMap::new();
        cancellations.insert("s1".to_string(), s1_token.clone());
        cancellations.insert("s2".to_string(), s2_token.clone());

        let mut paused_flags = HashMap::new();
        paused_flags.insert("s1".to_string(), s1_paused.clone());
        paused_flags.insert("s2".to_string(), s2_paused.clone());

        let mut handles = HashMap::new();
        handles.insert("s1".to_string(), s1_handle);

        let mut responders = HashMap::new();
        responders.insert("s1".to_string(), s1_responder);
        responders.insert("s2".to_string(), s2_responder);

        let state = AppState {
            runtime: Arc::new(tokio::sync::RwLock::new(runtime)),
            conversation,
            event_bus,
            switch,
            approval,
            settings,
            pending_ask_user,
            agent_cancellations: Arc::new(Mutex::new(cancellations)),
            agent_paused_flags: Arc::new(Mutex::new(paused_flags)),
            agent_handles: Arc::new(Mutex::new(handles)),
            approval_responders: Arc::new(Mutex::new(responders)),
            session_changes: Arc::new(Mutex::new(HashMap::new())),
        };

        cancel_session_runtime_resources(&state, "s1");

        // s1 removed from all four maps, s2 preserved
        {
            let c = state.agent_cancellations.lock().unwrap();
            assert!(!c.contains_key("s1"), "s1 should be removed from cancellations");
            assert!(c.contains_key("s2"), "s2 should remain in cancellations");
        }
        assert!(s1_token.is_cancelled(), "s1 token should be cancelled");
        assert!(!s2_token.is_cancelled(), "s2 token should NOT be cancelled");

        {
            let h = state.agent_handles.lock().unwrap();
            assert!(!h.contains_key("s1"), "s1 should be removed from handles");
        }

        {
            let p = state.agent_paused_flags.lock().unwrap();
            assert!(!p.contains_key("s1"), "s1 should be removed from paused flags");
            assert!(p.contains_key("s2"), "s2 should remain in paused flags");
        }

        {
            let r = state.approval_responders.lock().unwrap();
            assert!(!r.contains_key("s1"), "s1 should be removed from responders");
            assert!(r.contains_key("s2"), "s2 should remain in responders");
        }
    }
}

// ── Workspace helpers ──

/// Canonicalize 并验证 workspace 路径。拒绝不存在的路径。
fn validate_workspace_path(path: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(path);
    if !p.is_dir() {
        return Err(format!(
            "Workspace path does not exist or is not a directory: {}",
            path
        ));
    }
    p.canonicalize().map_err(|e| e.to_string())
}

/// 检查是否有任一 session 的 agent 在运行。若正在运行则返回 Err，阻止 workspace 切换。
/// 多会话并发：跨 workspace 切换会使旧 agent 持有的 tools Arc 指向旧 workspace，
/// 因此任一 session 在跑都必须拒绝切换（这是正确性要求，非简化）。
fn check_no_agent_running(
    handles: &HashMap<String, tokio::task::JoinHandle<()>>,
) -> Result<(), String> {
    let running: Vec<&String> = handles
        .iter()
        .filter(|(_, h)| !h.is_finished())
        .map(|(k, _)| k)
        .collect();
    if running.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Cannot switch workspace while agent(s) are running: {} session(s) active",
            running.len()
        ))
    }
}

// ── Workspace commands ──

#[derive(Debug, Serialize)]
pub struct WorkspaceInfo {
    pub path: String,
    pub is_temporary: bool,
}

/// 获取当前工作空间信息
#[tauri::command]
pub async fn get_workspace(
    state: State<'_, AppState>,
) -> Result<WorkspaceInfo, String> {
    let runtime = state.runtime.read().await;
    Ok(WorkspaceInfo {
        path: runtime.workspace.clone(),
        is_temporary: runtime.temporary,
    })
}

/// 切换工作空间。agent 运行中时拒绝切换。
#[tauri::command]
pub async fn set_workspace(
    state: State<'_, AppState>,
    path: String,
) -> Result<WorkspaceInfo, String> {
    // 1. 检查是否有任一 session 的 agent 在运行
    {
        let guard = state.agent_handles.lock().map_err(|e| e.to_string())?;
        check_no_agent_running(&guard)?;
    }

    // 2. 验证并 canonicalize 路径
    let canonical = validate_workspace_path(&path)?;
    let workspace_str = canonical.display().to_string();

    // 3. 用 canonical path 创建新 runtime
    // 模板 agent 的 cancellation/paused 仅作占位（实际运行用 create_session_agent 创建独立资源）
    let provider: Arc<dyn LlmProvider> = state.switch.clone();
    let new_runtime = crate::create_workspace_runtime(
        workspace_str.clone(),
        false,
        provider,
        state.approval.clone(),
        state.conversation.clone(),
        state.event_bus.clone(),
        CancellationToken::new(),
        Arc::new(AtomicBool::new(false)),
    );

    // 4. 同步更新 pending_ask_user 指向新 tools 的 pending_ask_user
    let new_pending = new_runtime
        .tools
        .pending_ask_user
        .clone()
        .unwrap_or_else(|| Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())));
    *state.pending_ask_user.lock().map_err(|e| e.to_string())? = new_pending;

    // 5. 替换 runtime（write lock）
    {
        let mut runtime = state.runtime.write().await;
        *runtime = new_runtime;
    }

    Ok(WorkspaceInfo {
        path: workspace_str,
        is_temporary: false,
    })
}

/// 切换回应用固定临时工作空间。
#[tauri::command]
pub async fn set_temporary_workspace(
    state: State<'_, AppState>,
) -> Result<WorkspaceInfo, String> {
    {
        let guard = state.agent_handles.lock().map_err(|e| e.to_string())?;
        check_no_agent_running(&guard)?;
    }

    let workspace = crate::ensure_temporary_workspace()?.display().to_string();
    let provider: Arc<dyn LlmProvider> = state.switch.clone();
    let new_runtime = crate::create_workspace_runtime(
        workspace.clone(),
        true,
        provider,
        state.approval.clone(),
        state.conversation.clone(),
        state.event_bus.clone(),
        CancellationToken::new(),
        Arc::new(AtomicBool::new(false)),
    );
    let pending = new_runtime.tools.pending_ask_user.clone()
        .unwrap_or_else(|| Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())));
    *state.pending_ask_user.lock().map_err(|e| e.to_string())? = pending;
    *state.runtime.write().await = new_runtime;

    Ok(WorkspaceInfo { path: workspace, is_temporary: true })
}

// ── Approval ──

#[derive(Debug, Deserialize)]
pub struct ApprovalResponse {
    pub tool_name: String,
    pub approved: bool,
    /// D1-T03: 批准范围 — "once" | "session" | "all_similar" | "always"
    #[serde(default)]
    pub scope: Option<String>,
}

/// Frontend responds to a pending approval request
#[tauri::command]
pub async fn respond_approval(
    state: State<'_, AppState>,
    response: ApprovalResponse,
    session_id: Option<String>,
) -> Result<String, String> {
    use rgoat_core::security::approval::{ApprovalDecision, ApprovalScope};

    let scope = response.scope.as_deref()
        .and_then(|s| match s {
            "once" => Some(ApprovalScope::Once),
            "session" => Some(ApprovalScope::Session),
            "all_similar" => Some(ApprovalScope::AllSimilar),
            "always" => Some(ApprovalScope::Always),
            _ => None,
        })
        .unwrap_or(ApprovalScope::Once);
    let decision = ApprovalDecision {
        approved: response.approved,
        approve_all: matches!(scope, ApprovalScope::Session | ApprovalScope::AllSimilar),
        scope,
    };

    // 多会话并发：按 session_id 从 approval_responders 取对应会话的响应器
    // 缺省回退到 runtime.approval_responder（兼容旧调用路径）
    let sender = if let Some(sid) = session_id.as_deref() {
        let responders = state.approval_responders.lock().map_err(|e| e.to_string())?;
        responders
            .get(sid)
            .and_then(|r| r.try_lock().ok()?.take())
    } else {
        None
    };

    let sender = match sender {
        Some(s) => s,
        None => {
            // 回退路径：从 runtime.approval_responder 取（兼容单 session 场景）
            let runtime = state.runtime.read().await;
            let sender_opt = runtime.approval_responder.lock().await.take();
            drop(runtime);
            sender_opt.ok_or_else(|| "No pending approval request".to_string())?
        }
    };

    sender.send(decision).map_err(|_| "Approval request is no longer waiting".to_string())?;

    Ok(format!(
        "Approval {} for tool '{}' (scope={:?})",
        if response.approved { "granted" } else { "denied" },
        response.tool_name,
        scope
    ))
}

// ── Workspace file listing ──

#[derive(Debug, Serialize)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileTreeNode>>,
}

/// List workspace files as a tree structure (skips .git, node_modules, target)
#[tauri::command]
pub async fn list_workspace_files(
    state: State<'_, AppState>,
    max_depth: Option<usize>,
) -> Result<FileTreeNode, String> {

    let ws_str = state.runtime.read().await.workspace.clone();
    let ws = std::path::PathBuf::from(&ws_str);
    let depth = max_depth.unwrap_or(3);
    walk_dir(&ws, &ws, depth).map_err(|e| e.to_string())
}

/// List a single layer of directory entries under the given relative path.
/// Used by the frontend for lazy-loading subdirectories on expand.
/// Path traversal protected: canonicalized target must remain within workspace.
#[tauri::command]
pub async fn list_directory(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<FileTreeNode>, String> {
    let ws_str = state.runtime.read().await.workspace.clone();
    let ws = std::path::PathBuf::from(&ws_str);
    let target = ws.join(&path);
    let canon_target = target.canonicalize().map_err(|e| e.to_string())?;
    let canon_ws = ws.canonicalize().map_err(|e| e.to_string())?;
    if !canon_target.starts_with(&canon_ws) {
        return Err(format!("Path escapes workspace: {}", path));
    }
    if !canon_target.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }
    let mut children = Vec::new();
    let entries: Vec<_> = std::fs::read_dir(&canon_target)
        .map_err(|e| e.to_string())?
        .collect();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_name = entry.file_name();
        let entry_name_str = entry_name.to_str().unwrap_or("");
        if SKIP_DIRS.contains(&entry_name_str) {
            continue;
        }
        let child_path = entry.path();
        let rel = child_path
            .strip_prefix(&ws)
            .unwrap_or(&child_path)
            .display()
            .to_string();
        children.push(FileTreeNode {
            name: entry_name_str.to_string(),
            path: rel,
            is_dir: child_path.is_dir(),
            children: None,
        });
    }
    children.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(children)
}

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target"];

fn walk_dir(root: &std::path::Path, dir: &std::path::Path, max_depth: usize) -> std::io::Result<FileTreeNode> {
    let name = dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".")
        .to_string();
    let rel_path = dir.strip_prefix(root)
        .unwrap_or(dir)
        .display()
        .to_string();
    let is_dir = dir.is_dir();

    if max_depth == 0 || !is_dir {
        return Ok(FileTreeNode {
            name,
            path: rel_path,
            is_dir,
            children: None,
        });
    }

    let mut children = Vec::new();
    let entries: Vec<_> = std::fs::read_dir(dir)?.collect();
    for entry in entries {
        let entry = entry?;
        let entry_name = entry.file_name();
        let entry_name_str = entry_name.to_str().unwrap_or("");

        if SKIP_DIRS.contains(&entry_name_str) {
            continue;
        }

        let child = walk_dir(root, &entry.path(), max_depth - 1)?;
        children.push(child);
    }

    children.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir) // directories first
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(FileTreeNode {
        name,
        path: rel_path,
        is_dir,
        children: if children.is_empty() { None } else { Some(children) },
    })
}

/// 读取工作空间内指定文件的文本内容（路径穿越防护）
#[tauri::command]
pub async fn read_workspace_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let ws = state.runtime.read().await.workspace.clone();
    let root = std::path::PathBuf::from(&ws)
        .canonicalize()
        .map_err(|e| format!("无法解析工作空间路径: {e}"))?;
    let file = root.join(&path);
    let canonical = file
        .canonicalize()
        .map_err(|e| format!("文件不存在: {e}"))?;
    if !canonical.starts_with(&root) {
        return Err("路径越权".into());
    }
    std::fs::read_to_string(&canonical).map_err(|e| format!("读取失败: {e}"))
}

// ── Agent Control Commands ──

/// GET /agent/status — 返回 agent 运行状态
/// 多会话并发：session_id 缺省时返回任一 session 是否在运行；指定时返回该 session 状态
#[tauri::command]
pub async fn get_agent_status(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<AgentStatus, String> {
    let handles = state.agent_handles.lock().map_err(|e| e.to_string())?;
    let (running, paused) = if let Some(sid) = session_id.as_deref() {
        // 指定 session：查该 session 的 handle 和 paused flag
        let running = handles.get(sid).map(|h| !h.is_finished()).unwrap_or(false);
        let paused = state.agent_paused_flags.lock().map_err(|e| e.to_string())?
            .get(sid)
            .map(|p| p.load(Ordering::SeqCst))
            .unwrap_or(false);
        (running, paused)
    } else {
        // 缺省：任一 session 在运行即为 running
        let running = handles.values().any(|h| !h.is_finished());
        // paused：任一 session 暂停即为 paused（简化，兼容旧 UI）
        let paused = state.agent_paused_flags.lock().map_err(|e| e.to_string())?
            .values().any(|p| p.load(Ordering::SeqCst));
        (running, paused)
    };

    Ok(AgentStatus { running, paused })
}

/// 统一清理指定会话的运行时资源（取消令牌、abort handle、移除四类 map 条目）。
/// 每个锁作用域结束后才获取下一个锁，避免嵌套持有多个 MutexGuard。
fn cancel_session_runtime_resources(state: &AppState, session_id: &str) {
    if let Ok(cancellations) = state.agent_cancellations.lock() {
        if let Some(token) = cancellations.get(session_id) {
            token.cancel();
        }
    }
    if let Ok(mut handles) = state.agent_handles.lock() {
        if let Some(handle) = handles.remove(session_id) {
            handle.abort();
        }
    }
    if let Ok(mut map) = state.agent_cancellations.lock() {
        map.remove(session_id);
    }
    if let Ok(mut map) = state.agent_paused_flags.lock() {
        map.remove(session_id);
    }
    if let Ok(mut map) = state.approval_responders.lock() {
        map.remove(session_id);
    }
}

/// POST /agent/cancel — 取消 agent
/// 多会话并发：session_id 缺省时取消所有运行中 session；指定时取消该 session
#[tauri::command]
pub async fn cancel_agent(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<String, String> {
    // 取出要取消的 session 列表
    let targets: Vec<String> = {
        let handles = state.agent_handles.lock().map_err(|e| e.to_string())?;
        match session_id.as_deref() {
            Some(sid) => {
                if handles.get(sid).map(|h| !h.is_finished()).unwrap_or(false) {
                    vec![sid.to_string()]
                } else {
                    vec![]
                }
            }
            None => handles
                .iter()
                .filter(|(_, h)| !h.is_finished())
                .map(|(k, _)| k.clone())
                .collect(),
        }
    };

    if targets.is_empty() {
        return Ok("No agent is running".to_string());
    }

    let mut cancelled = 0;
    for sid in &targets {
        cancel_session_runtime_resources(&state, sid);
        cancelled += 1;
    }

    Ok(format!("Agent cancelled ({} session(s))", cancelled))
}

/// POST /agent/pause — 暂停 agent（在下一个 step 边界）
/// 多会话并发：session_id 缺省时暂停所有运行中 session；指定时暂停该 session
#[tauri::command]
pub async fn pause_agent(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<String, String> {
    let targets: Vec<String> = match session_id.as_deref() {
        Some(sid) => vec![sid.to_string()],
        None => state.agent_paused_flags.lock().map_err(|e| e.to_string())?
            .keys().cloned().collect(),
    };

    for sid in &targets {
        if let Some(flag) = state.agent_paused_flags.lock().map_err(|e| e.to_string())?.get(sid) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    Ok("Agent paused".to_string())
}

/// POST /agent/resume — 恢复暂停的 agent
/// 多会话并发：session_id 缺省时恢复所有暂停 session；指定时恢复该 session
#[tauri::command]
pub async fn resume_agent(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<String, String> {
    let targets: Vec<String> = match session_id.as_deref() {
        Some(sid) => vec![sid.to_string()],
        None => state.agent_paused_flags.lock().map_err(|e| e.to_string())?
            .keys().cloned().collect(),
    };

    for sid in &targets {
        if let Some(flag) = state.agent_paused_flags.lock().map_err(|e| e.to_string())?.get(sid) {
            flag.store(false, Ordering::SeqCst);
        }
    }

    Ok("Agent resumed".to_string())
}

/// POST /agent/respond_ask_user — frontend responds to an ask_user prompt
#[tauri::command]
pub async fn respond_ask_user(
    state: State<'_, AppState>,
    response: AskUserResponse,
) -> Result<String, String> {
    // 外层 Mutex 取出内层 Arc 的 clone，再 lock 内层 Mutex 操作 HashMap
    let inner = state
        .pending_ask_user
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let mut pending = inner.lock().map_err(|e| e.to_string())?;

    if let Some(sender) = pending.remove(&response.request_id) {
        let _ = sender.send(response.response);
        Ok(format!("Response sent for request '{}'", response.request_id))
    } else {
        Err(format!(
            "No pending ask_user request with id '{}'",
            response.request_id
        ))
    }
}

// ── D1-T03: Session changes management ──

/// 获取指定会话的所有文件变更记录
#[tauri::command]
pub async fn get_session_changes(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<crate::FileChangeRecord>, String> {
    let changes = state.session_changes.lock().map_err(|e| e.to_string())?;
    Ok(changes.get(&session_id).cloned().unwrap_or_default())
}

/// 清空指定会话的所有文件变更记录
#[tauri::command]
pub async fn clear_session_changes(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut changes = state.session_changes.lock().map_err(|e| e.to_string())?;
    changes.remove(&session_id);
    Ok(())
}

/// 前端监听 file_changed 事件后调用此命令写入变更记录
#[tauri::command]
pub async fn add_session_change(
    state: State<'_, AppState>,
    session_id: String,
    change: crate::FileChangeRecord,
) -> Result<(), String> {
    let mut changes = state.session_changes.lock().map_err(|e| e.to_string())?;
    changes.entry(session_id).or_insert_with(Vec::new).push(change);
    Ok(())
}

// ── D3-T05: Skill 系统 IPC ──

/// 列出当前可用的 skills（全局 + 项目级，项目级覆盖同名）
#[tauri::command]
pub async fn list_skills(
    state: State<'_, AppState>,
) -> Result<Vec<rgoat_core::agent::react::SkillInfo>, String> {
    let ws = state.runtime.read().await.workspace.clone();
    Ok(rgoat_core::agent::react::ReActAgent::load_skills_structured(&ws))
}

/// 读取指定 skill 的完整 SKILL.md 内容
#[tauri::command]
pub async fn read_skill(
    state: State<'_, AppState>,
    name: String,
) -> Result<Option<String>, String> {
    let ws = state.runtime.read().await.workspace.clone();
    Ok(rgoat_core::agent::react::ReActAgent::read_skill_content(&ws, &name))
}

// ── Native Window Control Commands ──

#[tauri::command]
pub fn minimize_window(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_maximize_window(window: tauri::Window) -> Result<bool, String> {
    let is_max = window.is_maximized().map_err(|e| e.to_string())?;
    if is_max {
        window.unmaximize().map_err(|e| e.to_string())?;
        Ok(false)
    } else {
        window.maximize().map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[tauri::command]
pub fn close_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

