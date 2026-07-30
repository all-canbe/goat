//! RGoat TUI — lightweight AI coding assistant
//!
//! USAGE:
//!   rgoat "<prompt>"          Run a single agent task
//!   rgoat tui                 Launch interactive TUI
//!   rgoat mcp                 Start MCP stdio server
//!   rgoat list                List sessions
//!   rgoat --help              Show help

use std::env;
use std::sync::Arc;

use rgoat_core::agent::react::ReActAgent;
use rgoat_core::agent::types::AgentConfig;
use rgoat_core::cli::{parse_args, CliCommand, help_text};
use rgoat_core::conversation::manager::ConversationManager;
use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::core::config::Settings;
use rgoat_core::core::event_bus::EventBus;
use rgoat_core::core::workspace::resolve_workspace;
use rgoat_core::provider::provider::{LlmProvider, ProviderConfig, ProviderType};
use rgoat_core::provider::impls::{OpenAiCompatibleProvider, AnthropicProvider};
use rgoat_core::provider::switch::ProviderSwitch;
use rgoat_core::security::approval::{AgentMode, ApprovalEngine, ApprovalResponder};
use rgoat_core::tools::registry::ToolRegistry;
use rgoat_core::memory::vector_store::VectorMemory;
use rgoat_core::mcp::server::{GoatMcpServer, run_stdio_server};

mod app;
mod components;
mod keybindings;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let command = parse_args(&args);

    // In TUI mode, tracing output to stderr/stdout corrupts the alternate screen.
    // Route it to a file; otherwise keep the default stderr behaviour.
    let is_tui = matches!(command, CliCommand::Tui);
    if is_tui {
        let log_path = rgoat_core::core::workspace::get_data_dir().join("rgoat.log");
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path);
        if let Ok(file) = file {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "rgoat=info".into()),
                )
                .with_writer(move || -> Box<dyn std::io::Write> {
                    file.try_clone()
                        .map(|f| Box::new(f) as Box<dyn std::io::Write>)
                        .unwrap_or_else(|_| Box::new(std::io::sink()))
                })
                .init();
        } else {
            // Fallback: no logging at all to avoid corrupting TUI
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::new("off"))
                .with_writer(std::io::sink)
                .init();
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "rgoat=info".into()),
            )
            .init();
    }

    // Parse universal flags from raw args
    let auto_approve = args.iter().any(|a| a == "--auto-approve");
    let no_stream = args.iter().any(|a| a == "--no-stream");

    match command {
        CliCommand::Help => {
            println!("{}", help_text());
            return Ok(());
        }
        CliCommand::Version => {
            println!("rgoat {}", rgoat_core::VERSION);
            return Ok(());
        }
        CliCommand::McpServer => {
            run_mcp_server().await?;
            return Ok(());
        }
        _ => {}
    }

    // Load settings; if nothing configured, run setup wizard
    let mut settings = Settings::load_or_empty().unwrap_or_default();
    if !settings.has_configured_provider() {
        println!();
        println!("  ╔══════════════════════════════════════╗");
        println!("  ║     Welcome to RGoat! 🐐            ║");
        println!("  ║  Let's configure your LLM provider. ║");
        println!("  ╚══════════════════════════════════════╝");
        println!();
        match run_setup_wizard(&mut settings) {
            Ok(()) => {
                let _ = settings.save();
                println!("\n  ✓ Configuration saved! Starting RGoat...\n");
            }
            Err(e) => {
                eprintln!("\n  Setup failed: {}", e);
                eprintln!("  You can configure manually in: {}", 
                    rgoat_core::core::workspace::get_data_dir().join("setting.json").display());
                std::process::exit(1);
            }
        }
    }

    // Initialize workspace
    let workspace = resolve_workspace(None);
    let workspace_str = workspace.root().display().to_string();
    let root_path = workspace.root().to_path_buf();

    // Initialize core components
    let event_bus = Arc::new(EventBus::new(256));
    let cancellation = CancellationToken::new();
    let conversation = Arc::new(ConversationManager::new().await?);
    let approval = Arc::new(ApprovalEngine::new());
    let tools = Arc::new(ToolRegistry::new(root_path.clone()));
    let memory = Arc::new(VectorMemory::new_in_memory());
    memory.initialize().await?;

    // Setup providers from settings
    let mut switch = ProviderSwitch::new();
    let default_provider = init_providers(&mut switch, &settings);
    let switch = Arc::new(switch);
    // Auto-select default or first available
    let _ = switch.select(&default_provider).await;
    // Cast to trait object for Agent
    let provider: Arc<dyn LlmProvider> = switch.clone();

    // Determine mode
    let mode = match &command {
        CliCommand::Run { mode, .. } => match mode.as_str() {
            "plan" => AgentMode::Plan,
            "flow" => AgentMode::Flow,
            "yolo" => AgentMode::Yolo,
            "accept-edits" => AgentMode::AcceptEdits,
            _ => AgentMode::Agent,
        },
        _ => AgentMode::Agent,
    };

    match command {
        CliCommand::Run { prompt, .. } if !prompt.is_empty() => {
            let mut config = AgentConfig::default();
            if no_stream {
                config.stream = false;
            }

            // In auto-approve mode, use Yolo mode which bypasses approval checks
            let effective_mode = if auto_approve {
                tracing::info!("Running in auto-approve mode (yolo)");
                AgentMode::Yolo
            } else {
                mode
            };

            let agent = Arc::new(ReActAgent::new(
                config,
                provider,
                tools,
                approval,
                conversation.clone(),
                event_bus,
                cancellation,
                effective_mode,
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            ));

            let session = conversation.create_session(None, Some(&workspace_str)).await?;
            let result = agent.run(&session.id, &prompt, &workspace_str).await;

            match result {
                Ok(r) => println!("{}", r.answer),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        CliCommand::ListSessions => {
            let sessions = conversation.list_sessions().await?;
            for s in sessions {
                println!("[{}] {} ({}) {}", s.id.chars().take(8).collect::<String>(), s.title, s.message_count, s.updated_at.format("%Y-%m-%d %H:%M"));
            }
        }
        CliCommand::ResumeSession { session_id } => {
            let session = conversation.get_session(&session_id).await?;
            match session {
                Some(s) => println!("Session: {} ({})", s.title, s.message_count),
                None => eprintln!("Session not found: {}", session_id),
            }
        }
        CliCommand::Tui | CliCommand::Run { .. } => {
            let config = AgentConfig::default();
            let approval_responder: ApprovalResponder =
                std::sync::Arc::new(tokio::sync::Mutex::new(None));
            let agent = Arc::new(ReActAgent::new(
                config,
                provider,
                tools,
                approval,
                conversation.clone(),
                event_bus.clone(),
                cancellation.clone(),
                mode,
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                approval_responder.clone(),
            ));

            let event_rx = event_bus.subscribe();
            let review_provider = init_review_provider(&settings, &switch);
            app::run_tui(agent, conversation, event_rx, workspace_str, mode, switch, approval_responder, cancellation, review_provider).await?;
        }
        CliCommand::Index { path } => {
            println!("Code indexing not yet implemented for path: {}", path.display());
        }
        _ => {
            println!("{}", help_text());
        }
    }

    Ok(())
}

/// Initialize all providers from settings + env vars.
/// Returns the name of the default provider.
fn init_providers(switch: &mut ProviderSwitch, settings: &Settings) -> String {
    let mut default_name = String::from("deepseek");

    // ── DeepSeek ──
    let ds_key = env::var("DEEPSEEK_API_KEY").unwrap_or_default();
    if !ds_key.is_empty() {
        let cfg = ProviderConfig::deepseek(&ds_key, "deepseek-chat");
        switch.register_blocking(Arc::new(OpenAiCompatibleProvider::new(cfg)));
    }

    // ── OpenAI ──
    let oa_key = env::var("OPENAI_API_KEY").unwrap_or_default();
    if !oa_key.is_empty() {
        let cfg = ProviderConfig::openai(&oa_key, "gpt-4o");
        switch.register_blocking(Arc::new(OpenAiCompatibleProvider::new(cfg)));
    }

    // ── Anthropic ──
    let an_key = env::var("ANTHROPIC_API_KEY").unwrap_or_default();
    if !an_key.is_empty() {
        let cfg = ProviderConfig::anthropic(&an_key, "claude-sonnet-4-20250514");
        switch.register_blocking(Arc::new(AnthropicProvider::new(cfg)));
        default_name = String::from("anthropic");
    }

    // ── Custom providers from settings.json ──
    for ps in &settings.providers {
        let mut key = settings.get_api_key(&ps.name).unwrap_or_default();
        if key.is_empty() {
            // Try env var by convention: uppercase name + _API_KEY
            key = env::var(format!("{}_API_KEY", ps.name.to_uppercase())).unwrap_or_default();
        }
        if key.is_empty() { continue; }

        let base_url = settings.get_base_url(&ps.name)
            .unwrap_or_else(|| String::from("https://api.openai.com/v1"));
        let model = ps.models.as_ref()
            .and_then(|m| m.first())
            .cloned()
            .unwrap_or_else(|| String::from("gpt-4o"));

        // Determine provider type by URL pattern
        let provider_type = if base_url.contains("anthropic") {
            ProviderType::Anthropic
        } else {
            ProviderType::OpenAICompatible
        };

        let cfg = ProviderConfig::new(provider_type, &ps.name, &base_url, &key, &model);
        match provider_type {
            ProviderType::Anthropic => {
                switch.register_blocking(Arc::new(AnthropicProvider::new(cfg)));
            }
            ProviderType::OpenAICompatible => {
                switch.register_blocking(Arc::new(OpenAiCompatibleProvider::new(cfg)));
            }
        }
        default_name = ps.name.clone();
    }

    // ── Fallback: if no env var found, still register a placeholder that will fail gracefully ──
    if switch.list_names_blocking().is_empty() {
        let cfg = ProviderConfig::deepseek("", "deepseek-chat");
        switch.register_blocking(Arc::new(OpenAiCompatibleProvider::new(cfg)));
    }

    default_name
}

/// 初始化审查 Provider（若配置了独立的审查模型）
/// 若未配置则返回 None，调用方 fallback 到主 provider
fn init_review_provider(
    settings: &Settings,
    _switch: &ProviderSwitch,
) -> Option<Arc<dyn LlmProvider>> {
    let review_model = settings.review_model.as_ref()?;
    if review_model.is_empty() {
        return None;
    }

    // 获取 review base_url 和 api_key
    let base_url = settings
        .review_base_url
        .as_deref()
        .unwrap_or("");
    let api_key = settings
        .review_api_key
        .as_deref()
        .unwrap_or("");

    // 若未提供 base_url/api_key，尝试从主 providers 列表中查找
    let (final_base_url, final_api_key) = if base_url.is_empty() || api_key.is_empty() {
        // 尝试从主 provider 列表中获取
        if let Some(ps) = settings.providers.first() {
            (
                ps.base_url.as_deref().unwrap_or(base_url),
                ps.api_key.as_deref().unwrap_or(api_key),
            )
        } else {
            (base_url, api_key)
        }
    } else {
        (base_url, api_key)
    };

    if final_base_url.is_empty() {
        return None;
    }

    let provider_type = Settings::detect_provider_type(final_base_url);
    let cfg = ProviderConfig::new(
        provider_type,
        "review",
        final_base_url,
        final_api_key,
        review_model,
    );

    match provider_type {
        ProviderType::Anthropic => Some(Arc::new(AnthropicProvider::new(cfg))),
        ProviderType::OpenAICompatible => Some(Arc::new(OpenAiCompatibleProvider::new(cfg))),
    }
}

async fn run_mcp_server() -> anyhow::Result<()> {
    let workspace = resolve_workspace(None);
    let tools = Arc::new(ToolRegistry::new(workspace.root().to_path_buf()));
    let server = Arc::new(GoatMcpServer::new(tools));
    run_stdio_server(server).await?;
    Ok(())
}

/// Interactive setup wizard for first-time users.
/// Guides through: Base URL → auto-detect type → API Key → Model name.
fn run_setup_wizard(settings: &mut Settings) -> Result<(), String> {
    use std::io::{self, IsTerminal, Write};

    // Skip wizard in non-interactive environments (piped stdin/stdout)
    if !io::stdin().is_terminal() {
        return Err("Cannot run setup wizard in non-interactive mode (stdin is not a TTY). Please configure manually.".to_string());
    }

    let stdin = io::stdin();
    let mut input = String::new();

    loop {
        print!("  1. Enter Base URL (e.g. https://api.deepseek.com/v1): ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        input.clear();
        stdin.read_line(&mut input).map_err(|e| e.to_string())?;
        let url = input.trim().to_string();
        if url.is_empty() {
            println!("     URL cannot be empty.");
            continue;
        }
        let ptype = Settings::detect_provider_type(&url);
        let type_label = match ptype {
            ProviderType::OpenAICompatible => "OpenAI Compatible",
            ProviderType::Anthropic => "Anthropic",
        };
        println!("     Detected: {} API format", type_label);

        print!("  2. Enter API Key: ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        input.clear();
        stdin.read_line(&mut input).map_err(|e| e.to_string())?;
        let api_key = input.trim().to_string();
        if api_key.is_empty() {
            println!("     API key cannot be empty.");
            continue;
        }

        let default_model = match ptype {
            ProviderType::Anthropic => "claude-sonnet-4-20250514",
            ProviderType::OpenAICompatible => "deepseek-chat",
        };
        print!("  3. Enter Model name [{}]: ", default_model);
        io::stdout().flush().map_err(|e| e.to_string())?;
        input.clear();
        stdin.read_line(&mut input).map_err(|e| e.to_string())?;
        let model = input.trim();
        let model = if model.is_empty() { default_model.to_string() } else { model.to_string() };

        let default_name = match ptype {
            ProviderType::Anthropic => "anthropic",
            ProviderType::OpenAICompatible => {
                if url.contains("deepseek") { "deepseek" }
                else if url.contains("openai") { "openai" }
                else if url.contains("bigmodel") { "zhipu" }
                else { "custom" }
            }
        };
        print!("  4. Enter Provider name [{}]: ", default_name);
        io::stdout().flush().map_err(|e| e.to_string())?;
        input.clear();
        stdin.read_line(&mut input).map_err(|e| e.to_string())?;
        let name = input.trim();
        let name = if name.is_empty() { default_name } else { name };

        let name = name.to_string();
        let url = url.to_string();

        settings.add_or_update_provider(&name, &url, &api_key, &model);
        settings.provider = name.clone();
        settings.model = model.to_string();
        settings.save().map_err(|e| e.to_string())?;

        println!();
        println!("  Configuration complete!");
        println!("    Provider: {}", name);
        println!("    URL:      {}", url);
        println!("    Model:    {}", model);
        println!("    Type:     {}", type_label);
        return Ok(());
    }
}
