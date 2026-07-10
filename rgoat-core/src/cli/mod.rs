//! CLI 输入处理
//!
//! 处理命令行参数、交互式命令、模式切换等。
//! 注意：实际的 UI 渲染由 rgoat-tui / rgoat-desktop 负责，这里只提供核心解析。

use std::path::PathBuf;

/// CLI 命令
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    /// 运行单次 Agent 会话
    Run { prompt: String, mode: String },
    /// 启动交互式 TUI
    Tui,
    /// 启动 MCP 服务器
    McpServer,
    /// 索引代码
    Index { path: PathBuf },
    /// 列出会话
    ListSessions,
    /// 打开指定会话
    ResumeSession { session_id: String },
    /// 显示帮助
    Help,
    /// 显示版本
    Version,
}

/// 解析命令行参数
pub fn parse_args(args: &[String]) -> CliCommand {
    if args.is_empty() {
        return CliCommand::Tui;
    }

    match args[0].as_str() {
        "--version" | "-v" => CliCommand::Version,
        "--help" | "-h" => CliCommand::Help,
        "mcp" => CliCommand::McpServer,
        "tui" => CliCommand::Tui,
        "index" => {
            if args.len() < 2 {
                CliCommand::Run {
                    prompt: "index".to_string(),
                    mode: "agent".to_string(),
                }
            } else {
                CliCommand::Index {
                    path: PathBuf::from(&args[1]),
                }
            }
        }
        "list" | "sessions" => CliCommand::ListSessions,
        "resume" => {
            if args.len() < 2 {
                CliCommand::Help
            } else {
                CliCommand::ResumeSession {
                    session_id: args[1].clone(),
                }
            }
        }
        "run" => {
            let mode = args.iter().position(|a| a == "--mode").and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| "agent".to_string());
            let prompt_parts: Vec<String> = args.iter()
                .skip(1)
                .filter(|a| !a.starts_with("--"))
                .cloned()
                .collect();
            CliCommand::Run {
                prompt: prompt_parts.join(" "),
                mode,
            }
        }
        _ => {
            // 默认：把全部参数当作 prompt
            CliCommand::Run {
                prompt: args.join(" "),
                mode: "agent".to_string(),
            }
        }
    }
}

/// 交互式命令解析（用户在 TUI 中输入的斜杠命令）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractiveCommand {
    /// /mode agent|plan|flow|yolo
    Mode(String),
    /// /clear
    Clear,
    /// /save <filename>
    Save(String),
    /// /index
    IndexCode,
    /// /exit
    Exit,
    /// /help — 显示帮助
    Help,
    /// /model <name> — 切换模型
    Model(String),
    /// /compact — 压缩对话上下文
    Compact,
    /// /resume <session_id> — 恢复会话
    Resume(String),
    /// /plan — 切换到 Plan 模式
    Plan,
    /// 普通消息
    Message(String),
}

pub fn parse_interactive(input: &str) -> InteractiveCommand {
    let trimmed = input.trim();
    if trimmed.starts_with('/') {
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        match parts[0] {
            "/mode" => {
                if parts.len() > 1 {
                    InteractiveCommand::Mode(parts[1].to_string())
                } else {
                    InteractiveCommand::Message(trimmed.to_string())
                }
            }
            "/clear" => InteractiveCommand::Clear,
            "/save" => {
                if parts.len() > 1 {
                    InteractiveCommand::Save(parts[1].to_string())
                } else {
                    InteractiveCommand::Message(trimmed.to_string())
                }
            }
            "/index" | "/index-code" => InteractiveCommand::IndexCode,
            "/exit" | "/quit" => InteractiveCommand::Exit,
            "/help" => InteractiveCommand::Help,
            "/model" => {
                if parts.len() > 1 {
                    InteractiveCommand::Model(parts[1..].join(" "))
                } else {
                    InteractiveCommand::Message(trimmed.to_string())
                }
            }
            "/compact" => InteractiveCommand::Compact,
            "/resume" => {
                if parts.len() > 1 {
                    InteractiveCommand::Resume(parts[1].to_string())
                } else {
                    InteractiveCommand::Message(trimmed.to_string())
                }
            }
            "/plan" => InteractiveCommand::Plan,
            _ => InteractiveCommand::Message(trimmed.to_string()),
        }
    } else {
        InteractiveCommand::Message(trimmed.to_string())
    }
}

/// 生成帮助文本
pub fn help_text() -> String {
    r#"RGoat — lightweight AI coding assistant

USAGE:
    rgoat [COMMAND] [ARGS]

COMMANDS:
    rgoat "<prompt>"          Run a single agent task
    rgoat run "<prompt>"      Same as above
    rgoat run "<prompt>" --mode flow    Run in flow mode
    rgoat tui                 Launch interactive TUI
    rgoat index <path>        Index code at path
    rgoat mcp                 Start MCP stdio server
    rgoat list                List sessions
    rgoat resume <session_id> Resume a session
    rgoat --version           Show version
    rgoat --help              Show this help

INTERACTIVE COMMANDS (inside TUI):
    /mode agent|plan|flow|yolo   Switch mode
    /index                       Index current workspace code
    /save <file>                 Save conversation to file
    /clear                       Clear current conversation
    /help                        Show available commands
    /model <name>                Switch to model
    /compact                     Compress conversation context
    /resume <id>                 Resume a previous session
    /plan                        Switch to plan mode
    /exit                        Exit
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args() {
        assert!(matches!(parse_args(&["--version".to_string()]), CliCommand::Version));
        assert!(matches!(
            parse_args(&["run".to_string(), "hello".to_string()]),
            CliCommand::Run { prompt, mode } if prompt == "hello" && mode == "agent"
        ));
    }

    #[test]
    fn test_parse_interactive() {
        assert!(matches!(parse_interactive("/mode flow"), InteractiveCommand::Mode(m) if m == "flow"));
        assert!(matches!(parse_interactive("hello"), InteractiveCommand::Message(m) if m == "hello"));
    }
}
