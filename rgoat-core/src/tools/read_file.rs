//! Read 工具 — 读取本地文件
//!
//! 从 builtin.rs 拆分而来。保持原有功能不变，新增 CancellationToken 检查点：
//! 读文件前若已取消则直接返回。
//!
//! 增强：
//! - offset 改为 1-indexed（默认 1），与 description 语义一致
//! - 输出加 cat -n 风格行号前缀，便于模型精确引用行号
//! - 读取后若仍有剩余行，追加截断提示
//! - 二进制文件检测（UTF-8 解码失败时返回提示而非乱码）
//! - 路径变体查找（特殊空格归一化、smart quote）

use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

use super::registry::{Tool, ToolResult};
use crate::core::cancellation::CancellationToken;
use crate::security::approval::ToolCategory;

pub struct ReadFileTool {
    workspace: PathBuf,
    cancellation: Option<CancellationToken>,
}

impl ReadFileTool {
    pub fn new(workspace: PathBuf, cancellation: Option<CancellationToken>) -> Self {
        Self {
            workspace,
            cancellation,
        }
    }
}

/// 路径变体查找 — 处理特殊空格和 smart quote
///
/// 依次尝试：
/// 1. 原路径
/// 2. NBSP(U+00A0) 及特殊空格(U+2002-U+200A, U+202F, U+205F, U+3000) 替换为普通空格
/// 3. 单引号 ' 替换为 smart quote ' (U+2019)
/// 4. 组合：空格归一化 + smart quote
///
/// 找到第一个 exists 的变体返回；都不存在返回原路径（让后续 read 自然报错）。
/// 参考 pi path-utils.ts:16-29 的 variants 数组思路。
fn resolve_read_path(path: &Path) -> PathBuf {
    let original = path.to_path_buf();
    if original.exists() {
        return original;
    }

    let s = match path.to_str() {
        Some(s) => s,
        None => return original,
    };

    // 空格归一化：特殊空格 → 普通空格
    let space_normalized: String = s
        .chars()
        .map(|c| match c {
            '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
            _ => c,
        })
        .collect();

    // smart quote 替换：普通单引号 → U+2019
    let smart_quote: String = s.replace('\'', "\u{2019}");

    // 组合：空格归一化 + smart quote
    let combined: String = space_normalized.replace('\'', "\u{2019}");

    for variant in [space_normalized, smart_quote, combined] {
        let p = PathBuf::from(variant);
        if p.exists() {
            return p;
        }
    }

    original
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Reads a file from the local filesystem. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed)",
                    "default": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read",
                    "default": 2000
                }
            },
            "required": ["file_path"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Read
    }

    fn execution_mode(&self) -> super::registry::ExecutionMode {
        super::registry::ExecutionMode::Parallel
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let file_path = args["file_path"].as_str().unwrap_or("");
        // offset 1-indexed，默认 1；0 视为 1（保持 1-indexed 语义）
        let offset = args["offset"].as_u64().unwrap_or(1).max(1) as usize;
        let limit = args["limit"].as_u64().unwrap_or(2000) as usize;

        let path = match crate::core::paths::resolve_workspace_path(file_path, &self.workspace) {
            Ok(p) => p,
            Err(_) => {
                return ToolResult::error(
                    "Path must be inside the workspace",
                    "path_outside_workspace",
                )
            }
        };
        // 路径变体查找（特殊空格归一化、smart quote）
        let path = resolve_read_path(&path);

        // 读文件前检查取消
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation aborted", "cancelled");
            }
        }

        // 读 bytes 用于二进制检测
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                return ToolResult::error(
                    format!("Failed to read file: {}", e),
                    format!("{}", e),
                );
            }
        };

        // UTF-8 解码检测二进制
        let content = match String::from_utf8(bytes.clone()) {
            Ok(s) => s,
            Err(_) => {
                // 二进制文件：返回提示而非乱码
                return ToolResult::success(format!(
                    "[Binary file: {} ({} bytes). Content not displayed.]",
                    path.display(),
                    bytes.len()
                ))
                .with_metadata(json!({
                    "binary": true,
                    "size": bytes.len(),
                }));
            }
        };

        // 行号显示 + 截断
        let all_lines: Vec<&str> = content.lines().collect();
        let total = all_lines.len();

        if total == 0 {
            return ToolResult::success(String::new());
        }

        // 1-indexed: skip(offset-1)
        let skip = offset - 1;
        let start_display = offset; // 第一行行号 = offset

        let lines: Vec<&str> = all_lines.iter().skip(skip).take(limit).copied().collect();
        if lines.is_empty() {
            return ToolResult::success(String::new());
        }

        // cat -n 风格行号前缀: {:>6}\t{line}
        let numbered: String = lines
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>6}\t{}", start_display + i, line))
            .collect::<Vec<_>>()
            .join("\n");

        let end_display = start_display + lines.len() - 1;

        // 截断提示：还有剩余行时追加
        let output = if total > end_display {
            let next = end_display + 1;
            format!(
                "{}\n\n[Showing lines {}-{} of {}. Use offset={} to continue.]",
                numbered, start_display, end_display, total, next
            )
        } else {
            numbered
        };

        ToolResult::success(output)
    }
}
