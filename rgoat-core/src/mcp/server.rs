//! MCP 服务端
//!
//! 将 rgoat 的内置工具暴露为 MCP 工具，供其他客户端调用

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info};

use crate::mcp::types::*;
use crate::tools::registry::ToolRegistry;

/// MCP 服务端 trait
#[async_trait]
pub trait McpServer: Send + Sync {
    async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse;
}

/// 基于 rgoat 工具集的 MCP 服务端
pub struct GoatMcpServer {
    tools: Arc<ToolRegistry>,
    server_info: ImplementationInfo,
}

impl GoatMcpServer {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self {
            tools,
            server_info: ImplementationInfo {
                name: "rgoat-mcp-server".to_string(),
                version: crate::VERSION.to_string(),
            },
        }
    }

    fn make_response(id: Option<serde_json::Value>, result: Result<serde_json::Value, McpError>) -> JsonRpcResponse {
        match result {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(result),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(e.to_json_rpc_error()),
            },
        }
    }
}

#[async_trait]
impl McpServer for GoatMcpServer {
    async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        info!("MCP method: {}", request.method);
        let id = request.id.clone();

        let result = match request.method.as_str() {
            "initialize" => {
                let result = InitializeResult {
                    protocol_version: "2024-11-05".to_string(),
                    capabilities: ServerCapabilities {
                        tools: Some(ToolsCapability { list_changed: false }),
                        resources: None,
                        prompts: None,
                    },
                    server_info: self.server_info.clone(),
                };
                Ok(serde_json::to_value(result).unwrap_or_default())
            }
            "tools/list" => {
                let tools: Vec<McpTool> = self.tools.all_tool_defs().into_iter().map(|td| McpTool {
                    name: td.function.name,
                    description: td.function.description,
                    input_schema: td.function.parameters,
                }).collect();
                Ok(json!({ "tools": tools }))
            }
            "tools/call" => {
                let params = match request.params {
                    Some(p) => p,
                    None => return Self::make_response(id, Err(McpError::InvalidParams)),
                };
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                let result = self.tools.execute(name, arguments).await;
                let mcp_result = McpToolResult {
                    content: vec![McpContent::Text { text: result.output }],
                    is_error: if result.success { None } else { Some(true) },
                };
                Ok(serde_json::to_value(mcp_result).unwrap_or_default())
            }
            "resources/list" | "resources/read" | "prompts/list" => {
                Ok(json!({ "resources": [], "prompts": [] }))
            }
            "notifications/initialized" => {
                Ok(json!({}))
            }
            _ => Err(McpError::MethodNotFound(request.method)),
        };

        Self::make_response(id, result)
    }
}

/// 通过 stdio 运行 MCP 服务器
pub async fn run_stdio_server(server: Arc<dyn McpServer>) -> Result<(), McpError> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut stdout = stdout;
    let mut line = String::new();

    info!("MCP stdio server started");

    while reader.read_line(&mut line).await? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        debug!("MCP received: {}", trimmed);
        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let response = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                write_response(&mut stdout, response).await?;
                line.clear();
                continue;
            }
        };

        let response = server.handle_request(request).await;
        write_response(&mut stdout, response).await?;
        line.clear();
    }

    Ok(())
}

async fn write_response(stdout: &mut tokio::io::Stdout, response: JsonRpcResponse) -> Result<(), McpError> {
    let mut text = serde_json::to_string(&response)?;
    text.push('\n');
    stdout.write_all(text.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}
