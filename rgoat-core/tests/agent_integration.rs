//! 集成测试：核心 Agent 对话-执行-工具链路
//!
//! 测试覆盖：
//! 1. Mock LLM Provider → Agent 接收响应并解析工具调用
//! 2. Agent 执行内置工具（read_file）→ 获得结果
//! 3. Agent 将 observation 写回 DB（带 tool_call_id）
//! 4. EventBus 正确发出所有事件（Started→Thought→ToolCall→ToolResult→Finished）
//! 5. Agent 返回正确的最终答案
//!
//! 运行：cargo test -p rgoat-core --test agent_integration

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rgoat_core::agent::react::ReActAgent;
use rgoat_core::agent::types::{AgentConfig, AgentEvent};
use rgoat_core::conversation::manager::ConversationManager;
use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::core::event_bus::EventBus;
use rgoat_core::provider::provider::{
    ChatMessage, ChatOptions, ChatResponse, Choice, FunctionCall, LlmError, LlmProvider, LlmStream,
    MessageContent, ProviderType, Role, ToolCallDef, ToolDef,
};
use rgoat_core::security::approval::{AgentMode, ApprovalEngine, ApprovalDecision};
use rgoat_core::tools::registry::{ExecutionMode, Tool, ToolExecutionContext, ToolRegistry, ToolResult};
use rgoat_core::security::approval::ToolCategory;

// ============================================================================
// Mock LLM Provider — 两轮对话
// ============================================================================

/// 模拟两轮对话：
///   第 1 轮：返回 `read_file` 工具调用（读取测试文件）
///   第 2 轮：返回最终答案
struct MockTwoRoundProvider {
    /// 工具返回后 LLM 给出的最终答案
    final_answer: String,
    /// 第 1 轮要返回的工具调用
    tool_name: String,
    tool_args: serde_json::Value,
    /// 调用计数器
    call_count: std::sync::Mutex<u32>,
}

impl MockTwoRoundProvider {
    fn new(
        final_answer: &str,
        tool_name: &str,
        tool_args: serde_json::Value,
    ) -> Self {
        Self {
            final_answer: final_answer.to_string(),
            tool_name: tool_name.to_string(),
            tool_args,
            call_count: std::sync::Mutex::new(0),
        }
    }

    /// 计数并返回当前是第几轮
    fn next_count(&self) -> u32 {
        let mut c = self.call_count.lock().unwrap();
        *c += 1;
        *c
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockTwoRoundProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDef],
        _options: &ChatOptions,
    ) -> Result<ChatResponse, LlmError> {
        let count = self.next_count();

        if count == 1 {
            // 第 1 轮：返回工具调用
            let tc = ToolCallDef {
                id: "call_mock_001".to_string(),
                call_type: "function".to_string(),
                function: rgoat_core::provider::provider::FunctionCall {
                    name: self.tool_name.clone(),
                    arguments: self.tool_args.to_string(),
                },
            };

            // Thought 事件的 content 应该来自 assistant 消息
            let thought_text = format!("I'll use {} to read the file.", self.tool_name);

            Ok(ChatResponse {
                choices: vec![Choice {
                    message: ChatMessage {
                        role: Role::Assistant,
                        content: MessageContent::Text(thought_text),
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
            let has_tool = messages.iter().any(|m| m.role == Role::Tool);
            assert!(has_tool, "Second LLM call should include tool results");

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

    fn name(&self) -> &str {
        "mock_two_round"
    }

    fn model(&self) -> &str {
        "mock-model"
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAICompatible
    }
}

// ============================================================================
// 测试辅助函数
// ============================================================================

/// 从预先订阅的 broadcast Receiver 中收集所有待处理事件（非阻塞 drain）
async fn drain_events(
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

/// 创建 Agent 测试辅助函数，返回预订阅的 event receiver
async fn build_test_agent(
    workspace: std::path::PathBuf,
    db_dir: std::path::PathBuf,
    mock_provider: Arc<dyn LlmProvider>,
) -> (
    Arc<ReActAgent>,
    Arc<ConversationManager>,
    tokio::sync::broadcast::Receiver<rgoat_core::core::event_bus::Event>,
) {
    let event_bus = Arc::new(EventBus::new(256));
    let event_rx = event_bus.subscribe(); // 预先订阅，确保不漏事件
    let conversation = Arc::new(
        ConversationManager::new_with_path(&db_dir.join("test_conversations.db"))
            .await
            .expect("Failed to create ConversationManager"),
    );
    let approval = Arc::new(ApprovalEngine::new());
    let tools = Arc::new(ToolRegistry::new(workspace));

    let config = AgentConfig {
        max_steps: 5,
        model: "mock-model".to_string(),
        temperature: 0.0,
        max_tokens: 1024,
        stream: false,
        max_subagents: 1,
        max_subagent_depth: 1,
        max_flow_rounds: 3,
        auto_verify: false, // tests don't need real verification
        context_window: 128_000,
        max_steps_extend_limit: 2,
        no_progress_step_limit: 15,
        max_events: 2000,
        max_history_messages: 200,
        planning_enabled: false,
        task_persistence_enabled: false, // D1-T08: 测试不写 tasks.jsonl
        watchdog_timeout_secs: 300,
        checkpoint_interval_steps: 10,
    };

    let agent = Arc::new(ReActAgent::new(
        config,
        mock_provider,
        tools,
        approval,
        conversation.clone(),
        event_bus.clone(),
        CancellationToken::new(),
        AgentMode::Agent,
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Mutex::new(None::<tokio::sync::oneshot::Sender<ApprovalDecision>>)),
    ));

    (agent, conversation, event_rx)
}

// ============================================================================
// 测试用例
// ============================================================================

/// 核心测试：完整的 Agent ReAct 循环，包含工具调用和 DB 写入
#[tokio::test]
async fn test_full_react_loop_with_tool_call() {
    // ── 准备：创建临时工作空间 + 测试文件 ──
    let workspace = std::env::temp_dir().join("rgoat_test_full_react");
    let _ = std::fs::remove_dir_all(&workspace); // 清理旧数据
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let test_file = workspace.join("hello.txt");
    let file_content = "Hello, RGoat!\nThis is a test file.\nLine 3.";
    std::fs::write(&test_file, file_content).expect("write test file");

    // ── Mock Provider 配置 ──
    let mock = Arc::new(MockTwoRoundProvider::new(
        "The file contains: Hello, RGoat!",
        "read_file",
        serde_json::json!({
            "file_path": test_file.to_string_lossy().to_string()
        }),
    ));

    let (agent, conv, mut event_rx) =
        build_test_agent(workspace.clone(), workspace.clone(), mock).await;

    let session_id = "test-session-full-react";
    let _ = conv
        .get_or_create_session(session_id, Some("Integration Test"), Some(&workspace.to_string_lossy()))
        .await
        .expect("create session");

    // ── 执行 ──
    let result = agent
        .run(session_id, "What's in hello.txt?", &workspace.to_string_lossy())
        .await
        .expect("Agent should complete successfully");

    // ── 断言 1：最终答案 ──
    assert!(
        result.answer.contains("Hello, RGoat"),
        "Final answer should reference file content. Got: {}",
        result.answer
    );
    // 第 0 步：LLM 返回 read_file 工具调用 → 执行工具
    // 第 1 步：LLM 收到 observation 后返回最终答案 → Finished
    assert_eq!(result.steps_taken, 2, "Should take 2 steps (1 tool call + 1 final)");
    assert_eq!(result.tool_calls, 1, "Should have exactly 1 tool call");

    // ── 断言 2：EventBus 事件 ──
    let events = drain_events(&mut event_rx).await;
    println!("Collected {} events", events.len());

    let started_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Started { .. }))
        .count();
    assert_eq!(started_count, 1, "Should have exactly 1 Started event");

    let thought_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Thought { .. }))
        .count();
    assert!(thought_count >= 1, "Should have at least 1 Thought event");

    let has_tool_call = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCall { tool_name, .. } if tool_name == "read_file"));
    assert!(has_tool_call, "Should have a ToolCall event for read_file");

    let has_tool_result = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolResult { success: true, .. }));
    assert!(has_tool_result, "Should have a successful ToolResult event");

    let has_finished = events
        .iter()
        .any(|e| matches!(e, AgentEvent::Finished { .. }));
    assert!(has_finished, "Should have a Finished event");

    // ── 断言 3：DB 消息写入 ──
    let messages = conv
        .get_messages(session_id)
        .await
        .expect("get messages");
    println!("DB has {} messages", messages.len());

    // 至少应有：user(1) + assistant_with_tool(1) + tool_result(1) + assistant_final(1) = 4
    assert!(messages.len() >= 3, "DB should have at least 3 messages");

    let user_msgs: Vec<_> = messages.iter().filter(|m| m.role == "user").collect();
    assert_eq!(user_msgs.len(), 1, "1 user message");

    let assistant_msgs: Vec<_> = messages.iter().filter(|m| m.role == "assistant").collect();
    assert!(assistant_msgs.len() >= 1, "at least 1 assistant message");

    let tool_msgs: Vec<_> = messages.iter().filter(|m| m.role == "tool").collect();
    assert_eq!(tool_msgs.len(), 1, "1 tool observation message");

    // ── 断言 4：tool 消息应该有 tool_call_id ──
    let tool_msg = tool_msgs.first().unwrap();
    assert!(
        tool_msg.tool_call_id.is_some(),
        "Tool message MUST have tool_call_id set"
    );
    assert_eq!(tool_msg.tool_call_id.as_deref(), Some("call_mock_001"));
    assert!(
        tool_msg.content.contains("Hello, RGoat"),
        "Tool result should contain file content"
    );

    // ── 断言 5：final assistant 消息应包含最终答案 ──
    let final_assistant = assistant_msgs.last().unwrap();
    assert!(
        final_assistant.content.contains("Hello, RGoat"),
        "Final assistant message should contain answer"
    );

    // ── 清理 ──
    let _ = std::fs::remove_dir_all(&workspace);
}

// ============================================================================
// P3-2: terminate 机制验证
// ============================================================================

/// P3-2 单元测试：ToolResult 的 terminate 字段和 builder
#[tokio::test]
async fn test_terminate_tool_result_builder() {
    // 默认 terminate=false
    let success = ToolResult::success("ok");
    assert!(!success.terminate, "默认 success 结果 terminate 应为 false");

    let error = ToolResult::error("fail", "err");
    assert!(!error.terminate, "默认 error 结果 terminate 应为 false");

    // with_terminate 标记后为 true
    let terminated = ToolResult::success("terminated").with_terminate();
    assert!(terminated.terminate, "with_terminate 后应为 true");
    assert!(terminated.success, "仍应保持 success=true");
    assert_eq!(terminated.output, "terminated");

    let terminated_err = ToolResult::error("fail", "err").with_terminate();
    assert!(terminated_err.terminate, "error 结果也可标记 terminate");
    assert!(!terminated_err.success, "仍应保持 success=false");
}

/// P3-2 集成测试：工具返回 terminate=true 时，agent 应结束循环
#[tokio::test]
async fn test_terminate_stops_agent_loop() {
    use std::sync::Mutex;

    // 一个自定义工具，无论参数如何都返回 terminate=true 的成功结果
    struct TerminateTool;
    #[async_trait::async_trait]
    impl Tool for TerminateTool {
        fn name(&self) -> &str { "terminate_tool" }
        fn description(&self) -> &str { "A tool that signals termination" }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string"}
                },
                "required": ["message"]
            })
        }
        fn category(&self) -> ToolCategory { ToolCategory::Read }
        fn execution_mode(&self) -> ExecutionMode { ExecutionMode::Parallel }

        async fn execute(&self, args: serde_json::Value) -> ToolResult {
            let msg = args.get("message").and_then(|v| v.as_str()).unwrap_or("done");
            ToolResult::success(format!("TerminateTool executed: {}", msg)).with_terminate()
        }
    }

    // Mock LLM：第 1 轮调用 terminate_tool，第 2 轮不会到达
    struct TerminateProvider {
        call_count: Mutex<u32>,
    }
    #[async_trait::async_trait]
    impl LlmProvider for TerminateProvider {
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<ChatResponse, LlmError> {
            let mut c = self.call_count.lock().unwrap();
            *c += 1;
            if *c == 1 {
                let tc = ToolCallDef {
                    id: "call_terminate_001".to_string(),
                    call_type: "function".to_string(),
                    function: rgoat_core::provider::provider::FunctionCall {
                        name: "terminate_tool".to_string(),
                        arguments: r#"{"message":"finish"}"#.to_string(),
                    },
                };
                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: ChatMessage {
                            role: Role::Assistant,
                            content: MessageContent::Text("Calling terminate tool".to_string()),
                            name: None,
                            tool_call_id: None,
                            tool_calls: Some(vec![tc]),
                        },
                        finish_reason: Some("tool_calls".to_string()),
                    }],
                    usage: None,
                })
            } else {
                // 如果 agent 循环继续到这里，测试应该失败
                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: ChatMessage {
                            role: Role::Assistant,
                            content: MessageContent::Text("SHOULD NOT REACH".to_string()),
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
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<LlmStream, LlmError> {
            Err(LlmError::Config("n/a".to_string()))
        }
        fn name(&self) -> &str { "terminate_provider" }
        fn model(&self) -> &str { "mock" }
        fn provider_type(&self) -> ProviderType { ProviderType::OpenAICompatible }
    }

    let workspace = std::env::temp_dir().join("rgoat_test_terminate");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).expect("create workspace");

    // 创建包含自定义工具的 registry
    let mut tools = ToolRegistry::empty();
    tools.register(Arc::new(TerminateTool));

    let event_bus = Arc::new(EventBus::new(256));
    let conversation = Arc::new(
        ConversationManager::new_with_path(&workspace.join("test_terminate.db"))
            .await
            .expect("create db"),
    );
    let approval = Arc::new(ApprovalEngine::new());
    let config = AgentConfig {
        max_steps: 5,
        model: "mock".to_string(),
        temperature: 0.0,
        max_tokens: 1024,
        stream: false,
        max_subagents: 1,
        max_subagent_depth: 1,
        max_flow_rounds: 3,
        auto_verify: false,
        context_window: 128_000,
        max_steps_extend_limit: 2,
        no_progress_step_limit: 15,
        max_events: 2000,
        max_history_messages: 200,
        planning_enabled: false,
        task_persistence_enabled: false,
        watchdog_timeout_secs: 300,
        checkpoint_interval_steps: 10,
    };

    let agent = Arc::new(ReActAgent::new(
        config,
        Arc::new(TerminateProvider { call_count: Mutex::new(0) }),
        Arc::new(tools),
        approval,
        conversation.clone(),
        event_bus.clone(),
        CancellationToken::new(),
        AgentMode::Agent,
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Mutex::new(None::<tokio::sync::oneshot::Sender<ApprovalDecision>>)),
    ));

    let session_id = "test-terminate";
    conversation
        .get_or_create_session(session_id, Some("Terminate Test"), None)
        .await
        .expect("create session");

    let result = agent
        .run(session_id, "Terminate now", &workspace.to_string_lossy())
        .await
        .expect("Agent should complete");

    // 在第 1 轮 terminate 后直接结束，不进入第 2 轮
    assert_eq!(result.steps_taken, 1, "terminate 后应单步结束");
    assert_eq!(result.tool_calls, 1, "应执行 1 次工具调用");

    let _ = std::fs::remove_dir_all(&workspace);
}

/// 测试 2：Agent 在 max_steps 内完成（无工具调用场景）
#[tokio::test]
async fn test_simple_answer_no_tool_calls() {
    let workspace = std::env::temp_dir().join("rgoat_test_simple");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).expect("create workspace");

    // Mock 在第一轮就返回最终答案（无工具调用）
    struct SingleAnswerProvider;
    #[async_trait::async_trait]
    impl LlmProvider for SingleAnswerProvider {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDef],
            _options: &ChatOptions,
        ) -> Result<ChatResponse, LlmError> {
            Ok(ChatResponse {
                choices: vec![Choice {
                    message: ChatMessage {
                        role: Role::Assistant,
                        content: MessageContent::Text("42 is the answer.".to_string()),
                        name: None,
                        tool_call_id: None,
                        tool_calls: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }

        async fn chat_stream(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDef],
            _options: &ChatOptions,
        ) -> Result<LlmStream, LlmError> {
            Err(LlmError::Config("not implemented".to_string()))
        }

        fn name(&self) -> &str { "single_answer" }
        fn model(&self) -> &str { "mock" }
        fn provider_type(&self) -> ProviderType { ProviderType::OpenAICompatible }
    }

    let mock = Arc::new(SingleAnswerProvider);
    let (agent, conv, mut event_rx) =
        build_test_agent(workspace.clone(), workspace.clone(), mock).await;

    let session_id = "test-simple-answer";
    let _ = conv
        .get_or_create_session(session_id, Some("Simple Test"), None)
        .await
        .expect("create session");

    let result = agent
        .run(session_id, "What is the answer?", &workspace.to_string_lossy())
        .await
        .expect("Agent should complete");

    assert_eq!(result.answer, "42 is the answer.");
    assert_eq!(result.tool_calls, 0, "No tool calls expected");
    assert_eq!(result.steps_taken, 1);

    let events = drain_events(&mut event_rx).await;
    println!("Simple answer events: {:?}", events.iter().map(|e| match e { AgentEvent::Started { .. } => "started", AgentEvent::Thought { .. } => "thought", AgentEvent::ToolCall { .. } => "tool_call", AgentEvent::ToolResult { .. } => "tool_result", AgentEvent::Approval { .. } => "approval", AgentEvent::Message { .. } => "message", AgentEvent::StepCompleted { .. } => "step_completed", AgentEvent::Finished { .. } => "finished", AgentEvent::Error { .. } => "error", _ => "other" }).collect::<Vec<_>>());
    let has_finished = events.iter().any(|e| matches!(e, AgentEvent::Finished { .. }));
    assert!(has_finished, "Should have a Finished event");

    let messages = conv.get_messages(session_id).await.expect("get messages");
    assert_eq!(messages.len(), 2, "user + assistant");
    assert!(messages.iter().any(|m| m.role == "user"));
    assert!(messages.iter().any(|m| m.role == "assistant"));

    let _ = std::fs::remove_dir_all(&workspace);
}

/// 测试 3：验证 EventBus 事件类型的 serde tag 格式正确
#[tokio::test]
async fn test_event_serde_tags_are_correct() {
    // Thought 事件 → tag "thought"
    let ev = AgentEvent::Thought {
        step: 1,
        content: "test".to_string(),
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"].as_str(), Some("thought"));
    assert_eq!(json["step"].as_u64(), Some(1));
    assert_eq!(json["content"].as_str(), Some("test"));

    // ToolCall 事件 → tag "tool_call"
    let ev = AgentEvent::ToolCall {
        step: 1,
        tool_name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "/foo"}),
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"].as_str(), Some("tool_call"));
    assert_eq!(json["tool_name"].as_str(), Some("read_file"));

    // Finished 事件 → tag "finished"
    let ev = AgentEvent::Finished {
        answer: "done".to_string(),
        steps: 3,
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"].as_str(), Some("finished"));
    assert_eq!(json["answer"].as_str(), Some("done"));
}

/// 测试 4：验证 add_observation 写入的 tool_call_id 正确
#[tokio::test]
async fn test_tool_call_id_persists_in_db() {
    let workspace = std::env::temp_dir().join("rgoat_test_tcid");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let test_file = workspace.join("data.txt");
    std::fs::write(&test_file, "test content").expect("write test file");

    let mock = Arc::new(MockTwoRoundProvider::new(
        "Found 'test content'",
        "read_file",
        serde_json::json!({"file_path": test_file.to_string_lossy().to_string()}),
    ));

    let (agent, conv, _event_rx) = build_test_agent(workspace.clone(), workspace.clone(), mock).await;

    let session_id = "test-tool-call-id";
    conv.get_or_create_session(session_id, Some("TCID Test"), None)
        .await
        .expect("create session");

    let result = agent
        .run(session_id, "read data.txt", &workspace.to_string_lossy())
        .await
        .expect("Agent should complete");

    assert!(result.tool_calls > 0);

    let messages = conv.get_messages(session_id).await.expect("get messages");

    // 找到 tool 消息并验证 tool_call_id
    let tool_msg = messages
        .iter()
        .find(|m| m.role == "tool")
        .expect("Should have a tool message");
    assert_eq!(
        tool_msg.tool_call_id,
        Some("call_mock_001".to_string()),
        "tool_call_id should match the mock's tool call ID"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// ============================================================================
// Day 1-2 防卡死机制验收（M1-M4）— Mock LLM Provider 模拟 3 种场景
// ============================================================================

/// M1 验收：清晰的最终答案应被识别为完成，单步结束。
#[tokio::test]
async fn test_m1_clear_final_answer_finishes() {
    struct FinalProvider;
    #[async_trait::async_trait]
    impl LlmProvider for FinalProvider {
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<ChatResponse, LlmError> {
            Ok(ChatResponse {
                choices: vec![Choice {
                    message: ChatMessage {
                        role: Role::Assistant,
                        content: MessageContent::Text("任务已完成，所有改动已保存。".to_string()),
                        name: None,
                        tool_call_id: None,
                        tool_calls: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<LlmStream, LlmError> {
            Err(LlmError::Config("n/a".to_string()))
        }
        fn name(&self) -> &str { "final" }
        fn model(&self) -> &str { "mock" }
        fn provider_type(&self) -> ProviderType { ProviderType::OpenAICompatible }
    }

    let workspace = std::env::temp_dir().join("rgoat_test_m1");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let (agent, conv, _rx) = build_test_agent(workspace.clone(), workspace.clone(), Arc::new(FinalProvider)).await;
    let sid = "m1-session";
    conv.get_or_create_session(sid, Some("M1"), None).await.unwrap();

    let result = agent.run(sid, "完成任务", &workspace.to_string_lossy()).await.expect("ok");
    assert_eq!(result.steps_taken, 1, "清晰最终答案应单步结束");
    assert!(result.answer.contains("任务已完成"), "答案应含最终文本");

    let _ = std::fs::remove_dir_all(&workspace);
}

/// M2 验收：模型只输出文字（含延续词）不调工具 → 应被催促，而非卡死或提前退出。
/// 连续 3 次催促后第 4 次退出循环（以累积文本作为结果）。
#[tokio::test]
async fn test_m2_continuation_nudge_when_stuck() {
    struct StuckProvider;
    #[async_trait::async_trait]
    impl LlmProvider for StuckProvider {
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<ChatResponse, LlmError> {
            Ok(ChatResponse {
                choices: vec![Choice {
                    message: ChatMessage {
                        role: Role::Assistant,
                        content: MessageContent::Text("让我先看看文件内容".to_string()),
                        name: None,
                        tool_call_id: None,
                        tool_calls: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<LlmStream, LlmError> {
            Err(LlmError::Config("n/a".to_string()))
        }
        fn name(&self) -> &str { "stuck" }
        fn model(&self) -> &str { "mock" }
        fn provider_type(&self) -> ProviderType { ProviderType::OpenAICompatible }
    }

    let workspace = std::env::temp_dir().join("rgoat_test_m2");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let (agent, conv, _rx) = build_test_agent(workspace.clone(), workspace.clone(), Arc::new(StuckProvider)).await;
    let sid = "m2-session";
    conv.get_or_create_session(sid, Some("M2"), None).await.unwrap();

    // max_steps 默认 5；若 M2 未生效会无限循环（测试挂死），此处应能正常返回。
    let result = agent.run(sid, "做点事", &workspace.to_string_lossy()).await.expect("ok");
    // 第 1-3 次被催促 continue，第 4 次退出 → 共 4 步
    assert_eq!(result.steps_taken, 4, "M2 应在第 4 次退出循环");
    assert_eq!(result.answer, "让我先看看文件内容", "退出时以累积文本作为结果");

    let _ = std::fs::remove_dir_all(&workspace);
}

/// M3 验收：重复调用同一工具 → 第 3 次应熔断（注入提示、跳过执行、继续循环），
/// 后续换策略给出最终答案后正常结束。
#[tokio::test]
async fn test_m3_dedup_breaker() {
    struct RepeatToolProvider {
        call_count: std::sync::Mutex<u32>,
        tool_name: String,
        tool_args: serde_json::Value,
        final_answer: String,
    }
    #[async_trait::async_trait]
    impl LlmProvider for RepeatToolProvider {
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<ChatResponse, LlmError> {
            let count = { let mut c = self.call_count.lock().unwrap(); *c += 1; *c };
            if count <= 3 {
                let tc = ToolCallDef {
                    id: format!("call_repeat_{}", count),
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
                            content: MessageContent::Text(format!("调用 {} #{}", self.tool_name, count)),
                            name: None,
                            tool_call_id: None,
                            tool_calls: Some(vec![tc]),
                        },
                        finish_reason: Some("tool_calls".to_string()),
                    }],
                    usage: None,
                })
            } else {
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
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<LlmStream, LlmError> {
            Err(LlmError::Config("n/a".to_string()))
        }
        fn name(&self) -> &str { "repeat" }
        fn model(&self) -> &str { "mock" }
        fn provider_type(&self) -> ProviderType { ProviderType::OpenAICompatible }
    }

    let workspace = std::env::temp_dir().join("rgoat_test_m3");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();
    let test_file = workspace.join("data.txt");
    std::fs::write(&test_file, "payload").unwrap();

    let mock = Arc::new(RepeatToolProvider {
        call_count: std::sync::Mutex::new(0),
        tool_name: "read_file".to_string(),
        tool_args: serde_json::json!({ "file_path": test_file.to_string_lossy().to_string() }),
        final_answer: "已读取完毕，任务完成。".to_string(),
    });
    let (agent, conv, mut event_rx) = build_test_agent(workspace.clone(), workspace.clone(), mock).await;
    let sid = "m3-session";
    conv.get_or_create_session(sid, Some("M3"), None).await.unwrap();

    let result = agent.run(sid, "读取文件", &workspace.to_string_lossy()).await.expect("ok");
    assert!(result.answer.contains("任务完成"), "熔断后换策略应给出最终答案");

    let events = drain_events(&mut event_rx).await;
    let has_dedup = events.iter().any(|e| matches!(
        e,
        AgentEvent::ToolResult { success: false, output, .. } if output.contains("检测到重复调用")
    ));
    assert!(has_dedup, "M3 熔断应注入 '检测到重复调用' 提示而非执行工具");

    let _ = std::fs::remove_dir_all(&workspace);
}

/// M4 验收：模型输出被 max_tokens 截断（finish_reason=length）→ 应自动恢复，
/// 注入提示后下一轮给出最终答案。
#[tokio::test]
async fn test_m4_truncation_recovery() {
    struct TruncationProvider {
        call_count: std::sync::Mutex<u32>,
    }
    #[async_trait::async_trait]
    impl LlmProvider for TruncationProvider {
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<ChatResponse, LlmError> {
            let count = { let mut c = self.call_count.lock().unwrap(); *c += 1; *c };
            if count == 1 {
                // 第 1 轮：被截断，无 tool_calls，finish_reason=length
                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: ChatMessage {
                            role: Role::Assistant,
                            content: MessageContent::Text("我来读取文件".to_string()),
                            name: None,
                            tool_call_id: None,
                            tool_calls: None,
                        },
                        finish_reason: Some("length".to_string()),
                    }],
                    usage: None,
                })
            } else {
                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: ChatMessage {
                            role: Role::Assistant,
                            content: MessageContent::Text("文件读取完成，任务结束。".to_string()),
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
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef], _o: &ChatOptions) -> Result<LlmStream, LlmError> {
            Err(LlmError::Config("n/a".to_string()))
        }
        fn name(&self) -> &str { "trunc" }
        fn model(&self) -> &str { "mock" }
        fn provider_type(&self) -> ProviderType { ProviderType::OpenAICompatible }
    }

    let workspace = std::env::temp_dir().join("rgoat_test_m4");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let (agent, conv, _rx) = build_test_agent(workspace.clone(), workspace.clone(), Arc::new(TruncationProvider { call_count: std::sync::Mutex::new(0) })).await;
    let sid = "m4-session";
    conv.get_or_create_session(sid, Some("M4"), None).await.unwrap();

    let result = agent.run(sid, "读取文件", &workspace.to_string_lossy()).await.expect("ok");
    assert_eq!(result.steps_taken, 2, "截断后应自动恢复并完成");
    assert!(result.answer.contains("任务结束"), "恢复后应给出最终答案");

    let _ = std::fs::remove_dir_all(&workspace);
}

// ============================================================================
// Task 6: 截断 / 并行 / terminate 批量调度验收
// ============================================================================

/// 通用批量 Mock Provider：
/// - 第 1 轮返回给定 `tool_calls` + `finish_reason`
/// - 第 2+ 轮返回最终答案（可选断言收到的 tool 消息数）
///
/// 通过 `Arc<AtomicU32>` 暴露调用计数，供 terminate 测试断言。
struct BatchMockProvider {
    call_count: Arc<AtomicU32>,
    round1_tool_calls: Vec<ToolCallDef>,
    round1_finish_reason: String,
    final_answer: String,
    /// 第 2 轮断言：收到的 tool 角色消息数（0 = 不检查）
    expect_tool_messages: usize,
}

impl BatchMockProvider {
    fn new(
        call_count: Arc<AtomicU32>,
        round1_tool_calls: Vec<ToolCallDef>,
        round1_finish_reason: &str,
        final_answer: &str,
        expect_tool_messages: usize,
    ) -> Self {
        Self {
            call_count,
            round1_tool_calls,
            round1_finish_reason: round1_finish_reason.to_string(),
            final_answer: final_answer.to_string(),
            expect_tool_messages,
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for BatchMockProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDef],
        _options: &ChatOptions,
    ) -> Result<ChatResponse, LlmError> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
        if count == 1 {
            Ok(ChatResponse {
                choices: vec![Choice {
                    message: ChatMessage {
                        role: Role::Assistant,
                        content: MessageContent::Text("调用工具批次".to_string()),
                        name: None,
                        tool_call_id: None,
                        tool_calls: Some(self.round1_tool_calls.clone()),
                    },
                    finish_reason: Some(self.round1_finish_reason.clone()),
                }],
                usage: None,
            })
        } else {
            if self.expect_tool_messages > 0 {
                let tool_n = messages.iter().filter(|m| m.role == Role::Tool).count();
                assert_eq!(
                    tool_n, self.expect_tool_messages,
                    "第 2 轮应收到 {} 条 tool 消息，实际 {}",
                    self.expect_tool_messages, tool_n
                );
            }
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
        _m: &[ChatMessage],
        _t: &[ToolDef],
        _o: &ChatOptions,
    ) -> Result<LlmStream, LlmError> {
        Err(LlmError::Config("n/a".to_string()))
    }
    fn name(&self) -> &str {
        "batch_mock"
    }
    fn model(&self) -> &str {
        "mock"
    }
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAICompatible
    }
}

/// 构造 Agent 测试辅助函数（接受自定义 `ToolRegistry`），返回预订阅的 event receiver
async fn build_test_agent_with_registry(
    db_dir: std::path::PathBuf,
    mock_provider: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
) -> (
    Arc<ReActAgent>,
    Arc<ConversationManager>,
    tokio::sync::broadcast::Receiver<rgoat_core::core::event_bus::Event>,
) {
    let event_bus = Arc::new(EventBus::new(256));
    let event_rx = event_bus.subscribe(); // 预先订阅，确保不漏事件
    let conversation = Arc::new(
        ConversationManager::new_with_path(&db_dir.join("test_conversations.db"))
            .await
            .expect("Failed to create ConversationManager"),
    );
    let approval = Arc::new(ApprovalEngine::new());
    let tools = Arc::new(tools);

    let config = AgentConfig {
        max_steps: 5,
        model: "mock-model".to_string(),
        temperature: 0.0,
        max_tokens: 1024,
        stream: false,
        max_subagents: 1,
        max_subagent_depth: 1,
        max_flow_rounds: 3,
        auto_verify: false,
        context_window: 128_000,
        max_steps_extend_limit: 2,
        no_progress_step_limit: 15,
        max_events: 2000,
        max_history_messages: 200,
        planning_enabled: false,
        task_persistence_enabled: false,
        watchdog_timeout_secs: 300,
        checkpoint_interval_steps: 10,
    };

    let agent = Arc::new(ReActAgent::new(
        config,
        mock_provider,
        tools,
        approval,
        conversation.clone(),
        event_bus.clone(),
        CancellationToken::new(),
        AgentMode::Agent,
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Mutex::new(None::<tokio::sync::oneshot::Sender<ApprovalDecision>>)),
    ));

    (agent, conversation, event_rx)
}

/// 构造一个 `ToolCallDef`（减少测试样板）
fn tc(id: &str, name: &str, args: serde_json::Value) -> ToolCallDef {
    ToolCallDef {
        id: id.to_string(),
        call_type: "function".to_string(),
        function: FunctionCall {
            name: name.to_string(),
            arguments: args.to_string(),
        },
    }
}

// ── Step 1：截断 tool_call 防护 ──────────────────────────────────────────────

/// Task 6 验收：finish_reason=length 且含 tool_calls 时，每个 tool_call 都应回填
/// 截断错误 observation，且零真实工具执行。
#[tokio::test]
async fn truncated_tool_calls_rejected() {
    // 计数工具：每次执行递增计数器（截断时应零执行）
    struct CountingTool {
        counter: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl Tool for CountingTool {
        fn name(&self) -> &str {
            "counting_tool"
        }
        fn description(&self) -> &str {
            "increments a counter when executed"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type":"object","properties":{}})
        }
        fn category(&self) -> ToolCategory {
            ToolCategory::Read
        }
        fn execution_mode(&self) -> ExecutionMode {
            ExecutionMode::Parallel
        }
        async fn execute(&self, _args: serde_json::Value) -> ToolResult {
            self.counter.fetch_add(1, Ordering::SeqCst);
            ToolResult::success("counted")
        }
    }

    let workspace = std::env::temp_dir().join("rgoat_test_t6_trunc");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::empty();
    registry.register(Arc::new(CountingTool {
        counter: counter.clone(),
    }));

    let call_count = Arc::new(AtomicU32::new(0));
    let provider = Arc::new(BatchMockProvider::new(
        call_count,
        vec![
            tc("call_trunc_1", "counting_tool", serde_json::json!({})),
            tc("call_trunc_2", "counting_tool", serde_json::json!({})),
        ],
        "length",
        "工具执行完毕，任务完成。",
        2,
    ));

    let (agent, conv, _rx) = build_test_agent_with_registry(
        workspace.clone(),
        provider,
        registry,
    )
    .await;

    let sid = "t6-trunc";
    conv.get_or_create_session(sid, Some("T6 Truncation"), None)
        .await
        .unwrap();

    let result = agent
        .run(sid, "调用工具", &workspace.to_string_lossy())
        .await
        .expect("agent should complete");

    // 核心断言 1：零真实工具执行
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "截断的 tool_calls 不应执行任何工具"
    );

    // 核心断言 2：2 条 error tool observation，且消息含"截断"
    let messages = conv.get_messages(sid).await.expect("get messages");
    let tool_msgs: Vec<_> = messages.iter().filter(|m| m.role == "tool").collect();
    assert_eq!(tool_msgs.len(), 2, "应有 2 条 tool observation（截断错误）");
    assert!(
        tool_msgs.iter().all(|m| m.content.contains("截断")),
        "每条截断消息应含'截断'，实际: {:?}",
        tool_msgs.iter().map(|m| &m.content).collect::<Vec<_>>()
    );

    // 断言 3：tool_call_id 正确对应原始调用顺序
    let ids: Vec<_> = tool_msgs.iter().map(|m| m.tool_call_id.clone()).collect();
    assert_eq!(
        ids,
        vec![
            Some("call_trunc_1".to_string()),
            Some("call_trunc_2".to_string())
        ]
    );

    // 断言 4：agent 恢复并给出最终答案
    assert!(
        result.answer.contains("任务完成"),
        "恢复后应给最终答案，实际: {}",
        result.answer
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// ── Step 2：并行 / 串行调度 ──────────────────────────────────────────────────

/// 延迟工具：执行时记录开始顺序（tool_call_id）与并发度，sleep 后返回。
/// 通过 `mode` 区分 Parallel / Sequential，用于验证批量调度行为。
///
/// `max_concurrent` 记录执行期间观察到的最大并发数：并行 batch 应为 2，
/// 串行 batch 应为 1。相比墙钟阈值，该断言确定性更强、不受测试套件开销抖动影响。
struct DelayTool {
    tool_name: String,
    mode: ExecutionMode,
    delay: Duration,
    order_log: Arc<std::sync::Mutex<Vec<String>>>,
    active: Arc<AtomicUsize>,
    max_concurrent: Arc<AtomicUsize>,
}

impl DelayTool {
    fn record_start(&self) {
        let cur = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        // CAS 更新 max_concurrent（无锁原子）
        let mut seen = self.max_concurrent.load(Ordering::SeqCst);
        while cur > seen {
            match self.max_concurrent.compare_exchange(
                seen,
                cur,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => seen = actual,
            }
        }
    }

    fn record_end(&self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl Tool for DelayTool {
    fn name(&self) -> &str {
        &self.tool_name
    }
    fn description(&self) -> &str {
        "sleeps for a delay and records start order / concurrency"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"label":{"type":"string"}}})
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Read
    }
    fn execution_mode(&self) -> ExecutionMode {
        self.mode
    }

    async fn execute(&self, _args: serde_json::Value) -> ToolResult {
        // agent 实际调用 execute_ctx；此处仅为满足 trait
        self.record_start();
        tokio::time::sleep(self.delay).await;
        self.record_end();
        ToolResult::success("delayed")
    }

    async fn execute_ctx(
        &self,
        _args: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> ToolResult {
        let id = ctx.tool_call_id.clone();
        self.record_start();
        self.order_log.lock().unwrap().push(id.clone());
        tokio::time::sleep(self.delay).await;
        self.record_end();
        ToolResult::success(format!("delayed:{}", id))
    }
}

/// Task 6 验收：纯 Parallel batch 应并行执行（max_concurrent == 2），
/// 且 conversation 中 tool observation 的 tool_call_id 顺序与 provider 原始顺序一致。
#[tokio::test]
async fn parallel_batch_runs_concurrently() {
    let workspace = std::env::temp_dir().join("rgoat_test_t6_parallel");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let order_log = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::empty();
    registry.register(Arc::new(DelayTool {
        tool_name: "delay_parallel".to_string(),
        mode: ExecutionMode::Parallel,
        delay: Duration::from_millis(100),
        order_log: order_log.clone(),
        active: active.clone(),
        max_concurrent: max_concurrent.clone(),
    }));

    let call_count = Arc::new(AtomicU32::new(0));
    let provider = Arc::new(BatchMockProvider::new(
        call_count,
        vec![
            tc("call_p1", "delay_parallel", serde_json::json!({"label":"a"})),
            tc("call_p2", "delay_parallel", serde_json::json!({"label":"b"})),
        ],
        "tool_calls",
        "并行执行完毕，任务完成。",
        2,
    ));

    let (agent, conv, _rx) = build_test_agent_with_registry(
        workspace.clone(),
        provider,
        registry,
    )
    .await;

    let sid = "t6-parallel";
    conv.get_or_create_session(sid, Some("T6 Parallel"), None)
        .await
        .unwrap();

    let start = std::time::Instant::now();
    let result = agent
        .run(sid, "并行调用", &workspace.to_string_lossy())
        .await
        .expect("agent should complete");
    let elapsed = start.elapsed();

    // 断言 1：两个工具并发执行（max_concurrent == 2 证明 join_all 并行调度）
    assert_eq!(
        max_concurrent.load(Ordering::SeqCst),
        2,
        "纯 Parallel batch 应并行执行（max_concurrent=2），实际 {}，耗时 {:?}",
        max_concurrent.load(Ordering::SeqCst),
        elapsed
    );

    // 断言 2：两个工具都已执行
    let log = order_log.lock().unwrap().clone();
    assert_eq!(log.len(), 2, "两个工具都应执行");

    // 断言 3：conversation 中 tool observation 的 tool_call_id 顺序与 provider 原始顺序一致
    let messages = conv.get_messages(sid).await.expect("get messages");
    let tool_ids: Vec<_> = messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(|m| m.tool_call_id.clone())
        .collect();
    assert_eq!(
        tool_ids,
        vec![Some("call_p1".to_string()), Some("call_p2".to_string())],
        "tool observation 顺序应与 provider 原始顺序一致"
    );

    assert!(result.answer.contains("任务完成"));
    let _ = std::fs::remove_dir_all(&workspace);
}

/// Task 6 验收：含一个 Sequential 工具的 batch 应串行执行（max_concurrent == 1），
/// 开始顺序与 provider 原始 tool_calls 顺序一致。
#[tokio::test]
async fn sequential_then_parallel_runs_in_order() {
    let workspace = std::env::temp_dir().join("rgoat_test_t6_sequential");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let order_log = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let mk = |tool_name: &str, mode: ExecutionMode| DelayTool {
        tool_name: tool_name.to_string(),
        mode,
        delay: Duration::from_millis(100),
        order_log: order_log.clone(),
        active: active.clone(),
        max_concurrent: max_concurrent.clone(),
    };
    let mut registry = ToolRegistry::empty();
    registry.register(Arc::new(mk("delay_seq", ExecutionMode::Sequential)));
    registry.register(Arc::new(mk("delay_par", ExecutionMode::Parallel)));

    let call_count = Arc::new(AtomicU32::new(0));
    let provider = Arc::new(BatchMockProvider::new(
        call_count,
        vec![
            tc("call_s1", "delay_seq", serde_json::json!({"label":"first"})),
            tc("call_p1", "delay_par", serde_json::json!({"label":"second"})),
        ],
        "tool_calls",
        "串行执行完毕，任务完成。",
        2,
    ));

    let (agent, conv, _rx) = build_test_agent_with_registry(
        workspace.clone(),
        provider,
        registry,
    )
    .await;

    let sid = "t6-sequential";
    conv.get_or_create_session(sid, Some("T6 Sequential"), None)
        .await
        .unwrap();

    let start = std::time::Instant::now();
    let result = agent
        .run(sid, "串行调用", &workspace.to_string_lossy())
        .await
        .expect("agent should complete");
    let elapsed = start.elapsed();

    // 断言 1：含 Sequential 工具 → 整批串行 → max_concurrent == 1（任意时刻仅 1 个工具在执行）
    assert_eq!(
        max_concurrent.load(Ordering::SeqCst),
        1,
        "含 Sequential 的 batch 应串行执行（max_concurrent=1），实际 {}，耗时 {:?}",
        max_concurrent.load(Ordering::SeqCst),
        elapsed
    );

    // 断言 2：开始顺序与 provider 原始 tool_calls 顺序一致
    let log = order_log.lock().unwrap().clone();
    assert_eq!(
        log,
        vec!["call_s1".to_string(), "call_p1".to_string()],
        "开始顺序应与 provider 原始顺序一致，实际 {:?}",
        log
    );

    // 断言 3：conversation 中 tool observation 的 tool_call_id 顺序与原始顺序一致
    let messages = conv.get_messages(sid).await.expect("get messages");
    let tool_ids: Vec<_> = messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(|m| m.tool_call_id.clone())
        .collect();
    assert_eq!(
        tool_ids,
        vec![Some("call_s1".to_string()), Some("call_p1".to_string())],
        "tool observation 顺序应与 provider 原始顺序一致"
    );

    assert!(result.answer.contains("任务完成"));
    let _ = std::fs::remove_dir_all(&workspace);
}

// ── Step 3：terminate 混合 / 全终止 ──────────────────────────────────────────

/// 终止工具：返回 terminate=true 的成功结果
struct TerminateBatchTool;
#[async_trait::async_trait]
impl Tool for TerminateBatchTool {
    fn name(&self) -> &str {
        "terminate_batch_tool"
    }
    fn description(&self) -> &str {
        "signals termination"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"msg":{"type":"string"}}})
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Read
    }
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }
    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let msg = args.get("msg").and_then(|v| v.as_str()).unwrap_or("done");
        ToolResult::success(format!("terminated:{}", msg)).with_terminate()
    }
}

/// 普通工具：返回 terminate=false（默认）
struct PlainTool;
#[async_trait::async_trait]
impl Tool for PlainTool {
    fn name(&self) -> &str {
        "plain_tool"
    }
    fn description(&self) -> &str {
        "a plain tool that does not terminate"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"x":{"type":"string"}}})
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Read
    }
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }
    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let x = args.get("x").and_then(|v| v.as_str()).unwrap_or("?");
        ToolResult::success(format!("plain:{}", x))
    }
}

/// Task 6 验收：混合 batch（terminate + 非 terminate）必须继续下一轮，调用次数 = 2。
#[tokio::test]
async fn mixed_terminate_continues() {
    let workspace = std::env::temp_dir().join("rgoat_test_t6_mixed_term");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let mut registry = ToolRegistry::empty();
    registry.register(Arc::new(TerminateBatchTool));
    registry.register(Arc::new(PlainTool));

    let call_count = Arc::new(AtomicU32::new(0));
    let provider = Arc::new(BatchMockProvider::new(
        call_count.clone(),
        vec![
            tc("call_t1", "terminate_batch_tool", serde_json::json!({"msg":"end"})),
            tc("call_n1", "plain_tool", serde_json::json!({"x":"y"})),
        ],
        "tool_calls",
        "混合 batch 已处理，任务完成。",
        2,
    ));

    let (agent, conv, _rx) = build_test_agent_with_registry(
        workspace.clone(),
        provider,
        registry,
    )
    .await;

    let sid = "t6-mixed-term";
    conv.get_or_create_session(sid, Some("T6 Mixed Terminate"), None)
        .await
        .unwrap();

    let result = agent
        .run(sid, "混合调用", &workspace.to_string_lossy())
        .await
        .expect("agent should complete");

    // 混合 batch（非全部 terminate）→ 必须继续下一轮 → 调用次数 = 2
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "混合 batch 不应终止，应调用 2 轮"
    );
    assert!(result.answer.contains("任务完成"));
    let _ = std::fs::remove_dir_all(&workspace);
}

/// Task 6 验收：全部 terminate 的 batch 应在第 1 轮结束，调用次数 = 1。
#[tokio::test]
async fn all_terminate_stops() {
    let workspace = std::env::temp_dir().join("rgoat_test_t6_all_term");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();

    let mut registry = ToolRegistry::empty();
    registry.register(Arc::new(TerminateBatchTool));

    let call_count = Arc::new(AtomicU32::new(0));
    let provider = Arc::new(BatchMockProvider::new(
        call_count.clone(),
        vec![
            tc("call_t1", "terminate_batch_tool", serde_json::json!({"msg":"a"})),
            tc("call_t2", "terminate_batch_tool", serde_json::json!({"msg":"b"})),
        ],
        "tool_calls",
        "不应到达此答案",
        0, // 不会到达第 2 轮，无需断言 tool 消息数
    ));

    let (agent, conv, _rx) = build_test_agent_with_registry(
        workspace.clone(),
        provider,
        registry,
    )
    .await;

    let sid = "t6-all-term";
    conv.get_or_create_session(sid, Some("T6 All Terminate"), None)
        .await
        .unwrap();

    let result = agent
        .run(sid, "全部终止", &workspace.to_string_lossy())
        .await
        .expect("agent should complete");

    // 全部 terminate → 第 1 轮后结束 → 调用次数 = 1
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "全部 terminate 应在第 1 轮结束，调用次数应为 1"
    );
    assert_eq!(result.steps_taken, 1, "全部 terminate 应单步结束");
    let _ = std::fs::remove_dir_all(&workspace);
}
