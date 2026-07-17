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

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rgoat_core::agent::react::ReActAgent;
use rgoat_core::agent::types::{AgentConfig, AgentEvent};
use rgoat_core::conversation::manager::ConversationManager;
use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::core::event_bus::EventBus;
use rgoat_core::provider::provider::{
    ChatMessage, ChatResponse, Choice, LlmError, LlmProvider, LlmStream, MessageContent,
    ProviderType, Role, ToolCallDef, ToolDef,
};
use rgoat_core::security::approval::{AgentMode, ApprovalEngine, ApprovalDecision};
use rgoat_core::tools::registry::ToolRegistry;

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
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef]) -> Result<ChatResponse, LlmError> {
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
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef]) -> Result<LlmStream, LlmError> {
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
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef]) -> Result<ChatResponse, LlmError> {
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
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef]) -> Result<LlmStream, LlmError> {
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
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef]) -> Result<ChatResponse, LlmError> {
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
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef]) -> Result<LlmStream, LlmError> {
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
        async fn chat(&self, _m: &[ChatMessage], _t: &[ToolDef]) -> Result<ChatResponse, LlmError> {
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
        async fn chat_stream(&self, _m: &[ChatMessage], _t: &[ToolDef]) -> Result<LlmStream, LlmError> {
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
