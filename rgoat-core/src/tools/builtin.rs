//! 内置工具集
//!
//! 移植 Goat 原版 14+ 工具到 Rust，包括：
//! Read, Write, Edit, Glob, Grep, Shell, Git, WebSearch, WebFetch, AskUser, Notify, Patch

use async_trait::async_trait;
use serde_json::json;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use super::ask_user::AskUserTool;
use super::registry::{Tool, ToolResult};
use super::task_tool::TaskTool;
use super::web_fetch::WebFetchTool;
use super::web_search::WebSearchTool;
use crate::agent::subagent::SubAgentRuntime;
use crate::core::cancellation::CancellationToken;
use crate::core::event_bus::EventBus;
use crate::security::approval::ToolCategory;

// ============================================================================
// D1-T01: diff 生成辅助
// ============================================================================

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

/// Result of `create_builtin_tools` — the tool list plus an optional
/// handle to the AskUserTool's pending-requests map for desktop IPC.
pub struct BuiltinTools {
    pub tools: Vec<Arc<dyn Tool>>,
    pub pending_ask_user: Option<Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<String>>>>>,
}

// ============================================================================
// 工厂函数
// ============================================================================

/// 创建所有内置工具
///
/// `subagent_runtime`、`cancellation`、`event_bus` 为可选参数。
/// 当 `subagent_runtime` 和 `cancellation` 均为 `Some` 时，TaskTool 会被注册。
/// 当 `event_bus` 为 `Some` 时，AskUserTool 会被注册。
pub fn create_builtin_tools(
    workspace: PathBuf,
    http_client: reqwest::Client,
    subagent_runtime: Option<Arc<SubAgentRuntime>>,
    cancellation: Option<CancellationToken>,
    event_bus: Option<Arc<EventBus>>,
) -> BuiltinTools {
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(ReadFileTool::new(workspace.clone())),
        Arc::new(WriteFileTool::new(workspace.clone())),
        Arc::new(EditFileTool::new(workspace.clone())),
        Arc::new(GlobTool::new(workspace.clone())),
        Arc::new(GrepTool::new(workspace.clone())),
        Arc::new(ShellTool::new(workspace.clone())),
        Arc::new(GitTool::new(workspace.clone())),
        Arc::new(WebSearchTool::new(http_client.clone())),
        Arc::new(WebFetchTool::new(http_client)),
    ];
    let mut pending_ask_user = None;

    // Register TaskTool only when sub-agent infrastructure is available
    if let (Some(runtime), Some(cancel)) = (subagent_runtime, cancellation) {
        tools.push(Arc::new(TaskTool::new(runtime, cancel, workspace.clone())));
    }

    // Register AskUserTool when EventBus is available
    if let Some(bus) = event_bus {
        let ask_tool = AskUserTool::new(bus);
        pending_ask_user = Some(ask_tool.pending_map());
        tools.push(Arc::new(ask_tool));
    }

    BuiltinTools {
        tools,
        pending_ask_user,
    }
}

// ============================================================================
// Read — 读取文件
// ============================================================================

pub struct ReadFileTool {
    workspace: PathBuf,
}

impl ReadFileTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }

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
                    "description": "Line number to start reading from"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                }
            },
            "required": ["file_path"]
        })
    }

    fn category(&self) -> ToolCategory { ToolCategory::Read }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let file_path = args["file_path"].as_str().unwrap_or("");
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(2000) as usize;

        let path = crate::core::paths::safe_path(file_path, &self.workspace);

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().skip(offset).take(limit).collect();
                ToolResult::success(lines.join("\n"))
            }
            Err(e) => ToolResult::error(
                format!("Failed to read file: {}", e),
                format!("{}", e),
            ),
        }
    }
}

// ============================================================================
// Write — 写入文件
// ============================================================================

pub struct WriteFileTool {
    workspace: PathBuf,
}

impl WriteFileTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }

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

    fn category(&self) -> ToolCategory { ToolCategory::Write }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let file_path = args["file_path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let path = crate::core::paths::safe_path(file_path, &self.workspace);

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

        match std::fs::write(&path, content) {
            Ok(_) => {
                let rel_path = relative_path_str(&path, &self.workspace);
                let diff = generate_unified_diff(&old_content, content, &rel_path);
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
            }
            Err(e) => ToolResult::error(
                format!("Failed to write file: {}", e),
                format!("{}", e),
            ),
        }
    }
}

// ============================================================================
// Edit — 精确字符串替换编辑
// ============================================================================

pub struct EditFileTool {
    workspace: PathBuf,
}

impl EditFileTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str { "edit_file" }

    fn description(&self) -> &str {
        "Performs exact string replacements in an existing file. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to modify"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace it with"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false)",
                    "default": false
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    fn category(&self) -> ToolCategory { ToolCategory::Write }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let file_path = args["file_path"].as_str().unwrap_or("");
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);
        let path = crate::core::paths::safe_path(file_path, &self.workspace);

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e), format!("{}", e)),
        };

        if old_string.is_empty() {
            return ToolResult::error("old_string is empty", "empty_old_string");
        }

        let occurrences = content.matches(old_string).count();
        if occurrences == 0 {
            return ToolResult::error(
                "old_string not found in file",
                "string_not_found",
            );
        }

        if occurrences > 1 && !replace_all {
            return ToolResult::error(
                format!(
                    "Found {} occurrences of old_string. Use replace_all=true or provide more context.",
                    occurrences
                ),
                "multiple_occurrences",
            );
        }

        if old_string == new_string {
            return ToolResult::error("old_string and new_string are identical", "no_change");
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        match std::fs::write(&path, &new_content) {
            Ok(_) => {
                // D1-T01: 生成 unified diff
                let rel_path = relative_path_str(&path, &self.workspace);
                let diff = generate_unified_diff(&content, &new_content, &rel_path);
                let affected = vec![rel_path];
                ToolResult::success(format!(
                    "Successfully edited {}. Replaced {} occurrence(s).",
                    file_path,
                    if replace_all { occurrences } else { 1 }
                ))
                .with_diff(diff, affected)
                .with_metadata(json!({
                    "change_type": "edit",
                    "old_size": content.len(),
                    "new_size": new_content.len(),
                    "occurrences_replaced": if replace_all { occurrences } else { 1 },
                }))
            }
            Err(e) => ToolResult::error(format!("Failed to write file: {}", e), format!("{}", e)),
        }
    }
}

// ============================================================================
// Glob — 文件模式匹配
// ============================================================================

pub struct GlobTool {
    workspace: PathBuf,
}

impl GlobTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str { "glob" }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match (e.g., '**/*.rs')"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (defaults to workspace)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn category(&self) -> ToolCategory { ToolCategory::Read }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let path = args["path"]
            .as_str()
            .map(|p| crate::core::paths::safe_path(p, &self.workspace))
            .unwrap_or_else(|| self.workspace.clone());

        match glob::glob(&path.join(pattern).to_string_lossy()) {
            Ok(entries) => {
                let files: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .filter(|p| p.is_file())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();

                if files.is_empty() {
                    ToolResult::success("No files found")
                } else {
                    ToolResult::success(files.join("\n"))
                }
            }
            Err(e) => ToolResult::error(
                format!("Glob error: {}", e),
                format!("{}", e),
            ),
        }
    }
}

// ============================================================================
// Grep — 内容搜索
// ============================================================================

pub struct GrepTool {
    workspace: PathBuf,
}

impl GrepTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str { "grep" }

    fn description(&self) -> &str {
        "Search for a pattern in file contents. Supports regex."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in"
                },
                "include": {
                    "type": "string",
                    "description": "File pattern to include (e.g., '*.rs')"
                }
            },
            "required": ["pattern"]
        })
    }

    fn category(&self) -> ToolCategory { ToolCategory::Read }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let search_path = args["path"]
            .as_str()
            .map(|p| crate::core::paths::safe_path(p, &self.workspace))
            .unwrap_or_else(|| self.workspace.clone());

        let include = args["include"].as_str();

        let re = match regex::Regex::new(pattern) {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Invalid regex: {}", e), format!("{}", e)),
        };

        let mut results = Vec::new();
        let walker = ignore::WalkBuilder::new(&search_path)
            .hidden(false)
            .git_ignore(true)
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }

            let path = entry.path();

            // Filter by include pattern
            if let Some(inc) = include {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !glob::Pattern::new(inc).map_or(false, |p| p.matches(name)) {
                        continue;
                    }
                }
            }

            if let Ok(content) = std::fs::read_to_string(path) {
                for (line_num, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        results.push(format!(
                            "{}:{}: {}",
                            path.display(),
                            line_num + 1,
                            line.trim()
                        ));
                        if results.len() >= 100 {
                            break;
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            ToolResult::success("No matches found")
        } else {
            ToolResult::success(results.join("\n")).with_metadata(json!({
                "match_count": results.len()
            }))
        }
    }
}

// ============================================================================
// Shell — 执行系统命令（含超时控制）
// ============================================================================

pub struct ShellTool {
    workspace: PathBuf,
}

impl ShellTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str { "shell" }

    fn description(&self) -> &str {
        "Execute a shell command in the workspace directory. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 30000)",
                    "default": 30000
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let command = args["command"].as_str().unwrap_or("").to_string();
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(30000);

        // Use cmd.exe on Windows, sh elsewhere
        #[cfg(windows)]
        let (shell, shell_arg) = ("cmd", "/C");
        #[cfg(not(windows))]
        let (shell, shell_arg) = ("sh", "-c");

        let workspace = self.workspace.clone();
        let shell = shell.to_string();
        let shell_arg = shell_arg.to_string();

        // Wrap synchronous process call in spawn_blocking + tokio::time::timeout
        let timeout_dur = std::time::Duration::from_millis(timeout_ms);
        let spawn_result = tokio::time::timeout(
            timeout_dur,
            tokio::task::spawn_blocking(move || {
                std::process::Command::new(&shell)
                    .arg(&shell_arg)
                    .arg(&command)
                    .current_dir(&workspace)
                    .output()
            }),
        )
        .await;

        match spawn_result {
            // Timeout elapsed
            Err(_elapsed) => ToolResult::error(
                format!("Shell command timed out after {}ms", timeout_ms),
                "timeout",
            ),
            // spawn_blocking completed
            Ok(Ok(Ok(out))) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let exit_code = out.status.code().unwrap_or(-1);

                let mut result = String::new();
                if !stdout.is_empty() {
                    result.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str("[stderr]\n");
                    result.push_str(&stderr);
                }
                if result.is_empty() {
                    result = "(no output)".to_string();
                }

                ToolResult::success(result).with_metadata(json!({
                    "exit_code": exit_code,
                    "stdout_len": stdout.len(),
                    "stderr_len": stderr.len(),
                }))
            }
            // spawn_blocking itself failed (join error)
            Ok(Err(join_err)) => ToolResult::error(
                format!("Shell task panicked or was cancelled: {}", join_err),
                "spawn_error",
            ),
            // spawn_blocking returned an error from the command
            Ok(Ok(Err(cmd_err))) => ToolResult::error(
                format!("Command failed: {}", cmd_err),
                format!("{}", cmd_err),
            ),
        }
    }
}

// ============================================================================
// Git — Git 操作
// ============================================================================

pub struct GitTool {
    workspace: PathBuf,
}

impl GitTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for GitTool {
    fn name(&self) -> &str { "git" }

    fn description(&self) -> &str {
        "Run git commands in the workspace. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Git subcommand to run (e.g., 'status', 'diff', 'log')"
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> ToolCategory { ToolCategory::Read }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let command = args["command"].as_str().unwrap_or("");

        let output = std::process::Command::new("git")
            .current_dir(&self.workspace)
            .args(command.split_whitespace())
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let exit_code = out.status.code().unwrap_or(-1);

                let result = if !stdout.is_empty() {
                    stdout
                } else if !stderr.is_empty() {
                    stderr
                } else {
                    "(no output)".to_string()
                };

                ToolResult::success(result).with_metadata(json!({
                    "exit_code": exit_code
                }))
            }
            Err(e) => ToolResult::error(
                format!("Git command failed: {}", e),
                format!("{}", e),
            ),
        }
    }
}
