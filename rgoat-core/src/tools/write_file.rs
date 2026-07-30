//! Write 工具 — 写入本地文件
//!
//! 从 builtin.rs 拆分而来。持有 `FileMutationQueue` 序列化同一文件的并发写入，
//! 并在写文件前检查 `CancellationToken`。diff 生成逻辑与原 builtin.rs 完全一致。

use async_trait::async_trait;
use serde_json::json;
use similar::{ChangeTag, TextDiff};
use std::path::PathBuf;
use std::sync::Arc;

use super::file_mutation_queue::FileMutationQueue;
use super::registry::{Tool, ToolResult};
use crate::core::cancellation::CancellationToken;
use crate::security::approval::ToolCategory;

pub struct WriteFileTool {
    workspace: PathBuf,
    mutation_queue: Arc<FileMutationQueue>,
    cancellation: Option<CancellationToken>,
}

impl WriteFileTool {
    pub fn new(
        workspace: PathBuf,
        cancellation: Option<CancellationToken>,
        mutation_queue: Arc<FileMutationQueue>,
    ) -> Self {
        Self {
            workspace,
            cancellation,
            mutation_queue,
        }
    }
}

/// 生成 unified diff 格式字符串。
///
/// 输入旧/新文本与展示用文件路径，返回 `--- a/<path>\n+++ b/<path>\n` 开头的 diff。
/// 若 old == new 返回空字符串。
fn generate_unified_diff(old: &str, new: &str, file_path: &str) -> String {
    if old == new {
        return String::new();
    }
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();
    output.push_str(&format!("--- a/{}\n", file_path));
    output.push_str(&format!("+++ b/{}\n", file_path));
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => '-',
            ChangeTag::Insert => '+',
            ChangeTag::Equal => ' ',
        };
        output.push(sign);
        output.push_str(change.value());
        // 最后一行可能无换行符，补齐以保证 diff 行对齐
        if !change.value().ends_with('\n') {
            output.push('\n');
        }
    }
    output
}

/// 将绝对路径转为相对 workspace 的路径字符串（用于 diff 头部展示）。
fn relative_path_str(path: &std::path::Path, workspace: &std::path::Path) -> String {
    path.strip_prefix(workspace)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Writes a file to the local filesystem. Creates parent directories if needed."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Write
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let file_path = args["file_path"].as_str().unwrap_or("").to_string();
        let content = args["content"].as_str().unwrap_or("").to_string();
        let path = match crate::core::paths::resolve_workspace_path(&file_path, &self.workspace) {
            Ok(p) => p,
            Err(_) => {
                return ToolResult::error(
                    "Path must be inside the workspace",
                    "path_outside_workspace",
                )
            }
        };
        let workspace = self.workspace.clone();
        let cancellation = self.cancellation.clone();

        // 检查点1：入队前检查取消（快速失败，避免无谓排队）
        if let Some(token) = &cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation aborted", "cancelled");
            }
        }

        // 克隆 path 仅用于 run 的 &Path 参数；原 path 进入 async 块
        let path_for_key = path.clone();
        self.mutation_queue
            .run(&path_for_key, async move {
                // 检查点2：持锁后、读旧内容前再次检查取消（排队期间可能被取消）
                if let Some(token) = &cancellation {
                    if token.is_cancelled() {
                        return ToolResult::error("Operation aborted", "cancelled");
                    }
                }

                if let Some(parent) = path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        return ToolResult::error(
                            format!("Failed to create parent directories: {}", e),
                            format!("{}", e),
                        );
                    }
                }

                // D1-T01: 写入前捕获旧内容以生成 diff
                let old_content = std::fs::read_to_string(&path).unwrap_or_default();
                let is_new_file = !path.exists();

                // 检查点3：写新内容前检查取消（避免无谓 IO）
                if let Some(token) = &cancellation {
                    if token.is_cancelled() {
                        return ToolResult::error("Operation aborted", "cancelled");
                    }
                }

                // 原子写入：先写同目录临时文件，再 rename 原子覆盖目标。
                // 中途崩溃（断电、panic）只会留下临时文件，目标文件保持完整。
                // 同文件系统 rename 是原子的；Windows 上 std::fs::rename 用
                // MoveFileExW with REPLACE_EXISTING，可原子覆盖已存在文件。
                let tmp_path = path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(format!(
                        ".{}.tmp.{}.{}",
                        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
                        std::process::id(),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_nanos())
                            .unwrap_or(0),
                    ));

                if let Err(e) = std::fs::write(&tmp_path, &content) {
                    // 清理可能残留的半截临时文件
                    let _ = std::fs::remove_file(&tmp_path);
                    return ToolResult::error(
                        format!("Failed to write temp file: {}", e),
                        format!("{}", e),
                    );
                }

                if let Err(e) = std::fs::rename(&tmp_path, &path) {
                    // rename 失败时清理临时文件，避免孤儿
                    let _ = std::fs::remove_file(&tmp_path);
                    return ToolResult::error(
                        format!("Failed to rename temp file: {}", e),
                        format!("{}", e),
                    );
                }

                // 检查点4：写完成后返回前检查取消（与 ts 参考一致）
                if let Some(token) = &cancellation {
                    if token.is_cancelled() {
                        return ToolResult::error("Operation aborted", "cancelled");
                    }
                }

                let rel_path = relative_path_str(&path, &workspace);
                let diff = generate_unified_diff(&old_content, &content, &rel_path);
                let change_type = if is_new_file { "create" } else { "edit" };
                let affected = vec![rel_path];
                ToolResult::success(format!(
                    "Successfully wrote to {} ({})",
                    file_path, change_type
                ))
                .with_diff(diff, affected)
                .with_metadata(json!({
                    "change_type": change_type,
                    "old_size": old_content.len(),
                    "new_size": content.len(),
                }))
            })
            .await
    }
}
