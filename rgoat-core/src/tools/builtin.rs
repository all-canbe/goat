//! 内置工具集
//!
//! 移植 Goat 原版 14+ 工具到 Rust，包括：
//! Read, Write, Edit, Glob, Grep, Shell, Git, WebSearch, WebFetch, AskUser, Notify, Patch
//!
//! Read/Write/Edit/Shell 已拆分到独立模块（read_file/write_file/edit_file/shell），
//! 本文件保留 Glob/Grep/Git 工具实现以及 `create_builtin_tools` 工厂函数。

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use super::ask_user::AskUserTool;
use super::edit_file::EditFileTool;
use super::file_mutation_queue::FileMutationQueue;
use super::read_file::ReadFileTool;
use super::registry::{ExecutionMode, Tool, ToolResult};
use super::shell::ShellTool;
use super::task_tool::TaskTool;
use super::web_fetch::{web_fetch_client, WebFetchTool};
use super::web_search::WebSearchTool;
use super::write_file::WriteFileTool;
use crate::agent::subagent::SubAgentRuntime;
use crate::core::cancellation::CancellationToken;
use crate::core::event_bus::EventBus;
use crate::security::approval::ToolCategory;

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
///
/// Read/Write/Edit/Shell/WebSearch 接收 `cancellation` 副本，用于在关键点检查取消状态；
/// Write/Edit 额外共享同一个 `FileMutationQueue`，序列化对同一文件的并发写入。
pub fn create_builtin_tools(
    workspace: PathBuf,
    http_client: reqwest::Client,
    subagent_runtime: Option<Arc<SubAgentRuntime>>,
    cancellation: Option<CancellationToken>,
    event_bus: Option<Arc<EventBus>>,
) -> BuiltinTools {
    let mutation_queue = Arc::new(FileMutationQueue::new());

    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(ReadFileTool::new(workspace.clone(), cancellation.clone())),
        Arc::new(WriteFileTool::new(
            workspace.clone(),
            cancellation.clone(),
            mutation_queue.clone(),
        )),
        Arc::new(EditFileTool::new(
            workspace.clone(),
            cancellation.clone(),
            mutation_queue.clone(),
        )),
        Arc::new(GlobTool::new(workspace.clone(), cancellation.clone())),
        Arc::new(GrepTool::new(workspace.clone(), cancellation.clone())),
        Arc::new(ShellTool::new(workspace.clone(), cancellation.clone())),
        Arc::new(GitTool::new(workspace.clone())),
        Arc::new(WebSearchTool::new(
            http_client.clone(),
            cancellation.clone(),
        )),
        Arc::new(WebFetchTool::new(web_fetch_client(), cancellation.clone())),
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
// Glob — 文件模式匹配
// ============================================================================

pub struct GlobTool {
    workspace: PathBuf,
    cancellation: Option<CancellationToken>,
}

impl GlobTool {
    pub fn new(workspace: PathBuf, cancellation: Option<CancellationToken>) -> Self {
        Self {
            workspace,
            cancellation,
        }
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

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
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 200, max: 1000)",
                    "default": 200
                },
                "respect_gitignore": {
                    "type": "boolean",
                    "description": "Whether to respect .gitignore rules (default: true)",
                    "default": true
                }
            },
            "required": ["pattern"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Read
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        // Check cancellation before starting
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation cancelled", "cancelled");
            }
        }

        let pattern = args["pattern"].as_str().unwrap_or("");
        if pattern.is_empty() {
            return ToolResult::error("pattern is required", "pattern is required");
        }

        let search_path = match args["path"].as_str() {
            Some(p) => match crate::core::paths::resolve_workspace_path(p, &self.workspace) {
                Ok(resolved) => resolved,
                Err(_) => {
                    return ToolResult::error(
                        "Path must be inside the workspace",
                        "path_outside_workspace",
                    );
                }
            },
            None => self.workspace.clone(),
        };

        let max_results = args["max_results"]
            .as_i64()
            .map(|v| v.max(1).min(1000) as usize)
            .unwrap_or(200);

        let respect_gitignore = args["respect_gitignore"].as_bool().unwrap_or(true);

        // Compile glob pattern
        let glob_pattern = match glob::Pattern::new(pattern) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(
                    format!("Invalid glob pattern: {}", e),
                    format!("{}", e),
                );
            }
        };

        // Walk filesystem with gitignore support
        let mut walker = ignore::WalkBuilder::new(&search_path);
        walker
            .hidden(false)
            .git_ignore(respect_gitignore)
            .ignore(respect_gitignore)
            .git_global(respect_gitignore);

        let mut results: Vec<String> = Vec::new();

        for entry in walker.build().flatten() {
            // Check cancellation periodically
            if let Some(token) = &self.cancellation {
                if token.is_cancelled() {
                    return ToolResult::error("Operation cancelled", "cancelled");
                }
            }

            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }

            // Get relative path from search root
            let rel_path = match entry.path().strip_prefix(&search_path) {
                Ok(p) => p,
                Err(_) => continue,
            };

            // Match relative path against glob pattern
            if !glob_pattern.matches(&rel_path.to_string_lossy()) {
                continue;
            }

            results.push(rel_path.to_string_lossy().to_string());

            if results.len() >= max_results {
                break;
            }
        }

        // Sort results by path for stability
        results.sort();

        if results.is_empty() {
            ToolResult::success("No files found")
        } else {
            let truncated = results.len() >= max_results;
            let output = if truncated {
                format!(
                    "{}\n\n[Showing first {} matches. Increase max_results to inspect more.]",
                    results.join("\n"),
                    max_results
                )
            } else {
                results.join("\n")
            };
            ToolResult::success(output)
        }
    }
}

// ============================================================================
// Grep — 内容搜索
// ============================================================================

pub struct GrepTool {
    workspace: PathBuf,
    cancellation: Option<CancellationToken>,
}

impl GrepTool {
    pub fn new(workspace: PathBuf, cancellation: Option<CancellationToken>) -> Self {
        Self { workspace, cancellation }
    }
}

/// Map a file type name to a glob pattern for filtering
fn file_type_to_glob(file_type: &str) -> Option<&'static str> {
    match file_type.to_lowercase().as_str() {
        "rust" | "rs" => Some("*.rs"),
        "python" | "py" => Some("*.py"),
        "javascript" | "js" => Some("*.js"),
        "typescript" | "ts" => Some("*.ts"),
        "typescriptreact" | "tsx" => Some("*.tsx"),
        "javascriptreact" | "jsx" => Some("*.jsx"),
        "go" => Some("*.go"),
        "java" => Some("*.java"),
        "c" | "c-source" => Some("*.c"),
        "cpp" | "c++" | "cc" => Some("*.cpp"),
        "cxx" => Some("*.cxx"),
        "h" | "header" => Some("*.h"),
        "hpp" => Some("*.hpp"),
        "csharp" | "cs" => Some("*.cs"),
        "ruby" | "rb" => Some("*.rb"),
        "php" => Some("*.php"),
        "swift" => Some("*.swift"),
        "kotlin" | "kt" => Some("*.kt"),
        "scala" | "sc" => Some("*.scala"),
        "shell" | "bash" | "sh" => Some("*.sh"),
        "markdown" | "md" => Some("*.md"),
        "json" => Some("*.json"),
        "yaml" | "yml" => Some("*.yml"),
        "toml" => Some("*.toml"),
        "html" => Some("*.html"),
        "css" => Some("*.css"),
        "sql" => Some("*.sql"),
        "dockerfile" => Some("Dockerfile"),
        "makefile" => Some("Makefile"),
        "gradle" => Some("*.gradle"),
        "cmake" => Some("CMakeLists.txt"),
        _ => None,
    }
}

/// Owned parameters for a synchronous grep scan, suitable for being moved
/// into a `spawn_blocking` task. All fields are owned (`Send` + `'static`).
struct GrepRequest {
    search_path: PathBuf,
    pattern: String,
    include: Option<String>,
    before_context: usize,
    after_context: usize,
    max_results: usize,
    file_type: Option<String>,
    cancellation: Option<CancellationToken>,
}

/// Result of a synchronous grep scan.
struct GrepOutput {
    /// Formatted `path:line: content` entries, including context lines.
    results: Vec<String>,
    /// Number of matched lines (excluding context lines).
    match_count: usize,
    /// True only when an `(max_results + 1)`-th match was observed.
    truncated: bool,
}

/// Synchronous grep implementation: walks the file tree, reads files, applies
/// the regex, and merges context intervals. Intended to run inside
/// `tokio::task::spawn_blocking` so blocking I/O never stalls the async
/// runtime. Checks `cancellation` between files; returns `Err("cancelled")`
/// when the token is set (a sync context can only poll `is_cancelled`).
fn search_files_blocking(request: GrepRequest) -> Result<GrepOutput, String> {
    let re = match regex::Regex::new(&request.pattern) {
        Ok(r) => r,
        Err(e) => return Err(format!("Invalid regex: {}", e)),
    };

    // Resolve file_type → glob pattern once (not per file).
    let file_type_glob = request.file_type.as_deref().and_then(file_type_to_glob);

    let mut results: Vec<String> = Vec::new();
    let mut matched_lines: usize = 0;
    let mut has_more_match = false;

    let walker = ignore::WalkBuilder::new(&request.search_path)
        .hidden(false)
        .git_ignore(true)
        .build();

    'outer: for entry in walker.flatten() {
        // Check cancellation periodically.
        if let Some(token) = &request.cancellation {
            if token.is_cancelled() {
                return Err("cancelled".to_string());
            }
        }

        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
            continue;
        }

        let path = entry.path();

        // Filter by include pattern.
        if let Some(inc) = &request.include {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !glob::Pattern::new(inc.as_str()).map_or(false, |p| p.matches(name)) {
                    continue;
                }
            }
        }

        // Filter by file_type.
        if let Some(ft_glob) = &file_type_glob {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !glob::Pattern::new(ft_glob).map_or(false, |p| p.matches(name)) {
                    continue;
                }
            }
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let lines: Vec<&str> = content.lines().collect();

        // Collect context intervals for accepted matches in this file, then
        // merge overlapping/adjacent ranges before emitting lines. Context
        // lines never consume the `max_results` budget.
        let mut intervals: Vec<(usize, usize)> = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            // Check cancellation per line so very large files remain responsive.
            if let Some(token) = &request.cancellation {
                if token.is_cancelled() {
                    return Err("cancelled".to_string());
                }
            }

            if !re.is_match(line) {
                continue;
            }

            // Already at the match budget: this is the (N+1)-th match. Record
            // that more matches exist and stop scanning this file. The line is
            // intentionally NOT emitted, but already-collected intervals for
            // this file must still be merged/emitted below, so we break the
            // inner loop only — not the outer file loop.
            if matched_lines >= request.max_results {
                has_more_match = true;
                break;
            }

            matched_lines += 1;
            let context_start = line_num.saturating_sub(request.before_context);
            let context_end = std::cmp::min(line_num + request.after_context + 1, lines.len());
            intervals.push((context_start, context_end));
        }

        if !intervals.is_empty() {
            // Sort by start so overlaps/adjacencies can be merged in one pass.
            intervals.sort_by_key(|(start, _)| *start);
            let mut merged: Vec<(usize, usize)> = Vec::new();
            for (start, end) in intervals {
                if let Some((_, last_end)) = merged.last_mut() {
                    if start <= *last_end {
                        if end > *last_end {
                            *last_end = end;
                        }
                        continue;
                    }
                }
                merged.push((start, end));
            }

            for (start, end) in merged {
                for i in start..end {
                    results.push(format!("{}:{}: {}", path.display(), i + 1, lines[i].trim()));
                }
            }
        }

        // Hit the (N+1)-th match in this file: stop scanning further files now
        // that this file's accepted matches have been emitted.
        if has_more_match {
            break 'outer;
        }
    }

    Ok(GrepOutput {
        results,
        match_count: matched_lines,
        truncated: has_more_match,
    })
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

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
                },
                "before_context": {
                    "type": "integer",
                    "description": "Number of context lines to show before each match (default: 0)",
                    "default": 0
                },
                "after_context": {
                    "type": "integer",
                    "description": "Number of context lines to show after each match (default: 0)",
                    "default": 0
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches to return (default: 100, max: 500)",
                    "default": 100
                },
                "file_type": {
                    "type": "string",
                    "description": "File type filter (e.g., 'rust', 'python', 'typescript', 'go', 'java')"
                }
            },
            "required": ["pattern"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Read
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        // Check cancellation before starting.
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation cancelled", "cancelled");
            }
        }

        let pattern = args["pattern"].as_str().unwrap_or("").to_string();
        let search_path = match args["path"].as_str() {
            Some(p) => match crate::core::paths::resolve_workspace_path(p, &self.workspace) {
                Ok(resolved) => resolved,
                Err(_) => {
                    return ToolResult::error(
                        "Path must be inside the workspace",
                        "path_outside_workspace",
                    );
                }
            },
            None => self.workspace.clone(),
        };

        let include = args["include"].as_str().map(|s| s.to_string());
        let before_context = args["before_context"].as_i64().unwrap_or(0).max(0) as usize;
        let after_context = args["after_context"].as_i64().unwrap_or(0).max(0) as usize;
        let max_results = args["max_results"]
            .as_i64()
            .map(|v| v.max(1).min(500) as usize)
            .unwrap_or(100);
        let file_type = args["file_type"].as_str().map(|s| s.to_string());

        // All blocking work (tree walk, read_to_string, regex, context merge)
        // runs in a spawn_blocking task so the async runtime is never stalled.
        let request = GrepRequest {
            search_path,
            pattern,
            include,
            before_context,
            after_context,
            max_results,
            file_type,
            cancellation: self.cancellation.clone(),
        };

        let output = match tokio::task::spawn_blocking(move || search_files_blocking(request))
            .await
        {
            Ok(Ok(o)) => o,
            Ok(Err(e)) if e == "cancelled" => {
                return ToolResult::error("Operation cancelled", "cancelled");
            }
            Ok(Err(e)) => return ToolResult::error(e, "grep_error"),
            Err(join_err) => return ToolResult::error(join_err.to_string(), "grep_join_error"),
        };

        let GrepOutput {
            results,
            match_count,
            truncated,
        } = output;

        if results.is_empty() {
            ToolResult::success("No matches found").with_metadata(json!({
                "match_count": match_count,
                "truncated": truncated,
            }))
        } else {
            let output = if truncated {
                format!(
                    "{}\n\n[Showing first {} matches. Increase max_results to inspect more.]",
                    results.join("\n"),
                    max_results
                )
            } else {
                results.join("\n")
            };
            ToolResult::success(output).with_metadata(json!({
                "match_count": match_count,
                "truncated": truncated,
            }))
        }
    }
}

// ============================================================================
// Git — Git 操作
// ============================================================================

/// Read-only git subcommands whitelist
const READONLY_GIT_SUBCOMMANDS: &[&str] = &[
    "status",
    "diff",
    "log",
    "show",
    "blame",
    "describe",
    "ls-files",
    "ls-tree",
    "config",
    "remote",
    "stash",
    "tag",
    "branch",
    "help",
    "version",
];

/// Classify a git command string into a `ToolCategory`.
///
/// Parses the first word of the command string as the subcommand.
/// Read-only subcommands return `Read`; everything else returns `Write`.
/// Subcommands with known destructive flags (e.g. `reset --hard`, `branch -D`)
/// return `Destructive`.
pub fn git_subcommand_category(command: &str) -> ToolCategory {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return ToolCategory::Write;
    }

    // Use shlex to properly parse the first subcommand
    let args = match shlex::split(trimmed) {
        Some(a) => a,
        None => return ToolCategory::Write,
    };

    let subcmd = match args.first() {
        Some(s) => s.as_str(),
        None => return ToolCategory::Write,
    };

    // Check for destructive patterns first
    if subcmd == "reset" && args.len() > 1 {
        // reset --hard is destructive
        if args.iter().any(|a| a == "--hard") {
            return ToolCategory::Destructive;
        }
    }
    if subcmd == "clean" {
        return ToolCategory::Destructive;
    }
    if subcmd == "push" {
        // force push is destructive
        if args.iter().any(|a| a == "--force" || a == "-f") {
            return ToolCategory::Destructive;
        }
        return ToolCategory::Write;
    }
    if subcmd == "branch" && args.len() > 1 {
        // branch -D or --delete --force
        if args.iter().any(|a| a == "-D" || a == "--delete") {
            return ToolCategory::Destructive;
        }
        return ToolCategory::Write; // branch create/rename
    }
    if subcmd == "rm" {
        return ToolCategory::Destructive;
    }
    if subcmd == "gc" || subcmd == "prune" || subcmd == "filter-branch" {
        return ToolCategory::Destructive;
    }
    if subcmd == "update-ref" && args.iter().any(|a| a == "-d") {
        return ToolCategory::Destructive;
    }

    // Check read-only whitelist
    if READONLY_GIT_SUBCOMMANDS.contains(&subcmd) {
        // Some subcommands have variations: need deeper check
        match subcmd {
            "stash" => {
                // stash list/show = read; stash push/pop/apply/drop = write
                if args.len() > 1 && args[1] != "list" && args[1] != "show" {
                    return ToolCategory::Write;
                }
                ToolCategory::Read
            }
            "tag" => {
                // tag -l/list = read; tag creation = write
                if args.len() > 1 && args[1] != "-l" && args[1] != "--list" {
                    return ToolCategory::Write;
                }
                ToolCategory::Read
            }
            "branch" => {
                // branch (no args) or branch --list = read; branch -d = write
                if args.len() == 1 || args[1] == "--list" {
                    return ToolCategory::Read;
                }
                ToolCategory::Write
            }
            "config" => {
                // config --list or config <key> = read; config <key> <value> = write
                if args.len() <= 2 {
                    return ToolCategory::Read;
                }
                ToolCategory::Write
            }
            "remote" => {
                // remote -v/remote show = read; remote add/remove = write
                if args.len() == 1 || args[1] == "-v" || args[1] == "--verbose" || args[1] == "show" {
                    return ToolCategory::Read;
                }
                ToolCategory::Write
            }
            _ => ToolCategory::Read,
        }
    } else {
        // Not in read-only whitelist
        ToolCategory::Write
    }
}

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
    fn name(&self) -> &str {
        "git"
    }

    fn description(&self) -> &str {
        "Run git commands in the workspace. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Git subcommand to run (e.g., 'status', 'diff', 'log', 'commit -m \"message\"')"
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> ToolCategory {
        // Default category for unknown command; the react loop uses
        // `git_subcommand_category()` for dynamic classification.
        ToolCategory::Write
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let command = args["command"].as_str().unwrap_or("");
        if command.trim().is_empty() {
            return ToolResult::error("command is required", "command is required");
        }

        // Parse arguments with shlex for proper quoted-string support
        let split_args = match shlex::split(command) {
            Some(a) if !a.is_empty() => a,
            _ => {
                return ToolResult::error(
                    format!("Failed to parse git command: {}", command),
                    "parse_error",
                );
            }
        };

        // Build the full git command line
        let mut cmd = tokio::process::Command::new("git");
        cmd.current_dir(&self.workspace)
            .args(&split_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Spawn the child process
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return ToolResult::error(
                    format!("Failed to spawn git: {}", e),
                    "spawn_error",
                );
            }
        };

        // Take stdout/stderr handles to read async
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut s) = stdout_handle {
                let _ = tokio::io::AsyncReadExt::read_to_end(&mut s, &mut buf).await;
            }
            buf
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut s) = stderr_handle {
                let _ = tokio::io::AsyncReadExt::read_to_end(&mut s, &mut buf).await;
            }
            buf
        });

        // Wait with 30s timeout
        let timeout_dur = Duration::from_secs(30);
        let status = match tokio::time::timeout(timeout_dur, child.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return ToolResult::error(
                    format!("Git command wait failed: {}", e),
                    "spawn_error",
                );
            }
            Err(_) => {
                // Timeout: kill + reap
                let _ = child.kill().await;
                let _ = child.wait().await;
                return ToolResult::error(
                    format!("Git command timed out after 30s: {}", command),
                    "timeout",
                );
            }
        };

        // Collect output
        let stdout_buf = stdout_task.await.unwrap_or_default();
        let stderr_buf = stderr_task.await.unwrap_or_default();
        let stdout = String::from_utf8_lossy(&stdout_buf).to_string();
        let stderr = String::from_utf8_lossy(&stderr_buf).to_string();
        let exit_code = status.code().unwrap_or(-1);
        let stdout_len = stdout.len();
        let stderr_len = stderr.len();

        let result = if !stdout.is_empty() {
            stdout
        } else if !stderr.is_empty() {
            stderr
        } else {
            "(no output)".to_string()
        };

        let metadata = json!({
            "exit_code": exit_code,
            "stdout_len": stdout_len,
            "stderr_len": stderr_len,
        });

        if exit_code == 0 {
            ToolResult::success(result).with_metadata(metadata)
        } else {
            let output_with_status = format!(
                "{}\n\n[Git command exited with code {}]",
                result, exit_code
            );
            ToolResult::error(output_with_status, "nonzero_exit").with_metadata(metadata)
        }
    }
}
