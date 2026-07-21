//! Tauri IPC commands — bridge between frontend and rgoat-core

use std::sync::atomic::Ordering;

use rgoat_core::core::config::Settings;
use rgoat_core::provider::impls::{AnthropicProvider, OpenAiCompatibleProvider};
use rgoat_core::provider::provider::{ChatOptions, ProviderConfig, ProviderType, ThinkingLevel};
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

    let sid = session_id.clone();

    // Create session if it doesn't exist, using the frontend-supplied ID as the
    // real session ID (not just the title). This prevents FOREIGN KEY errors when
    // the Agent later writes tool messages to this session.
    let _ = state.conversation
        .get_or_create_session(&session_id, Some("New Session"), Some(&state.workspace))
        .await
        .map_err(|e| e.to_string())?;

    let agent = state.agent.clone();
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
    agent.set_mode(agent_mode);
    let prompt = request.prompt.clone();
    let workspace = state.workspace.clone();
    // 请求级思考强度：仅本次 send_prompt 调用生效，不持久化到 AgentConfig
    let chat_options = ChatOptions {
        thinking_level: parse_thinking_level(&request.thinking_level),
    };

    // Spawn agent in background
    let handle = tokio::spawn(async move {
        // D3-T04: Plan Mode 下使用 PlanRunner 两阶段流程
        let result = if agent_mode == rgoat_core::security::approval::AgentMode::Plan {
            let runner = rgoat_core::agent::plan_runner::PlanRunner::new(
                agent.clone(),
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
            agent.run_with_options(&sid, &prompt, &workspace, &chat_options).await.map(|_| ())
        };
        if let Err(e) = &result {
            tracing::error!("Agent error: {}", e);
        }
    });

    // Store handle for get_agent_status() to check
    {
        let mut guard = state.agent_handle.lock()
            .map_err(|e| e.to_string())?;
        *guard = Some(handle);
    }

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
    }).collect())
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
    })
}

/// Delete a conversation session
#[tauri::command]
pub async fn delete_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    state.conversation
        .delete_session(&session_id)
        .await
        .map_err(|e| e.to_string())
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
    use rgoat_core::core::config::ProviderSettings;
    use rgoat_core::provider::switch::ProviderSwitch;

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
        for name in ["deepseek", "OpenAI", "ANTHROPIC"] {
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
) -> Result<String, String> {
    let mut pending = state.pending_approval.lock().map_err(|e| e.to_string())?;
    if let Some(approval) = pending.take() {
        if approval.tool_name == response.tool_name {
            // D1-T03: 解析 scope 字符串为 ApprovalScope 枚举
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
            let _ = approval.sender.send(decision);
            Ok(format!(
                "Approval {} for tool '{}' (scope={:?})",
                if response.approved { "granted" } else { "denied" },
                response.tool_name, scope
            ))
        } else {
            Err(format!(
                "Tool name mismatch: expected '{}', got '{}'",
                approval.tool_name, response.tool_name
            ))
        }
    } else {
        Err("No pending approval request".to_string())
    }
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

    let ws = std::path::PathBuf::from(&state.workspace);
    let depth = max_depth.unwrap_or(3);
    walk_dir(&ws, &ws, depth).map_err(|e| e.to_string())
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

// ── Agent Control Commands ──

/// GET /agent/status — returns whether the agent is running and/or paused
#[tauri::command]
pub async fn get_agent_status(
    state: State<'_, AppState>,
) -> Result<AgentStatus, String> {
    let paused = state.agent_paused.load(Ordering::SeqCst);
    let running = state
        .agent_handle
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .map(|h| !h.is_finished())
        .unwrap_or(false);

    Ok(AgentStatus { running, paused })
}

/// POST /agent/cancel — cancel the currently running agent
#[tauri::command]
pub async fn cancel_agent(
    state: State<'_, AppState>,
) -> Result<String, String> {
    state.agent_cancellation.cancel();
    Ok("Agent cancellation requested".to_string())
}

/// POST /agent/pause — pause the agent at the next step boundary
#[tauri::command]
pub async fn pause_agent(
    state: State<'_, AppState>,
) -> Result<String, String> {
    state.agent_paused.store(true, Ordering::SeqCst);
    Ok("Agent paused".to_string())
}

/// POST /agent/resume — resume a paused agent
#[tauri::command]
pub async fn resume_agent(
    state: State<'_, AppState>,
) -> Result<String, String> {
    state.agent_paused.store(false, Ordering::SeqCst);
    Ok("Agent resumed".to_string())
}

/// POST /agent/respond_ask_user — frontend responds to an ask_user prompt
#[tauri::command]
pub async fn respond_ask_user(
    state: State<'_, AppState>,
    response: AskUserResponse,
) -> Result<String, String> {
    let mut pending = state
        .pending_ask_user
        .lock()
        .map_err(|e| e.to_string())?;

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
    Ok(rgoat_core::agent::react::ReActAgent::load_skills_structured(&state.workspace))
}

/// 读取指定 skill 的完整 SKILL.md 内容
#[tauri::command]
pub async fn read_skill(
    state: State<'_, AppState>,
    name: String,
) -> Result<Option<String>, String> {
    Ok(rgoat_core::agent::react::ReActAgent::read_skill_content(&state.workspace, &name))
}
