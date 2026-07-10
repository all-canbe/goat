//! MCP 协议 — 客户端/服务端
//!
//! 基于 JSON-RPC 2.0 的 Model Context Protocol 实现：
//! - `client`：连接外部 MCP 服务器，将其工具引入 rgoat
//! - `server`：将 rgoat 工具暴露为 MCP 服务器
//! - `types`：协议类型定义

pub mod types;
pub mod client;
pub mod server;

pub use types::*;
pub use client::{McpClient, StdioMcpClient, McpToolAdapter};
pub use server::{McpServer, GoatMcpServer, run_stdio_server};
