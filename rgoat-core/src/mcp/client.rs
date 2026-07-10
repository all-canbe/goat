//! MCP 客户端
//!
//! 通过 stdio/sse 连接 MCP 服务器并调用工具

use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::mcp::types::*;
use crate::security::approval::ToolCategory;
use crate::tools::registry::{Tool, ToolResult};

/// MCP 客户端 trait
#[async_trait]
pub trait McpClient: Send + Sync {
    async fn initialize(&mut self) -> Result<InitializeResult, McpError>;
    async fn list_tools(&self) -> Result<Vec<McpTool>, McpError>;
    async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<McpToolResult, McpError>;
    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError>;
    async fn read_resource(&self, uri: &str) -> Result<McpResourceContent, McpError>;
    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError>;
}

/// 基于 stdio 的 MCP 客户端（连接本地命令启动的服务器）
pub struct StdioMcpClient {
    command: String,
    args: Vec<String>,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Mutex<Option<BufReader<ChildStdout>>>,
    request_id: AtomicU64,
}

impl StdioMcpClient {
    pub fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            stdout: Mutex::new(None),
            request_id: AtomicU64::new(1),
        }
    }

    pub async fn connect(&self) -> Result<(), McpError> {
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| {
            McpError::Transport("Failed to open stdin".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            McpError::Transport("Failed to open stdout".to_string())
        })?;

        *self.child.lock().await = Some(child);
        *self.stdin.lock().await = Some(stdin);
        *self.stdout.lock().await = Some(BufReader::new(stdout));
        Ok(())
    }

    async fn send_request(&self, method: &str, params: Option<serde_json::Value>) -> Result<serde_json::Value, McpError> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(id)),
            method: method.to_string(),
            params,
        };

        let mut text = serde_json::to_string(&request)?;
        text.push('\n');

        {
            let mut stdin_guard = self.stdin.lock().await;
            let stdin = stdin_guard.as_mut().ok_or(McpError::NotInitialized)?;
            stdin.write_all(text.as_bytes()).await?;
            stdin.flush().await?;
        }
        debug!("MCP request: {}", text.trim());

        let mut line = String::new();
        {
            let mut stdout_guard = self.stdout.lock().await;
            let stdout = stdout_guard.as_mut().ok_or(McpError::NotInitialized)?;
            stdout.read_line(&mut line).await?;
        }
        debug!("MCP response: {}", line.trim());

        let response: JsonRpcResponse = serde_json::from_str(&line)?;
        if let Some(err) = response.error {
            return Err(McpError::Transport(format!("RPC error {}: {}", err.code, err.message)));
        }

        response.result.ok_or_else(|| McpError::Protocol("Empty result".to_string()))
    }
}

#[async_trait]
impl McpClient for StdioMcpClient {
    async fn initialize(&mut self) -> Result<InitializeResult, McpError> {
        let has_stdin = self.stdin.lock().await.is_some();
        if !has_stdin {
            self.connect().await?;
        }

        let params = InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: ImplementationInfo {
                name: "rgoat".to_string(),
                version: crate::VERSION.to_string(),
            },
        };

        let result = self.send_request("initialize", Some(serde_json::to_value(params)?)).await?;
        let init: InitializeResult = serde_json::from_value(result)?;
        info!("MCP server initialized: {} {}", init.server_info.name, init.server_info.version);
        Ok(init)
    }

    async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        let result = self.send_request("tools/list", None).await?;
        let tools: Vec<McpTool> = serde_json::from_value(result.get("tools").cloned().unwrap_or(json!([])))?;
        Ok(tools)
    }

    async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<McpToolResult, McpError> {
        let params = json!({
            "name": name,
            "arguments": arguments,
        });
        let result = self.send_request("tools/call", Some(params)).await?;
        let tool_result: McpToolResult = serde_json::from_value(result)?;
        Ok(tool_result)
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        let result = self.send_request("resources/list", None).await?;
        let resources: Vec<McpResource> = serde_json::from_value(result.get("resources").cloned().unwrap_or(json!([])))?;
        Ok(resources)
    }

    async fn read_resource(&self, uri: &str) -> Result<McpResourceContent, McpError> {
        let params = json!({ "uri": uri });
        let result = self.send_request("resources/read", Some(params)).await?;
        let content: McpResourceContent = serde_json::from_value(result.get("contents").and_then(|c| c.get(0)).cloned().unwrap_or(json!({})))?;
        Ok(content)
    }

    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError> {
        let result = self.send_request("prompts/list", None).await?;
        let prompts: Vec<McpPrompt> = serde_json::from_value(result.get("prompts").cloned().unwrap_or(json!([])))?;
        Ok(prompts)
    }
}

impl Drop for StdioMcpClient {
    fn drop(&mut self) {
        // Best-effort kill; async drop not available in stable Rust
        if let Ok(mut child) = self.child.try_lock() {
            if let Some(mut c) = child.take() {
                let _ = c.start_kill();
            }
        }
    }
}

/// 将 MCP 工具包装为 rgoat Tool
pub struct McpToolAdapter {
    name: String,
    description: String,
    parameters: serde_json::Value,
    client: Arc<dyn McpClient>,
}

impl McpToolAdapter {
    pub fn new(tool: McpTool, client: Arc<dyn McpClient>) -> Self {
        Self {
            name: tool.name,
            description: tool.description,
            parameters: tool.input_schema,
            client,
        }
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn parameters(&self) -> serde_json::Value { self.parameters.clone() }
    fn category(&self) -> ToolCategory { ToolCategory::Mcp }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        match self.client.call_tool(&self.name, args).await {
            Ok(result) => {
                let text = result.content.iter()
                    .map(|c| match c {
                        McpContent::Text { text } => text.clone(),
                        _ => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                ToolResult::success(text)
            }
            Err(e) => ToolResult::error("", e.to_string()),
        }
    }
}
