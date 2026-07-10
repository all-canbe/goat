//! Ask user tool — interactive tool for prompting user input during execution
//!
//! Emits events via EventBus and waits for a response via oneshot channel.
//! Supports optional pre-defined options for the frontend to display as buttons.

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use super::registry::{Tool, ToolResult};
use crate::core::event_bus::{EventBus, EventType};
use crate::security::approval::ToolCategory;

/// Interactive tool that asks the user a question during agent execution.
///
/// Uses the EventBus to emit an `ask_user` event and waits for a response
/// via a oneshot channel (5-minute timeout). Multiple concurrent requests
/// are supported via a HashMap keyed by request_id.
pub struct AskUserTool {
    event_bus: Arc<EventBus>,
    /// Map of request_id → oneshot sender for pending user queries
    pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<String>>>>,
}

impl AskUserTool {
    /// Create a new AskUserTool backed by an EventBus.
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            event_bus,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Return a clone of the pending-requests map handle.
    ///
    /// Used by the desktop IPC layer (`respond_ask_user`) to deliver
    /// frontend responses back to the waiting tool invocation.
    pub fn pending_map(&self) -> Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<String>>>> {
        self.pending.clone()
    }
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user a question during execution. Use when you need clarification, confirmation, or additional input."
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Interactive
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "header": {
                    "type": "string",
                    "description": "Short header for the question (default: \"Question\")",
                    "default": "Question"
                },
                "options": {
                    "type": "array",
                    "description": "Optional list of options to present as selectable buttons",
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": {
                                "type": "string",
                                "description": "Display label for the option"
                            },
                            "description": {
                                "type": "string",
                                "description": "Optional description of what this option means"
                            }
                        },
                        "required": ["label"]
                    }
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let question = args["question"].as_str().unwrap_or("").to_string();
        let header = args["header"].as_str().unwrap_or("Question").to_string();
        let options = args.get("options").cloned();
        let request_id = uuid::Uuid::new_v4().to_string();

        // ── Build and emit the ask_user event ──
        let event_data = json!({
            "question": question,
            "header": header,
            "options": options,
            "request_id": request_id,
        });

        self.event_bus.emit(
            EventType::ToolCallStart,
            "ask_user",
            event_data,
        );

        // ── Create oneshot channel for the response ──
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();

        // Store sender keyed by request_id (short-lived lock)
        {
            let mut pending = self.pending.lock().unwrap();
            pending.insert(request_id.clone(), tx);
        }

        // ── Wait for response with 5-minute timeout ──
        let timeout = tokio::time::Duration::from_secs(300);
        let result = tokio::time::timeout(timeout, rx).await;

        // Clean up pending entry regardless of outcome
        {
            let mut pending = self.pending.lock().unwrap();
            pending.remove(&request_id);
        }

        match result {
            Ok(Ok(response)) => ToolResult::success(response),
            Ok(Err(_recv_err)) => ToolResult::error(
                "User response channel closed unexpectedly",
                "channel_closed",
            ),
            Err(_elapsed) => ToolResult::success("User did not respond in time"),
        }
    }
}
