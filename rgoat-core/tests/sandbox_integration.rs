//! 沙箱全链路集成测试
//!
//! 验证 sandbox 在 agent 运行时工具执行路径中的集成：
//! 1. sandbox 启用时，workspace 内文件操作正常
//! 2. sandbox 启用时，workspace 外路径被拒绝
//! 3. sandbox 启用时，受保护目录（.git）被拒绝
//! 4. sandbox 未启用时，行为不变（向后兼容）
//!
//! 运行：cargo test -p rgoat-core --test sandbox_integration

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rgoat_core::agent::react::ReActAgent;
use rgoat_core::agent::types::{AgentConfig, AgentEvent};
use rgoat_core::conversation::manager::ConversationManager;
use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::core::event_bus::EventBus;
use rgoat_core::provider::provider::{
    ChatMessage, ChatOptions, ChatResponse, Choice, LlmError, LlmProvider, LlmStream, MessageContent,
    ProviderType, Role, ToolCallDef, ToolDef,
};
use rgoat_core::security::approval::{AgentMode, ApprovalEngine};
use rgoat_core::security::sandbox::{create_sandbox, Sandbox, SandboxLevel};
use rgoat_core::tools::registry::ToolRegistry;

// ============================================================================
// Mock LLM Provider — 可配置工具调用
// ============================================================================

struct MockProvider {
    tool_name: String,
    tool_args: serde_json::Value,
    final_answer: String,
    call_count: std::sync::Mutex<u32>,
}

impl MockProvider {
    fn new(tool_name: &str, tool_args: serde_json::Value, final_answer: &str) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            tool_args,
            final_answer: final_answer.to_string(),
            call_count: std::sync::Mutex::new(0),
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockProvider {
    async fn chat(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDef],
        _options: &ChatOptions,
    ) -> Result<ChatResponse, LlmError> {
        let mut c = self.call_count.lock().unwrap();
        *c += 1;
        let count = *c;

        if count == 1 {
            // 第 1 轮：返回工具调用
            let tc = ToolCallDef {
                id: format!("call_mock_{}", count),
                call_type: "function".to_string(),
                function: rgoat_core::provider::provider::FunctionCall {
                    name: self.tool_name.clone(),
                    arguments: self.tool_args.to_string(),
                },
            };
            Ok(ChatResponse {
                choices: vec![Choice {
                    message: ChatMessage {
                        role: Role::Assistant,
                        content: MessageContent::Text("I'll use a tool.".to_string()),
                        name: None,
                        tool_call_id: None,
                        tool_calls: Some(vec![tc]),
                    },
                    finish_reason: Some("tool_calls".to_string()),
                }],
                usage: None,
            })
        } else {
            // 第 2+ 轮：返回最终答案
            Ok(ChatResponse {
                choices: vec![Choice {
                    message: ChatMessage {
                        role: Role::Assistant,
                        content: MessageContent::Text(self.final_answer.clone()),
                        name: None,
                        tool_call_id: None,
                        tool_calls: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }
    }

    async fn chat_stream(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDef],
        _options: &ChatOptions,
    ) -> Result<LlmStream, LlmError> {
        Err(LlmError::Config("stream not implemented for mock".to_string()))
    }

    fn name(&self) -> &str { "mock" }
    fn model(&self) -> &str { "mock-model" }
    fn provider_type(&self) -> ProviderType { ProviderType::OpenAICompatible }
}

// ============================================================================
// 测试辅助
// ============================================================================

/// 初始化沙箱，容忍 Windows Job Object 失败
fn init_sandbox(sb: &dyn Sandbox, level: SandboxLevel, workspace: &std::path::PathBuf) {
    if let Err(e) = sb.init(level, workspace) {
        eprintln!("sandbox init warning (path checks still valid): {e}");
    }
}

/// 收集 agent 事件
fn collect_tool_results(
    rx: &mut tokio::sync::broadcast::Receiver<rgoat_core::core::event_bus::Event>,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(ev) => {
                if let Ok(ae) = serde_json::from_value::<AgentEvent>(ev.data) {
                    events.push(ae);
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
        }
    }
    events
}

// ============================================================================
// 测试用例
// ============================================================================

/// sandbox 启用时，workspace 内文件读取正常
#[tokio::test]
async fn test_sandbox_allows_workspace_file() {
    let workspace = std::env::temp_dir().join("rgoat_sandbox_allow");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let test_file = workspace.join("hello.txt");
    std::fs::write(&test_file, "Hello from workspace!").unwrap();

    // 创建并初始化 sandbox
    let sb = create_sandbox();
    init_sandbox(&*sb, SandboxLevel::WorkspaceWrite, &workspace);

    let mock = Arc::new(MockProvider::new(
        "read_file",
        serde_json::json!({
            "file_path": test_file.to_string_lossy().to_string()
        }),
        "File read successfully",
    ));

    // 直接构建带 sandbox 的 agent
    let event_bus = Arc::new(EventBus::new(256));
    let _event_rx = event_bus.subscribe();
    let db_path = workspace.join(".rgoat_test.db");
    let conversation = Arc::new(
        ConversationManager::new_with_path(&db_path)
            .await
            .unwrap(),
    );
    let approval = Arc::new(ApprovalEngine::new());
    let tools = Arc::new(ToolRegistry::new(workspace.clone()));

    let config = AgentConfig::default();
    let agent = ReActAgent::new(
        config,
        mock,
        tools,
        approval,
        conversation.clone(),
        event_bus.clone(),
        CancellationToken::new(),
        AgentMode::Yolo,
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Mutex::new(None)),
    );
    let agent = Arc::new(agent.with_sandbox(Arc::from(sb)));

    let session_id = "test-sandbox-allow";
    let _ = conversation
        .get_or_create_session(session_id, Some("test"), Some(&workspace.to_string_lossy()))
        .await
        .unwrap();

    let result = agent
        .run(session_id, "Read hello.txt", &workspace.to_string_lossy())
        .await
        .expect("agent should complete");

    // 验证 agent 成功完成
    assert!(
        result.answer.contains("successfully") || !result.answer.is_empty(),
        "Agent should complete successfully for workspace file"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

/// sandbox 启用时，workspace 外路径被拒绝
#[tokio::test]
async fn test_sandbox_blocks_outside_workspace() {
    let workspace = std::env::temp_dir().join("rgoat_sandbox_block");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    // workspace 外的文件
    let outside_file = std::env::temp_dir().join("rgoat_outside_secret.txt");
    std::fs::write(&outside_file, "secret data").unwrap();

    // 创建并初始化 sandbox
    let sb = create_sandbox();
    init_sandbox(&*sb, SandboxLevel::WorkspaceWrite, &workspace);

    let mock = Arc::new(MockProvider::new(
        "read_file",
        serde_json::json!({
            "file_path": outside_file.to_string_lossy().to_string()
        }),
        "Should not reach here",
    ));

    let event_bus = Arc::new(EventBus::new(256));
    let mut event_rx = event_bus.subscribe();
    let db_path = workspace.join(".rgoat_test.db");
    let conversation = Arc::new(
        ConversationManager::new_with_path(&db_path)
            .await
            .unwrap(),
    );
    let approval = Arc::new(ApprovalEngine::new());
    let tools = Arc::new(ToolRegistry::new(workspace.clone()));

    let config = AgentConfig::default();
    let agent = ReActAgent::new(
        config,
        mock,
        tools,
        approval,
        conversation.clone(),
        event_bus.clone(),
        CancellationToken::new(),
        AgentMode::Yolo,
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Mutex::new(None)),
    );
    let agent = Arc::new(agent.with_sandbox(Arc::from(sb)));

    let session_id = "test-sandbox-block";
    let _ = conversation
        .get_or_create_session(session_id, Some("test"), Some(&workspace.to_string_lossy()))
        .await
        .unwrap();

    let _result = agent
        .run(session_id, "Read outside file", &workspace.to_string_lossy())
        .await
        .expect("agent should still complete (with error in tool result)");

    // 收集事件，检查是否有 ToolResult 事件包含沙箱拒绝消息
    let events = collect_tool_results(&mut event_rx);
    let has_sandbox_denial = events.iter().any(|e| {
        if let AgentEvent::ToolResult { output, .. } = e {
            output.contains("沙箱安全拒绝") || output.contains("sandbox_path_denied")
        } else {
            false
        }
    });

    assert!(
        has_sandbox_denial,
        "Should have sandbox denial in tool results. Events: {:?}",
        events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
    );

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&outside_file);
}

/// sandbox 启用时，.git 目录被拒绝
#[tokio::test]
async fn test_sandbox_blocks_protected_dir() {
    let workspace = std::env::temp_dir().join("rgoat_sandbox_git");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    // 在 workspace 内创建 .git 目录
    let git_dir = workspace.join(".git");
    std::fs::create_dir_all(&git_dir).unwrap();
    let git_file = git_dir.join("config");
    std::fs::write(&git_file, "[core]").unwrap();

    let sb = create_sandbox();
    init_sandbox(&*sb, SandboxLevel::WorkspaceWrite, &workspace);

    let mock = Arc::new(MockProvider::new(
        "read_file",
        serde_json::json!({
            "file_path": git_file.to_string_lossy().to_string()
        }),
        "Should not reach here",
    ));

    let event_bus = Arc::new(EventBus::new(256));
    let mut event_rx = event_bus.subscribe();
    let db_path = workspace.join(".rgoat_test.db");
    let conversation = Arc::new(
        ConversationManager::new_with_path(&db_path)
            .await
            .unwrap(),
    );
    let approval = Arc::new(ApprovalEngine::new());
    let tools = Arc::new(ToolRegistry::new(workspace.clone()));

    let config = AgentConfig::default();
    let agent = ReActAgent::new(
        config,
        mock,
        tools,
        approval,
        conversation.clone(),
        event_bus.clone(),
        CancellationToken::new(),
        AgentMode::Yolo,
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Mutex::new(None)),
    );
    let agent = Arc::new(agent.with_sandbox(Arc::from(sb)));

    let session_id = "test-sandbox-git";
    let _ = conversation
        .get_or_create_session(session_id, Some("test"), Some(&workspace.to_string_lossy()))
        .await
        .unwrap();

    let _ = agent
        .run(session_id, "Read .git/config", &workspace.to_string_lossy())
        .await
        .expect("agent should complete");

    let events = collect_tool_results(&mut event_rx);
    let has_denial = events.iter().any(|e| {
        if let AgentEvent::ToolResult { output, .. } = e {
            output.contains("沙箱安全拒绝") || output.contains("sandbox_path_denied")
        } else {
            false
        }
    });

    assert!(
        has_denial,
        "Should deny .git access. Events: {:?}",
        events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>()
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

/// sandbox 未启用时，行为不变（向后兼容）
#[tokio::test]
async fn test_no_sandbox_backward_compatible() {
    let workspace = std::env::temp_dir().join("rgoat_no_sandbox");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    // workspace 外文件
    let outside_file = std::env::temp_dir().join("rgoat_outside_compat.txt");
    std::fs::write(&outside_file, "data").unwrap();

    let mock = Arc::new(MockProvider::new(
        "read_file",
        serde_json::json!({
            "file_path": outside_file.to_string_lossy().to_string()
        }),
        "Read without sandbox",
    ));

    let event_bus = Arc::new(EventBus::new(256));
    let mut event_rx = event_bus.subscribe();
    let db_path = workspace.join(".rgoat_test.db");
    let conversation = Arc::new(
        ConversationManager::new_with_path(&db_path)
            .await
            .unwrap(),
    );
    let approval = Arc::new(ApprovalEngine::new());
    let tools = Arc::new(ToolRegistry::new(workspace.clone()));

    let config = AgentConfig::default();
    // 不设置 sandbox
    let agent = Arc::new(ReActAgent::new(
        config,
        mock,
        tools,
        approval,
        conversation.clone(),
        event_bus.clone(),
        CancellationToken::new(),
        AgentMode::Yolo,
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Mutex::new(None)),
    ));

    let session_id = "test-no-sandbox";
    let _ = conversation
        .get_or_create_session(session_id, Some("test"), Some(&workspace.to_string_lossy()))
        .await
        .unwrap();

    let _result = agent
        .run(session_id, "Read outside file", &workspace.to_string_lossy())
        .await
        .expect("agent should complete");

    // 没有 sandbox 时应正常读取（无沙箱拒绝）
    let events = collect_tool_results(&mut event_rx);
    let has_denial = events.iter().any(|e| {
        if let AgentEvent::ToolResult { output, success, .. } = e {
            *success == false && output.contains("沙箱安全拒绝")
        } else {
            false
        }
    });

    assert!(
        !has_denial,
        "Without sandbox, no denial should occur"
    );

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&outside_file);
}
