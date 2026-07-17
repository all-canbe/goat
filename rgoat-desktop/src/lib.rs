//! RGoat Desktop — Tauri-based AI coding assistant
//!
//! Architecture:
//!   WebView frontend (index.html) ← invoke → Rust commands → rgoat-core Agent
//!                                               ↕
//!                                          EventBus → frontend events

mod commands;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use tauri::Manager;
use tauri::Emitter;
use tauri::WebviewUrl;
use tauri::WebviewWindowBuilder;

use rgoat_core::agent::react::ReActAgent;
use rgoat_core::agent::types::AgentConfig;
use rgoat_core::conversation::manager::ConversationManager;
use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::core::config::Settings;
use rgoat_core::core::event_bus::EventBus;
use rgoat_core::core::workspace::resolve_workspace;
use rgoat_core::memory::vector_store::VectorMemory;
use rgoat_core::provider::switch::ProviderSwitch;
use rgoat_core::security::approval::{AgentMode, ApprovalDecision, ApprovalEngine, ApprovalResponder};
use rgoat_core::tools::registry::ToolRegistry;

/// Application state shared across all Tauri commands
pub struct AppState {
    pub agent: Arc<ReActAgent>,
    pub conversation: Arc<ConversationManager>,
    pub event_bus: Arc<EventBus>,
    pub switch: Arc<ProviderSwitch>,
    pub workspace: String,
    /// Frontend can respond to approval requests by calling respond_approval
    pub pending_approval: Arc<Mutex<Option<PendingApproval>>>,
    /// Frontend can respond to ask_user requests by calling respond_ask_user
    pub pending_ask_user: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<String>>>>,
    /// Cancellation token for stopping the running agent
    pub agent_cancellation: CancellationToken,
    /// Whether the agent is paused at a step boundary
    pub agent_paused: Arc<AtomicBool>,
    /// Handle to the currently running agent task
    pub agent_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// D1-T03: 会话内文件变更记录 — session_id → changes
    pub session_changes: Arc<Mutex<HashMap<String, Vec<FileChangeRecord>>>>,
}

/// Represents a pending approval request awaiting frontend response
pub struct PendingApproval {
    pub tool_name: String,
    pub args: serde_json::Value,
    /// D1-T03: 改为携带完整 ApprovalDecision（含 scope）而非 bool
    pub sender: tokio::sync::oneshot::Sender<ApprovalDecision>,
}

/// D1-T03: 单个文件变更记录（前端 invoke add_session_change 时序列化）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileChangeRecord {
    pub file_path: String,
    pub change_type: String, // "create" | "edit" | "delete"
    pub diff: String,
    pub tool_name: String,
    pub timestamp: String,
    pub additions: usize,
    pub deletions: usize,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rgoat=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // ── Initialize all components synchronously via block_on ──
            let rt = tokio::runtime::Runtime::new().unwrap();
            let (state, event_bus) = rt.block_on(async {
                init_app_state().await.unwrap()
            });

            // ── Bridge EventBus → Tauri events ──
            let handle = app.handle().clone();
            let mut rx = event_bus.subscribe();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    loop {
                        match rx.recv().await {
                            Ok(event) => {
                                if let serde_json::Value::Object(mut map) = event.data.clone() {
                                    map.insert("source".to_string(), serde_json::Value::String(event.source.clone()));
                                    let _ = handle.emit("agent-event", serde_json::Value::Object(map));
                                } else {
                                    let fallback = serde_json::json!({
                                        "type": "unknown",
                                        "source": event.source,
                                    });
                                    let _ = handle.emit("agent-event", fallback);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
            });

            app.manage(state);

            // ── Create the main window (we manage it ourselves, not from config) ──
            let _window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::App("index.html".into()),
            )
            .title("RGoat — AI Coding Assistant")
            .inner_size(900.0, 700.0)
            .min_inner_size(600.0, 400.0)
            .center()
            .resizable(true)
            .build()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::send_prompt,
            commands::get_sessions,
            commands::delete_session,
            commands::rename_session,
            commands::list_providers,
            commands::switch_provider,
            commands::get_current_provider,
            commands::has_configured_provider,
            commands::configure_provider,
            commands::respond_approval,
            commands::list_workspace_files,
            commands::cancel_agent,
            commands::pause_agent,
            commands::resume_agent,
            commands::get_agent_status,
            commands::respond_ask_user,
            // D1-T03: 会话变更管理
            commands::get_session_changes,
            commands::clear_session_changes,
            commands::add_session_change,
            // D3-T05: Skill 系统
            commands::list_skills,
            commands::read_skill,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn init_app_state() -> Result<(AppState, Arc<EventBus>), Box<dyn std::error::Error>> {
    let settings = Settings::load().unwrap_or_default();
    let workspace = resolve_workspace(None);
    let workspace_str = workspace.root().display().to_string();
    let root_path = workspace.root().to_path_buf();

    let event_bus = Arc::new(EventBus::new(256));
    let conversation = Arc::new(ConversationManager::new().await?);
    let approval = Arc::new(ApprovalEngine::new());
    let agent_cancellation = CancellationToken::new();

    // Create tool registry with EventBus so AskUserTool is registered
    let tools = Arc::new(ToolRegistry::with_event_bus(root_path, Some(event_bus.clone())));
    let pending_ask_user = tools
        .pending_ask_user
        .clone()
        .unwrap_or_else(|| Arc::new(Mutex::new(HashMap::new())));

    let memory = Arc::new(VectorMemory::new_in_memory());
    memory.initialize().await?;

    // Setup providers
    let mut switch = ProviderSwitch::new();
    let default_provider = init_providers(&mut switch, &settings);
    let switch = Arc::new(switch);
    if !default_provider.is_empty() {
        let _ = switch.select(&default_provider).await;
    }

    let provider: Arc<dyn rgoat_core::provider::provider::LlmProvider> = switch.clone();

    let config = AgentConfig::default();

    let agent_paused = Arc::new(AtomicBool::new(false));
    let approval_responder: ApprovalResponder =
        std::sync::Arc::new(tokio::sync::Mutex::new(None));
    let agent = Arc::new(ReActAgent::new(
        config,
        provider,
        tools,
        approval,
        conversation.clone(),
        event_bus.clone(),
        agent_cancellation.clone(),
        AgentMode::Agent,
        agent_paused.clone(),
        approval_responder,
    ));

    Ok((
        AppState {
            agent,
            conversation,
            event_bus: event_bus.clone(),
            switch,
            workspace: workspace_str,
            pending_approval: Arc::new(Mutex::new(None)),
            pending_ask_user,
            agent_cancellation,
            agent_paused,
            agent_handle: Arc::new(Mutex::new(None)),
            session_changes: Arc::new(Mutex::new(HashMap::new())),
        },
        event_bus,
    ))
}

fn init_providers(switch: &mut ProviderSwitch, settings: &Settings) -> String {
    let mut default_name = String::new();

    // ── DeepSeek ──
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        if !key.is_empty() {
            let cfg = rgoat_core::provider::provider::ProviderConfig::deepseek(&key, "deepseek-chat");
            switch.register_blocking(Arc::new(rgoat_core::provider::impls::OpenAiCompatibleProvider::new(cfg)));
            default_name = String::from("deepseek");
        }
    }

    // ── OpenAI ──
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        if !key.is_empty() {
            let cfg = rgoat_core::provider::provider::ProviderConfig::openai(&key, "gpt-4o");
            switch.register_blocking(Arc::new(rgoat_core::provider::impls::OpenAiCompatibleProvider::new(cfg)));
            if default_name.is_empty() { default_name = String::from("openai"); }
        }
    }

    // ── Anthropic ──
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            let cfg = rgoat_core::provider::provider::ProviderConfig::anthropic(&key, "claude-sonnet-4-20250514");
            switch.register_blocking(Arc::new(rgoat_core::provider::impls::AnthropicProvider::new(cfg)));
            if default_name.is_empty() { default_name = String::from("anthropic"); }
        }
    }

    // ── Custom providers from settings.json ──
    for ps in &settings.providers {
        let mut key = settings.get_api_key(&ps.name).unwrap_or_default();
        if key.is_empty() {
            key = std::env::var(format!("{}_API_KEY", ps.name.to_uppercase())).unwrap_or_default();
        }
        if key.is_empty() { continue; }

        let base_url = settings.get_base_url(&ps.name)
            .unwrap_or_else(|| String::from("https://api.openai.com/v1"));
        let model = ps.models.as_ref()
            .and_then(|m| m.first()).cloned()
            .unwrap_or_else(|| String::from("gpt-4o"));

        let provider_type = if base_url.contains("anthropic") {
            rgoat_core::provider::provider::ProviderType::Anthropic
        } else {
            rgoat_core::provider::provider::ProviderType::OpenAICompatible
        };

        let cfg = rgoat_core::provider::provider::ProviderConfig::new(provider_type, &ps.name, &base_url, &key, &model);
        match provider_type {
            rgoat_core::provider::provider::ProviderType::Anthropic => {
                switch.register_blocking(Arc::new(rgoat_core::provider::impls::AnthropicProvider::new(cfg)));
            }
            rgoat_core::provider::provider::ProviderType::OpenAICompatible => {
                switch.register_blocking(Arc::new(rgoat_core::provider::impls::OpenAiCompatibleProvider::new(cfg)));
            }
        }
        if default_name.is_empty() { default_name = ps.name.clone(); }
    }

    // Fallback
    if switch.list_names_blocking().is_empty() {
        let cfg = rgoat_core::provider::provider::ProviderConfig::deepseek("", "deepseek-chat");
        switch.register_blocking(Arc::new(rgoat_core::provider::impls::OpenAiCompatibleProvider::new(cfg)));
    }

    default_name
}
