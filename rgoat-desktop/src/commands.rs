//! Tauri IPC commands — bridge between frontend and rgoat-core

use std::sync::atomic::Ordering;

use tauri::State;
use serde::{Deserialize, Serialize};

use crate::AppState;

// ── Request / Response types ──

#[derive(Debug, Deserialize)]
pub struct SendPromptRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    #[allow(dead_code)]
    pub mode: Option<String>,
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
}

#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub name: String,
    pub model: String,
    pub provider_type: String,
    pub is_current: bool,
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

/// Send a prompt to the Agent (runs asynchronously, emits events to frontend)
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
    let prompt = request.prompt.clone();
    let workspace = state.workspace.clone();

    // Spawn agent in background
    let handle = tokio::spawn(async move {
        let result = agent.run(&sid, &prompt, &workspace).await;
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
    }).collect())
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
    Ok(details.into_iter().map(|d| ProviderInfo {
        name: d.name,
        model: d.model,
        provider_type: d.provider_type,
        is_current: d.is_current,
    }).collect())
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
    use rgoat_core::core::config::Settings;
    use rgoat_core::provider::provider::{ProviderConfig, ProviderType};
    use rgoat_core::provider::impls::{OpenAiCompatibleProvider, AnthropicProvider};
    use std::sync::Arc;

    let mut settings = Settings::load_or_empty().map_err(|e| e.to_string())?;

    settings.add_or_update_provider(&name, &base_url, &api_key, &model);
    settings.provider = name.clone();
    settings.model = model.clone();
    settings.save().map_err(|e| e.to_string())?;

    // Register the new provider in the running switch and select it immediately
    let provider_type = Settings::detect_provider_type(&base_url);
    let cfg = ProviderConfig::new(provider_type, &name, &base_url, &api_key, &model);
    let provider: Arc<dyn rgoat_core::provider::provider::LlmProvider> = match provider_type {
        ProviderType::Anthropic => Arc::new(AnthropicProvider::new(cfg)),
        ProviderType::OpenAICompatible => Arc::new(OpenAiCompatibleProvider::new(cfg)),
    };

    state.switch.register(provider).await;
    state.switch.select(&name).await.map_err(|e| e.to_string())?;

    Ok(format!(
        "Provider '{}' saved and activated. Model: {}.",
        name, model
    ))
}

// ── Approval ──

#[derive(Debug, Deserialize)]
pub struct ApprovalResponse {
    pub tool_name: String,
    pub approved: bool,
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
            let _ = approval.sender.send(response.approved);
            Ok(format!(
                "Approval {} for tool '{}'",
                if response.approved { "granted" } else { "denied" },
                response.tool_name
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
