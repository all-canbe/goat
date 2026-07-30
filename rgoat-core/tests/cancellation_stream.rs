use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use futures::stream;
use rgoat_core::agent::react::ReActAgent;
use rgoat_core::agent::types::{AgentConfig, AgentError, AgentEvent};
use rgoat_core::conversation::manager::ConversationManager;
use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::core::event_bus::EventBus;
use rgoat_core::provider::provider::{
    ChatMessage, ChatOptions, ChatResponse, LlmError, LlmProvider, LlmStream, ProviderType, StreamChoice,
    StreamChunk, StreamDelta, ToolDef,
};
use rgoat_core::security::approval::{AgentMode, ApprovalDecision, ApprovalEngine};
use rgoat_core::tools::registry::ToolRegistry;

struct CancelledStreamProvider {
    cancellation: CancellationToken,
}

#[async_trait::async_trait]
impl LlmProvider for CancelledStreamProvider {
    async fn chat(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDef],
        _options: &ChatOptions,
    ) -> Result<ChatResponse, LlmError> {
        unreachable!("streaming is enabled")
    }

    async fn chat_stream(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDef],
        _options: &ChatOptions,
    ) -> Result<LlmStream, LlmError> {
        self.cancellation.cancel();
        Ok(Box::pin(stream::iter(vec![Ok(StreamChunk {
            choices: vec![StreamChoice {
                delta: StreamDelta {
                    content: Some("should not be emitted".to_string()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        })])))
    }

    fn name(&self) -> &str {
        "cancelled-stream"
    }

    fn model(&self) -> &str {
        "mock"
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAICompatible
    }
}

#[tokio::test]
async fn streaming_cancellation_emits_cancelled_and_stops() {
    let workspace = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let cancellation = CancellationToken::new();
    let event_bus = Arc::new(EventBus::new(16));
    let mut events = event_bus.subscribe();
    let conversation = Arc::new(
        ConversationManager::new_with_path(&database.path().join("conversations.db"))
            .await
            .unwrap(),
    );
    let agent = ReActAgent::new(
        AgentConfig {
            stream: true,
            planning_enabled: false,
            task_persistence_enabled: false,
            ..AgentConfig::default()
        },
        Arc::new(CancelledStreamProvider {
            cancellation: cancellation.clone(),
        }),
        Arc::new(ToolRegistry::new(workspace.path().to_path_buf())),
        Arc::new(ApprovalEngine::new()),
        conversation.clone(),
        event_bus,
        cancellation,
        AgentMode::Agent,
        Arc::new(AtomicBool::new(false)),
        Arc::new(tokio::sync::Mutex::new(None::<
            tokio::sync::oneshot::Sender<ApprovalDecision>,
        >)),
    );
    conversation
        .get_or_create_session("stream-cancel", Some("test"), None)
        .await
        .unwrap();

    let result = agent
        .run("stream-cancel", "test", &workspace.path().display().to_string())
        .await;

    assert!(matches!(result, Err(AgentError::Cancelled)));
    let mut cancelled = false;
    while let Ok(event) = events.try_recv() {
        if let Ok(AgentEvent::Cancelled { .. }) = serde_json::from_value(event.data) {
            cancelled = true;
        }
    }
    assert!(cancelled, "stream cancellation should emit a Cancelled event");
}
