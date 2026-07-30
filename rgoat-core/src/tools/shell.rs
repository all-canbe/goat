//! Shell 工具 — 执行系统命令（含超时控制 + 取消 + 输出截断 + 退出码处理）
//!
//! 从 builtin.rs 拆分而来。阶段2增强：
//! - 使用 tokio::process 异步子进程，超时后 kill 子进程并 reap（P0）
//! - 输出截断（保留尾部）+ 写临时文件（P0）
//! - 退出码非零明确返回 error
//! - 等待期间响应取消信号（select! 同时监听 cancellation + timeout + child.wait）

use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

use super::registry::{Tool, ToolExecutionContext, ToolResult, ToolStreamEvent};
use crate::core::cancellation::CancellationToken;
use crate::security::approval::ToolCategory;

/// 最大保留行数（超出则截断保留尾部）
const MAX_LINES: usize = 2000;
/// 最大保留字节数（超出则截断保留尾部）
const MAX_BYTES: usize = 512 * 1024;
/// 取消轮询间隔（CancellationToken 无 async wait，必须轮询）
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct ShellTool {
    workspace: PathBuf,
    cancellation: Option<CancellationToken>,
}

impl ShellTool {
    pub fn new(workspace: PathBuf, cancellation: Option<CancellationToken>) -> Self {
        Self {
            workspace,
            cancellation,
        }
    }
}

/// 轮询取消令牌：已取消则返回；无 token 则永不完成
async fn poll_cancellation(token: Option<CancellationToken>) {
    let Some(token) = token else {
        // 无 token：永不取消，挂起等待（loop 让 future 永不 ready）
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    };
    loop {
        if token.is_cancelled() {
            return;
        }
        tokio::time::sleep(CANCEL_POLL_INTERVAL).await;
    }
}

/// 等待任一取消令牌触发：ctx（每次调用级）或 tool（工具级）。
/// None 的令牌分支永不完成（poll_cancellation 内部挂起），因此只有真实
/// 被取消的令牌才会让此 future 就绪。
async fn wait_for_cancellation(
    context: Option<CancellationToken>,
    tool: Option<CancellationToken>,
) {
    tokio::select! {
        _ = poll_cancellation(context) => {}
        _ = poll_cancellation(tool) => {}
    }
}

/// 截断后的输出
struct TruncatedOutput {
    /// 显示给 agent 的内容（含截断提示）
    display: String,
    /// 总行数
    total_lines: usize,
    /// 是否截断
    truncated: bool,
    /// 全输出临时文件路径（仅截断时有）
    full_output_path: Option<String>,
}

/// 流式节流间隔（100ms）
const STREAM_THROTTLE_INTERVAL: Duration = Duration::from_millis(100);

/// 合并 stdout/stderr 并按需截断
///
/// 截断策略：保留尾部（最近输出最重要）。先按行截断到 MAX_LINES，再按字节截断到 MAX_BYTES
/// （对齐到行首避免截断半个字符）。截断时全量写入临时文件。
fn build_output(stdout: &str, stderr: &str) -> TruncatedOutput {
    let mut combined = String::new();
    if !stdout.is_empty() {
        combined.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str("[stderr]\n");
        combined.push_str(stderr);
    }
    if combined.is_empty() {
        combined = "(no output)".to_string();
    }

    let total_lines = combined.lines().count();
    let total_bytes = combined.len();

    let need_truncate_lines = total_lines > MAX_LINES;
    let need_truncate_bytes = total_bytes > MAX_BYTES;

    if !need_truncate_lines && !need_truncate_bytes {
        return TruncatedOutput {
            display: combined,
            total_lines,
            truncated: false,
            full_output_path: None,
        };
    }

    // 写入临时文件（全量输出）
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    let temp_path = std::env::temp_dir().join(format!("rgoat-shell-{}-{}.log", pid, timestamp));
    let full_output_path = temp_path.to_string_lossy().to_string();
    let _ = std::fs::write(&temp_path, &combined);

    // 截断保留尾部
    let mut shown: String = if need_truncate_lines {
        let lines: Vec<&str> = combined.lines().collect();
        let start = lines.len() - MAX_LINES;
        lines[start..].join("\n")
    } else {
        combined.clone()
    };

    // 按字节截断（对齐到行首避免半行）
    if shown.len() > MAX_BYTES {
        let start = shown.len() - MAX_BYTES;
        let aligned_start = shown[start..]
            .find('\n')
            .map(|i| start + i + 1)
            .unwrap_or(start);
        shown = shown[aligned_start..].to_string();
    }

    let shown_lines = shown.lines().count();
    let display = format!(
        "{}\n\n[Showing last {} lines of {}. Full output: {}]",
        shown, shown_lines, total_lines, full_output_path
    );

    TruncatedOutput {
        display,
        total_lines,
        truncated: true,
        full_output_path: Some(full_output_path),
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the workspace directory. \
         Output is truncated to last 2000 lines or 512KB if exceeded \
         (full output saved to a temp file). Returns error with code \
         'nonzero_exit' when exit code != 0, 'timeout' when killed after \
         timeout_ms, or 'cancelled' when aborted. "
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
                    "description": "Timeout in milliseconds (default: 30000). On timeout the child process is killed.",
                    "default": 30000
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let command = args["command"].as_str().unwrap_or("").to_string();
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(30000);

        // 启动命令前检查取消
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation aborted", "cancelled");
            }
        }

        let (shell, shell_arg) = shell_cmd();
        let mut child = match spawn_child(&shell, shell_arg, &command, &self.workspace) {
            Ok(c) => c,
            Err(e) => return e,
        };

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

        let timeout_dur = Duration::from_millis(timeout_ms);

        let status = tokio::select! {
            biased;
            _ = poll_cancellation(self.cancellation.clone()) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return ToolResult::error("Operation aborted while running", "cancelled");
            }
            r = tokio::time::timeout(timeout_dur, child.wait()) => match r {
                Ok(Ok(status)) => status,
                Ok(Err(e)) => {
                    return ToolResult::error(
                        format!("Command wait failed: {}", e), "spawn_error",
                    )
                }
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return ToolResult::error(
                        format!("Shell command timed out after {}ms", timeout_ms), "timeout",
                    );
                }
            },
        };

        let stdout_buf = stdout_task.await.unwrap_or_default();
        let stderr_buf = stderr_task.await.unwrap_or_default();
        let stdout = String::from_utf8_lossy(&stdout_buf).to_string();
        let stderr = String::from_utf8_lossy(&stderr_buf).to_string();
        let exit_code = status.code().unwrap_or(-1);

        let truncated = build_output(&stdout, &stderr);
        let mut metadata = json!({
            "exit_code": exit_code,
            "stdout_len": stdout.len(),
            "stderr_len": stderr.len(),
            "truncated": truncated.truncated,
            "total_lines": truncated.total_lines,
        });
        if let Some(path) = &truncated.full_output_path {
            metadata["full_output"] = json!(path);
        }

        if exit_code == 0 {
            ToolResult::success(truncated.display).with_metadata(metadata)
        } else {
            let output_with_status = format!(
                "{}\n\n[Command exited with code {}]",
                truncated.display, exit_code
            );
            ToolResult::error(output_with_status, "nonzero_exit").with_metadata(metadata)
        }
    }

    // P2-1: 流式执行上下文 — 带流式增量输出
    // 使用 tokio::io::BufReader 逐行读取 stdout/stderr，以 100ms 节流发送
    // ToolStreamEvent。最终结果与 execute() 一致。
    async fn execute_ctx(&self, args: serde_json::Value, ctx: &ToolExecutionContext) -> ToolResult {
        self.execute_streaming(args, ctx).await
    }
}

// P2-1: ShellTool 的流式执行实现
impl ShellTool {
    /// 流式执行入口（由 Tool::execute_ctx 调用）
    pub async fn execute_streaming(
        &self,
        args: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> ToolResult {
        let command = args["command"].as_str().unwrap_or("").to_string();
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(30000);

        // 启动命令前检查取消（ctx 与 tool 两级）
        if let Some(token) = &ctx.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation aborted", "cancelled");
            }
        }
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation aborted", "cancelled");
            }
        }

        let (shell, shell_arg) = shell_cmd();
        let mut child = match spawn_child(&shell, shell_arg, &command, &self.workspace) {
            Ok(c) => c,
            Err(e) => return e,
        };

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // ── 流式读取 stdout/stderr 带 100ms 节流 ──
        let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::channel::<String>(256);
        let (stderr_tx, mut stderr_rx) = tokio::sync::mpsc::channel::<String>(256);

        let stdout_task = tokio::spawn(async move {
            if let Some(handle) = stdout_handle {
                let reader = BufReader::new(handle);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if stdout_tx.send(line).await.is_err() {
                        break;
                    }
                }
            }
        });
        let stderr_task = tokio::spawn(async move {
            if let Some(handle) = stderr_handle {
                let reader = BufReader::new(handle);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if stderr_tx.send(line).await.is_err() {
                        break;
                    }
                }
            }
        });

        // 总超时：从 spawn 后固定一个绝对 deadline，而非每次输出后重新
        // 计时（空闲超时）。持续输出不再能阻止 deadline 触发。
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        let mut stdout_lines: Vec<String> = Vec::new();
        let mut stderr_lines: Vec<String> = Vec::new();
        let mut last_stream_time = tokio::time::Instant::now();
        let has_updates = ctx.has_updates();
        // 流事件必须使用模型返回的真实 tool_call_id，不得伪造共享 ID。
        let tool_call_id = ctx.tool_call_id.clone();

        // 子进程退出 + 输出收集的循环
        let exit_code = loop {
            tokio::select! {
                biased;

                // 取消：ctx（每次调用级）与 tool（工具级）任一触发即终止
                _ = wait_for_cancellation(ctx.cancellation.clone(), self.cancellation.clone()) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return ToolResult::error("Operation aborted while running", "cancelled");
                }

                // 总超时：到 deadline 即 kill，与输出节奏无关
                // 必须在 stdout/stderr 之前，biased 模式下确保持续输出无法饥饿超时
                _ = tokio::time::sleep_until(deadline) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return ToolResult::error(
                        format!("Shell command timed out after {}ms", timeout_ms), "timeout",
                    );
                }

                // 从 stdout 通道读取一行
                Some(line) = stdout_rx.recv() => {
                    stdout_lines.push(line.clone());
                    // 节流发送流更新
                    if has_updates && last_stream_time.elapsed() >= STREAM_THROTTLE_INTERVAL {
                        let content = stdout_lines.last().map(|s| s.as_str()).unwrap_or(&line);
                        ctx.send_update(ToolStreamEvent {
                            tool_call_id: tool_call_id.clone(),
                            content: content.to_string(),
                            metadata: Some(json!({"stream": "stdout"})),
                        });
                        last_stream_time = tokio::time::Instant::now();
                    }
                }

                // 从 stderr 通道读取一行
                Some(line) = stderr_rx.recv() => {
                    stderr_lines.push(line.clone());
                    if has_updates && last_stream_time.elapsed() >= STREAM_THROTTLE_INTERVAL {
                        ctx.send_update(ToolStreamEvent {
                            tool_call_id: tool_call_id.clone(),
                            content: format!("[stderr] {}", line),
                            metadata: Some(json!({"stream": "stderr"})),
                        });
                        last_stream_time = tokio::time::Instant::now();
                    }
                }

                // 子进程退出
                status = child.wait() => {
                    break status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
                }
            }
        };

        // 子进程退出后，先 await reader 任务确保所有行已写入 channel
        // （避免与 child.wait() 的竞争），再 drain channel 至关闭。
        let _ = stdout_task.await;
        let _ = stderr_task.await;
        while let Some(line) = stdout_rx.recv().await {
            stdout_lines.push(line);
        }
        while let Some(line) = stderr_rx.recv().await {
            stderr_lines.push(line);
        }

        let stdout = stdout_lines.join("\n");
        let stderr = stderr_lines.join("\n");

        let truncated = build_output(&stdout, &stderr);
        let mut metadata = json!({
            "exit_code": exit_code,
            "stdout_len": stdout.len(),
            "stderr_len": stderr.len(),
            "truncated": truncated.truncated,
            "total_lines": truncated.total_lines,
        });
        if let Some(path) = &truncated.full_output_path {
            metadata["full_output"] = json!(path);
        }

        if exit_code == 0 {
            ToolResult::success(truncated.display).with_metadata(metadata)
        } else {
            let output_with_status = format!(
                "{}\n\n[Command exited with code {}]",
                truncated.display, exit_code
            );
            ToolResult::error(output_with_status, "nonzero_exit").with_metadata(metadata)
        }
    }
}

/// 获取 shell 命令名称和参数
fn shell_cmd() -> (&'static str, &'static str) {
    #[cfg(windows)]
    return ("cmd", "/C");
    #[cfg(not(windows))]
    return ("sh", "-c");
}

/// 生成子进程
fn spawn_child(
    shell: &str,
    shell_arg: &str,
    command: &str,
    workspace: &PathBuf,
) -> Result<tokio::process::Child, ToolResult> {
    match tokio::process::Command::new(shell)
        .arg(shell_arg)
        .arg(command)
        .current_dir(workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => Ok(c),
        Err(e) => Err(ToolResult::error(
            format!("Failed to spawn command: {}", e),
            "spawn_error",
        )),
    }
}
