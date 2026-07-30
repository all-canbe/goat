//! Concrete provider implementations
//!
//! Implements LlmProvider trait for OpenAI-compatible and Anthropic APIs

use async_trait::async_trait;
use futures::channel::mpsc;
use futures::SinkExt;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing;

use super::provider::*;

// ═══════════════════════════════════════════════════════════
// OpenAI-compatible HTTP provider (DeepSeek, OpenAI, 智谱, etc.)
// ═══════════════════════════════════════════════════════════

pub struct OpenAiCompatibleProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("OpenAI Compatible HTTP client configuration must be valid");
        Self {
            config,
            client,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        options: &ChatOptions,
    ) -> Result<ChatResponse, LlmError> {
        let body = build_openai_chat_body(&self.config, messages, tools, false, options);

        let mut req = self.client
            .post(format!("{}/chat/completions", self.config.base_url.trim_end_matches('/')))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(ref headers) = self.config.extra_headers {
            for (key, value) in headers {
                req = req.header(key, value);
            }
        }

        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status: status.as_u16(), body: text });
        }
        resp.json().await.map_err(|e| LlmError::Http(e.into()))
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        options: &ChatOptions,
    ) -> Result<LlmStream, LlmError> {
        let body = build_openai_chat_body(&self.config, messages, tools, true, options);

        let mut req = self.client
            .post(format!("{}/chat/completions", self.config.base_url.trim_end_matches('/')))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(ref headers) = self.config.extra_headers {
            for (key, value) in headers {
                req = req.header(key, value);
            }
        }

        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status: status.as_u16(), body: text });
        }

        // SSE streaming: use a channel to bridge the spawn block to a Stream
        let (mut tx, rx) = mpsc::channel::<Result<StreamChunk, LlmError>>(32);

        let provider_name = self.config.name.clone();
        let model_name = self.config.model.clone();
        tracing::info!(
            provider = %provider_name,
            model = %model_name,
            "streaming request start"
        );

        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let mut first_chunk_time: Option<std::time::Duration> = None;
            let mut chunk_count: usize = 0;
            let mut byte_stream = resp.bytes_stream();
            let mut line_buf: Vec<u8> = Vec::new();
            let mut current_data = Vec::<String>::new();
            let mut saw_done = false;

            while let Some(result) = byte_stream.next().await {
                let bytes = match result {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let elapsed = start.elapsed();
                        tracing::warn!(
                            provider = %provider_name,
                            model = %model_name,
                            elapsed_ms = elapsed.as_millis() as u64,
                            chunk_count = chunk_count,
                            "stream body error: {}",
                            error
                        );
                        let _ = tx.send(Err(LlmError::Stream(error.to_string()))).await;
                        return;
                    }
                };

                line_buf.extend_from_slice(&bytes);

                while let Some(newline_pos) = line_buf.iter().position(|&b| b == b'\n') {
                    let line_bytes = &line_buf[..newline_pos];
                    let line_bytes = if line_bytes.last() == Some(&b'\r') {
                        &line_bytes[..line_bytes.len() - 1]
                    } else {
                        line_bytes
                    };
                    let line = String::from_utf8_lossy(line_bytes).into_owned();
                    line_buf.drain(..=newline_pos);

                    if line.is_empty() {
                        if !current_data.is_empty() {
                            if first_chunk_time.is_none() {
                                first_chunk_time = Some(start.elapsed());
                            }
                            if !dispatch_openai_sse_event(&mut current_data, &mut saw_done, &mut tx).await {
                                return;
                            }
                            if !saw_done {
                                chunk_count += 1;
                            }
                        }
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data:") {
                        current_data.push(data.trim_start().to_owned());
                    }
                    // 忽略 event:, id:, retry:, :comment 等非 data 行
                }
            }

            if !current_data.is_empty() && !saw_done {
                dispatch_openai_sse_event(&mut current_data, &mut saw_done, &mut tx).await;
                if !saw_done {
                    chunk_count += 1;
                }
            }

            let elapsed = start.elapsed();
            let end_reason = if saw_done { "[DONE]" } else { "EOF" };
            tracing::info!(
                provider = %provider_name,
                model = %model_name,
                elapsed_ms = elapsed.as_millis() as u64,
                chunk_count = chunk_count,
                first_chunk_ms = first_chunk_time.map(|d| d.as_millis() as u64).unwrap_or(0),
                end_reason = end_reason,
                "streaming request end"
            );
        });

        Ok(Box::pin(rx))
    }

    fn name(&self) -> &str { &self.config.name }
    fn model(&self) -> &str { &self.config.model }
    fn provider_type(&self) -> ProviderType { self.config.provider_type }
    fn base_url(&self) -> &str { &self.config.base_url }
    fn api_key(&self) -> &str { &self.config.api_key }
}

// ═══════════════════════════════════════════════════════════
// Anthropic-compatible HTTP provider (Claude via Messages API)
// ═══════════════════════════════════════════════════════════

pub struct AnthropicProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self { config, client: reqwest::Client::new() }
    }

    fn build_anthropic_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<serde_json::Value>) {
        let mut system = None;
        let mut anthropic_msgs = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => {
                    let text = content_to_str(&msg.content);
                    system = Some(if let Some(ref s) = system {
                        format!("{}\n{}", s, text)
                    } else {
                        text
                    });
                }
                Role::User => {
                    anthropic_msgs.push(json!({
                        "role": "user",
                        "content": content_to_str(&msg.content),
                    }));
                }
                Role::Assistant => {
                    anthropic_msgs.push(json!({
                        "role": "assistant",
                        "content": content_to_str(&msg.content),
                    }));
                }
                Role::Tool => {
                    // Tool result → user message with tool_result content block
                    let tool_call_id = msg.tool_call_id.as_deref().unwrap_or("");
                    let text = content_to_str(&msg.content);
                    anthropic_msgs.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": text,
                        }],
                    }));
                }
            }
        }

        (system, anthropic_msgs)
    }

    fn convert_tools(tools: &[ToolDef]) -> Vec<serde_json::Value> {
        tools.iter().map(|td| {
            json!({
                "name": td.function.name,
                "description": td.function.description,
                "input_schema": td.function.parameters,
            })
        }).collect()
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        _options: &ChatOptions,
    ) -> Result<ChatResponse, LlmError> {
        // Anthropic ignores ChatOptions for now.
        let (system, anthropic_msgs) = Self::build_anthropic_messages(messages);
        let anthropic_tools = Self::convert_tools(tools);

        let mut body = json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens.unwrap_or(4096),
            "temperature": self.config.temperature.unwrap_or(0.3),
            "messages": anthropic_msgs,
        });

        if let Some(sys) = system {
            body["system"] = json!(sys);
        }
        if !anthropic_tools.is_empty() {
            body["tools"] = json!(anthropic_tools);
        }

        let url = format!("{}/messages", self.config.base_url.trim_end_matches('/'));
        let mut req = self.client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(ref headers) = self.config.extra_headers {
            for (key, value) in headers {
                req = req.header(key, value);
            }
        }

        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status: status.as_u16(), body: text });
        }

        let raw: AnthropicResponse = resp.json().await.map_err(|e| LlmError::Http(e.into()))?;
        Ok(convert_anthropic_response(raw))
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        _options: &ChatOptions,
    ) -> Result<LlmStream, LlmError> {
        // Anthropic ignores ChatOptions for now.
        let (system, anthropic_msgs) = Self::build_anthropic_messages(messages);
        let anthropic_tools = Self::convert_tools(tools);

        let mut body = json!({
            "model": self.config.model,
            "messages": anthropic_msgs,
            "stream": true,
            "max_tokens": self.config.max_tokens.unwrap_or(4096),
        });

        if let Some(sys) = system {
            body["system"] = json!(sys);
        }
        if !anthropic_tools.is_empty() {
            body["tools"] = json!(anthropic_tools);
        }

        let url = format!("{}/messages", self.config.base_url.trim_end_matches('/'));
        let mut req = self.client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(ref headers) = self.config.extra_headers {
            for (key, value) in headers {
                req = req.header(key, value);
            }
        }

        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status: status.as_u16(), body: text });
        }

        // SSE streaming: use a channel to bridge the spawn block to a Stream
        let (mut tx, rx) = mpsc::channel::<Result<StreamChunk, LlmError>>(32);

        tokio::spawn(async move {
            let mut byte_stream = resp.bytes_stream();
            let mut line_buf = String::new();
            let mut current_data = String::new();

            // Tool use accumulators: index → (name, id, json_args)
            let mut tool_use_builders: BTreeMap<u32, (Option<String>, String, String)> = BTreeMap::new();
            // Track input tokens from message_start, output from message_delta
            let mut stream_input_tokens: Option<u64> = None;
            let mut stream_output_tokens: Option<u64> = None;

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.try_send(Err(LlmError::Stream(e.to_string())));
                        return;
                    }
                };

                let chunk_str = String::from_utf8_lossy(&chunk);
                line_buf.push_str(&chunk_str);

                while let Some(line_end) = line_buf.find('\n') {
                    let line = line_buf[..line_end].trim().to_string();
                    line_buf = line_buf[line_end + 1..].to_string();

                    if line.is_empty() {
                        // Empty line = end of SSE event
                        if let Some(data) = current_data.strip_prefix("data: ") {
                            if let Ok(event_data) = serde_json::from_str::<serde_json::Value>(data) {
                                let event_type = event_data["type"].as_str().unwrap_or("");

                                match event_type {
                                    "message_start" => {
                                        tool_use_builders.clear();
                                        // Capture input_tokens from message_start usage
                                        if let Some(usage) = event_data["message"]["usage"].as_object() {
                                            stream_input_tokens = usage.get("input_tokens")
                                                .and_then(|v| v.as_u64());
                                        }
                                    }
                                    "content_block_start" => {
                                        let block = &event_data["content_block"];
                                        if block["type"].as_str() == Some("tool_use") {
                                            let index = event_data["index"].as_u64().unwrap_or(0) as u32;
                                            let name = block["name"].as_str().unwrap_or("").to_string();
                                            let id = block["id"].as_str().unwrap_or("").to_string();
                                            tool_use_builders.insert(index, (Some(name), id, String::new()));
                                        }
                                    }
                                    "content_block_delta" => {
                                        let delta = &event_data["delta"];

                                        if delta["type"] == "text_delta" {
                                            let text = delta["text"].as_str().unwrap_or("");
                                            let chunk = StreamChunk {
                                                choices: vec![StreamChoice {
                                                    delta: StreamDelta {
                                                        content: Some(text.to_string()),
                                                        tool_calls: None,
                                                    },
                                                    finish_reason: None,
                                                }],
                                                usage: None,
                                            };
                                            let _ = tx.try_send(Ok(chunk));
                                        } else if delta["type"] == "input_json_delta" {
                                            let partial = delta["partial_json"].as_str().unwrap_or("");
                                            let index = event_data["index"].as_u64().unwrap_or(0) as u32;
                                            if let Some((_, _, ref mut args)) = tool_use_builders.get_mut(&index) {
                                                args.push_str(partial);
                                            }
                                        }
                                    }
                                    "content_block_stop" => {
                                        let block_index = event_data["index"].as_u64().unwrap_or(0) as u32;
                                        if let Some((name, id, args)) = tool_use_builders.get(&block_index) {
                                            if let Some(name) = name {
                                                let json_args = if args.is_empty() { "{}".to_string() } else { args.clone() };
                                                let chunk = StreamChunk {
                                                    choices: vec![StreamChoice {
                                                        delta: StreamDelta {
                                                            content: None,
                                                            tool_calls: Some(vec![StreamToolCallDelta {
                                                                index: block_index,
                                                                id: Some(id.clone()),
                                                                function: Some(StreamFunctionDelta {
                                                                    name: Some(name.clone()),
                                                                    arguments: Some(json_args),
                                                                }),
                                                            }]),
                                                        },
                                                        finish_reason: None,
                                                    }],
                                                    usage: None,
                                                };
                                                let _ = tx.try_send(Ok(chunk));
                                            }
                                        }
                                    }
                                    "message_delta" => {
                                        // stop_reason + usage — emit a finish chunk
                                        let stop_reason = event_data["delta"]["stop_reason"]
                                            .as_str()
                                            .unwrap_or("end_turn");
                                        // Capture output_tokens from message_delta usage
                                        if let Some(output) = event_data["usage"]["output_tokens"].as_u64() {
                                            stream_output_tokens = Some(output);
                                        }
                                        let usage = match (stream_input_tokens, stream_output_tokens) {
                                            (Some(input), Some(output)) => Some(UsageInfo {
                                                prompt_tokens: input,
                                                completion_tokens: output,
                                                total_tokens: input + output,
                                            }),
                                            _ => None,
                                        };
                                        let chunk = StreamChunk {
                                            choices: vec![StreamChoice {
                                                delta: StreamDelta {
                                                    content: None,
                                                    tool_calls: None,
                                                },
                                                finish_reason: Some(stop_reason.to_string()),
                                            }],
                                            usage,
                                        };
                                        let _ = tx.try_send(Ok(chunk));
                                    }
                                    "message_stop" => {
                                        // Stream ended
                                    }
                                    "ping" => {
                                        // Anthropic sends periodic pings to keep the connection alive
                                    }
                                    _ => {}
                                }
                            }
                        }
                        current_data.clear();
                    } else if line.starts_with("event: ") {
                        // Track event type (not strictly needed for data dispatch)
                    } else if line.starts_with("data: ") {
                        current_data = line;
                    }
                }
            }
        });

        Ok(Box::pin(rx))
    }

    fn name(&self) -> &str { &self.config.name }
    fn model(&self) -> &str { &self.config.model }
    fn provider_type(&self) -> ProviderType { ProviderType::Anthropic }
    fn base_url(&self) -> &str { &self.config.base_url }
    fn api_key(&self) -> &str { &self.config.api_key }
}

// ═══════════════════════════════════════════════════════════
// Anthropic response types
// ═══════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AnthropicResponse {
    id: String,
    #[serde(rename = "type")]
    response_type: String,
    role: String,
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text { text: String },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
}

fn convert_anthropic_response(raw: AnthropicResponse) -> ChatResponse {
    let mut content_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in raw.content {
        match block {
            AnthropicContentBlock::Text { text } => {
                content_parts.push(ContentPart::Text { text });
            }
            AnthropicContentBlock::ToolUse { id, name, input } => {
                let args = serde_json::to_string(&input).unwrap_or_default();
                tool_calls.push(ToolCallDef {
                    id,
                    call_type: "function".to_string(),
                    function: FunctionCall { name, arguments: args },
                });
            }
        }
    }

    let message = ChatMessage {
        role: Role::Assistant,
        content: MessageContent::Parts(content_parts),
        name: None,
        tool_call_id: None,
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
    };

    let usage = raw.usage.map(|u| UsageInfo {
        prompt_tokens: u.input_tokens,
        completion_tokens: u.output_tokens,
        total_tokens: u.input_tokens + u.output_tokens,
    });

    ChatResponse {
        choices: vec![Choice {
            message,
            finish_reason: raw.stop_reason,
        }],
        usage,
    }
}

// ═══════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════

fn content_to_str(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Parts(parts) => {
            parts.iter()
                .map(|p| match p {
                    ContentPart::Text { text } => text.clone(),
                    ContentPart::ImageUrl { image_url: _ } => "[image]".to_string(),
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

/// Dispatch a complete SSE event's data lines as a StreamChunk.
///
/// Returns `false` if the channel has been closed (receiver dropped),
/// `true` otherwise.
async fn dispatch_openai_sse_event(
    current_data: &mut Vec<String>,
    saw_done: &mut bool,
    tx: &mut mpsc::Sender<Result<StreamChunk, LlmError>>,
) -> bool {
    let payload = current_data.join("\n");
    current_data.clear();

    if payload == "[DONE]" {
        *saw_done = true;
        return true;
    }

    match serde_json::from_str::<StreamChunk>(&payload) {
        Ok(chunk) => {
            if tx.send(Ok(chunk)).await.is_err() {
                return false;
            }
        }
        Err(e) => {
            tracing::warn!(
                "OpenAI Compatible SSE: skipping malformed event (len={}, err={}): {}",
                payload.len(),
                e,
                payload.chars().take(120).collect::<String>()
            );
        }
    }

    true
}

/// Build OpenAI-compatible chat/completions JSON body (unit-testable).
pub(crate) fn build_openai_chat_body(
    config: &ProviderConfig,
    messages: &[ChatMessage],
    tools: &[ToolDef],
    stream: bool,
    options: &ChatOptions,
) -> serde_json::Value {
    let mut body = json!({
        "model": config.model,
        "messages": messages,
        "max_tokens": config.max_tokens.unwrap_or(4096),
        "temperature": config.temperature.unwrap_or(0.3),
    });
    if stream {
        body["stream"] = json!(true);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    // 请求级思考强度：OpenAI reasoning_effort 字段
    // Low→low, Medium→medium, High→high, Max→high, Default/None→omit
    if let Some(level) = options.thinking_level {
        let effort = match level {
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High | ThinkingLevel::Max => Some("high"),
            ThinkingLevel::Default => None,
        };
        if let Some(effort) = effort {
            body["reasoning_effort"] = json!(effort);
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> ProviderConfig {
        ProviderConfig::openai("test-key", "gpt-test")
    }

    fn sample_messages() -> Vec<ChatMessage> {
        vec![ChatMessage {
            role: Role::User,
            content: MessageContent::Text("hi".into()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }]
    }

    #[test]
    fn non_stream_body_sets_reasoning_effort_high() {
        let options = ChatOptions {
            thinking_level: Some(ThinkingLevel::High),
        };
        let body = build_openai_chat_body(
            &sample_config(),
            &sample_messages(),
            &[],
            false,
            &options,
        );
        assert_eq!(body.get("reasoning_effort").and_then(|v| v.as_str()), Some("high"));
        assert!(body.get("stream").is_none());
    }

    #[test]
    fn non_stream_body_omits_reasoning_effort_for_default_and_none() {
        let body_none = build_openai_chat_body(
            &sample_config(),
            &sample_messages(),
            &[],
            false,
            &ChatOptions::default(),
        );
        assert!(body_none.get("reasoning_effort").is_none());

        let body_default = build_openai_chat_body(
            &sample_config(),
            &sample_messages(),
            &[],
            false,
            &ChatOptions {
                thinking_level: Some(ThinkingLevel::Default),
            },
        );
        assert!(body_default.get("reasoning_effort").is_none());
    }

    #[test]
    fn stream_body_sets_reasoning_effort_high() {
        let options = ChatOptions {
            thinking_level: Some(ThinkingLevel::High),
        };
        let body = build_openai_chat_body(
            &sample_config(),
            &sample_messages(),
            &[],
            true,
            &options,
        );
        assert_eq!(body.get("stream"), Some(&json!(true)));
        assert_eq!(body.get("reasoning_effort").and_then(|v| v.as_str()), Some("high"));
    }

    #[test]
    fn stream_body_omits_reasoning_effort_for_default_and_none() {
        let body_none = build_openai_chat_body(
            &sample_config(),
            &sample_messages(),
            &[],
            true,
            &ChatOptions::default(),
        );
        assert_eq!(body_none.get("stream"), Some(&json!(true)));
        assert!(body_none.get("reasoning_effort").is_none());

        let body_default = build_openai_chat_body(
            &sample_config(),
            &sample_messages(),
            &[],
            true,
            &ChatOptions {
                thinking_level: Some(ThinkingLevel::Default),
            },
        );
        assert_eq!(body_default.get("stream"), Some(&json!(true)));
        assert!(body_default.get("reasoning_effort").is_none());
    }

    #[test]
    fn reasoning_effort_maps_low_medium_max() {
        for (level, expected) in [
            (ThinkingLevel::Low, "low"),
            (ThinkingLevel::Medium, "medium"),
            (ThinkingLevel::Max, "high"),
        ] {
            let body = build_openai_chat_body(
                &sample_config(),
                &sample_messages(),
                &[],
                false,
                &ChatOptions { thinking_level: Some(level) },
            );
            assert_eq!(
                body.get("reasoning_effort").and_then(|v| v.as_str()),
                Some(expected),
                "level {:?} should map to {}",
                level,
                expected
            );
        }
    }
}
