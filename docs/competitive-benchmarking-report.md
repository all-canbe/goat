# goat_rust 竞品对标分析报告

> 对标基准：OpenCode（开源 MIT） & Claude Code（Anthropic 闭源）
> 调研日期：2026-07-07
> 产品定位：goat_rust = Rust 重写的本地优先 AI Coding Agent（目标：与 OpenCode/Claude Code 功能对齐）

---

## 一、竞品概述

### 1.1 OpenCode

OpenCode 是 SST 团队开发的开源终端 AI 编程 Agent（GitHub 95,000+ stars，MIT 协议），使用 **Go** 语言编写。核心理念是**无厂商锁定、本地模型友好、多供应商自由**。它基于 Vercel AI SDK 实现了对 75+ LLM 提供商的统一抽象，提供一套完整的 TUI（终端用户界面），支持 LSP 集成、Docker 沙箱隔离，以及丰富的插件和自定义 Agent 体系。

**核心优势**：多模型自由度（75+ providers）、本地模型支持（Ollama/LM Studio）、LSP 代码智能感知、开源社区驱动、TUI 多窗格界面。

**主要弱点**：Agentic 任务成功率比 Claude Code 低约 8 个百分点、OAuth 被 Anthropic 封禁（需直接 API key）、MCP 生态较新、TUI 学习曲线。

### 1.2 Claude Code

Claude Code 是 Anthropic 的官方终端编程 Agent（闭源），基于 **Node.js**。其核心优势在于与 Claude 模型的深度集成（专有优化使 Agentic 任务成功率高约 8%），拥有成熟的子 Agent 编排（10-100 并行子代理）、6 种权限模式、30 种 Hook 事件、38 个内置工具、完整的 Git/GitHub 工作流集成、以及覆盖 VS Code / JetBrains / Slack 等多平台的生态。

**核心优势**：最强的 Agentic Loop（82% 任务完成率）、成熟的 MCP 生态、丰富的 Hook/插件系统、企业级安全与权限、多平台 IDE 集成。

**主要弱点**：仅支持 Anthropic 模型（锁定风险）、最低 $20/月、不支持本地模型、使用上限（Pro 方案用量受限后需跳至 $100/月 Max）、无 LSP 集成、无 Docker 隔离。

---

## 二、逐项功能对比表（按模块分组）

### 2.1 Agent 核心循环

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| Agent 循环机制 | ReAct（`src/session/prompt.ts`） | ReAct + 多 Agent 编排 | ReAct（`agent/react.rs`）✅ | 已对齐 | — |
| 子 Agent / Task 委托 | TaskTool 创建子 Agent，5 种内置 Agent | Task + Agent 工具，10-100 并行子代理，worktree 隔离 | SubAgentRuntime 10并发/3深度✅ | 部分缺口 | P1 |
| Plan Mode | Plan Agent 内置（禁止编辑，仅写 `.opencode/plans/*.md`） | `/plan` 命令 + EnterPlanMode/ExitPlanMode 工具 | Plan 模式 ✅（但无 enter/exit 工具） | 部分缺口 | P1 |
| Flow/Pipeline | 无内置 Flow | 无内置 Flow | FlowPipeline（实现→审查→修复）✅ | goat_rust 独有 | — |
| 流式输出 | ✅ AI SDK streamText | ✅ 原生流式 | ✅ stream + 非流式回退 | 已对齐 | — |
| Extended Thinking | ❌（依赖模型自身） | ✅ 原生支持（effort 等级） | ❌ | 完全缺失 | P2 |
| 多步骤推理 | ✅ 通过 Agent Loop | ✅ 原生 | ✅ | 已对齐 | — |
| 死循环检测 | ✅ `doom_loop` 检测 | ✅ 分类器审查 | ❌ | 完全缺失 | P1 |

### 2.2 工具系统

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| 文件读取 | ReadTool ✅ | Read ✅ | ReadFileTool ✅ | 已对齐 | — |
| 文件写入 | WriteTool ✅ | Write ✅ | WriteFileTool ✅ | 已对齐 | — |
| 精确编辑 | EditTool ✅ | Edit ✅ | EditFileTool ✅ | 已对齐 | — |
| 文件模式匹配 | GlobTool ✅ | Glob ✅ | GlobTool ✅ | 已对齐 | — |
| 内容搜索 | GrepTool ✅ | Grep ✅ | GrepTool ✅ | 已对齐 | — |
| Shell 命令 | BashTool ✅ | Bash + PowerShell ✅ | ShellTool ✅ | 已对齐 | — |
| Git 操作 | ❌（通过 Bash） | ❌（通过 Bash + 专用命令） | GitTool ✅ | goat_rust 独有 | — |
| Web 搜索 | WebSearchTool ✅ | WebSearch ✅ | ❌ | **完全缺失** | **P0** |
| 网页抓取 | WebFetchTool ✅ | WebFetch ✅ | ❌ | **完全缺失** | **P0** |
| 提问用户 | QuestionTool ✅ | AskUserQuestion ✅ | ❌ | 完全缺失 | P1 |
| Todo 管理 | TodoWriteTool / TodoReadTool ✅ | TodoWrite ✅ | ❌ | 完全缺失 | P1 |
| 子任务创建 | TaskTool ✅ | TaskCreate + TaskUpdate + TaskGet + TaskList ✅ | ❌（仅 SubAgentRuntime 内部） | **完全缺失** | **P0** |
| 代码搜索 | CodeSearchTool ✅ | ❌（通过 Grep） | ❌ | 部分缺口 | P2 |
| 应用补丁 | ApplyPatchTool ✅ | ❌ | ❌ | 部分缺口 | P2 |
| 技能/插件工具 | SkillTool ✅ | Skill ✅ | ❌ | **完全缺失** | **P0** |
| LSP 代码智能 | ❌（通过事件集成） | LSP ✅ | ❌ | 完全缺失 | P1 |
| 通知/推送 | ❌ | PushNotification ✅ | ❌ | 完全缺失 | P2 |
| 定时任务 | ❌ | CronCreate/Delete/List ✅ | ❌ | 完全缺失 | P2 |
| Jupyter Notebook | ❌ | NotebookEdit ✅ | ❌ | 完全缺失 | P2 |
| 后台监控 | ❌ | Monitor ✅ | ❌ | 完全缺失 | P2 |
| MCP 资源 | ❌（通过 MCP 层） | ListMcpResourcesTool / ReadMcpResourceTool ✅ | ❌ | 部分缺口 | P2 |
| **工具总数** | **16 个** | **38 个** | **7 个** | **严重不足** | **P0** |

### 2.3 多模型支持

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| Provider 数量 | 20+ (AI SDK) | 1 (Anthropic only) | 2 (OpenAI-compatible + Anthropic) | 部分缺口 | P1 |
| 本地模型 | ✅ Ollama / LM Studio / llama.cpp | ❌ | ❌ (Candle 计划中但未集成) | 部分缺口 | P1 |
| 运行时切换 | ✅ 键盘快捷键切换 | ❌ | ✅ ProviderSwitch | 已对齐 | — |
| 模型路由 | ✅ 按 Agent 分配模型 | ❌ | ❌ (model_router.rs 文件存在但未在核心循环中使用) | 完全缺失 | P1 |
| GPT 模型 | ✅ | ❌ | ✅ (via OpenAI-compatible) | 已对齐 | — |
| Claude 模型 | ✅ | ✅ | ✅ | 已对齐 | — |
| Gemini 模型 | ✅ | ❌ | ❌ | 完全缺失 | P2 |
| 云厂商 (Bedrock/Vertex) | ✅ | ❌ | ❌ | 完全缺失 | P2 |

### 2.4 MCP 集成

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| MCP Client | ✅ | ✅ | ✅ (client.rs) | 已对齐 | — |
| MCP Server | ✅ | ✅ | ✅ (server.rs) | 已对齐 | — |
| MCP 配置 | `opencode.json` 中 `mcp` 键 | `.mcp.json` | ❌（无自动配置加载） | 完全缺失 | P1 |
| MCP OAuth | ❌ | ✅ | ❌ | 完全缺失 | P2 |
| MCP 工具适配 | ✅ | ✅ | ✅ McpToolAdapter | 已对齐 | — |

### 2.5 会话管理

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| 持久化存储 | `~/.config/opencode/sessions/` | `~/.claude/sessions/` | SQLite ✅ | 已对齐 | — |
| 会话恢复 | ✅ | ✅ `/resume` | ✅（SQL 查询恢复） | 已对齐 | — |
| 上下文压缩 | ✅ 自动/手动 | ✅ `/compact` + PreCompact/PostCompact hooks | ✅ 四层压缩器（compressor.rs） | 已对齐 | — |
| 会话 Fork | ❌ | ✅ | ❌ | 完全缺失 | P2 |
| 会话搜索 | ❌ | ❌ | ✅（ConversationManager 支持） | goat_rust 独有 | — |
| 崩溃恢复 | ⚠️ 不稳定 | ✅ 完整支持 | ❌ | 完全缺失 | P2 |
| 会话导出 | ❌ | ✅ `/export` | ❌ | 完全缺失 | P2 |
| 自动命名 | ❌ | ✅ | ❌ | 完全缺失 | P2 |

### 2.6 项目上下文

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| 项目配置文件 | `AGENTS.md` + `opencode.json` | `CLAUDE.md` + `.claude/settings.json` | ❌ | **完全缺失** | **P0** |
| 分层配置 | ✅ 全局 → 项目 → `.opencode/` | ✅ 全局 → 项目 → `.claude/` | ❌（仅 Settings struct） | **完全缺失** | **P0** |
| 自定义 Agent 定义 | ✅ `opencode.json` 中 `agent` 键 | ✅ `.claude/agents/` | ❌ | 完全缺失 | P1 |
| 规则/Rules 系统 | ✅ AGENTS.md | ✅ CLAUDE.md + `.claude/rules/` | ❌ | **完全缺失** | **P0** |
| `/init` 自动初始化 | ❌ | ✅ 分析代码库生成 CLAUDE.md | ❌ | 完全缺失 | P2 |

### 2.7 权限/审批系统

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| 权限模式 | Agent 粒度（allow/deny/ask） | 6 种全局模式（default/acceptEdits/plan/auto/dontAsk/bypassPermissions） | 5 种模式（Agent/Plan/Flow/AcceptEdits/Yolo） | 部分缺口 | P1 |
| 工具分类审批 | ✅ | ✅ 分类器 | ✅ 5 类别（Read/Write/Shell/Network/Destructive） | 已对齐 | — |
| 路径规则 | ✅ 正则匹配 | ✅ glob 匹配 | ❌ | 完全缺失 | P1 |
| 自动模式分类器 | ❌ | ✅ 独立分类器模型审查操作 | ❌ | 完全缺失 | P1 |
| 受保护路径 | ✅ `.env` 等 | ✅ 系统路径保护 | ❌ | 完全缺失 | P1 |
| 审批链 | ✅ deny→allow→ask | ✅ 规则→分类器→提示 | ✅ 5 层防御链 | 已对齐 | — |
| Bypass 免疫 | ❌ | ❌ | ✅ bypass_immune guard | goat_rust 独有 | — |

### 2.8 Hook/插件系统

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| Hook 事件数 | ~20 个事件 | **30 个事件** | **4 个**（PreTool/PostTool/PreMessage/OnFinish） | **严重不足** | **P0** |
| PreToolUse | ✅ `tool.execute.before` | ✅ PreToolUse | ✅ PreTool | 已对齐 | — |
| PostToolUse | ✅ `tool.execute.after` | ✅ PostToolUse | ✅ PostTool | 已对齐 | — |
| SessionStart | ✅ `session.created` | ✅ SessionStart | ❌ | 完全缺失 | P1 |
| SessionEnd | ✅ `session.deleted` | ✅ SessionEnd | ❌ | 完全缺失 | P1 |
| UserPromptSubmit | ❌ | ✅ UserPromptSubmit | ❌ | 完全缺失 | P1 |
| Notification | ❌ | ✅ Notification | ❌ | 完全缺失 | P2 |
| Stop | ❌ | ✅ Stop | ❌（OnFinish 部分覆盖） | 部分缺口 | P1 |
| PreCompact/PostCompact | ✅ `experimental.session.compacting` | ✅ PreCompact/PostCompact | ❌ | 完全缺失 | P1 |
| SubagentStart/Stop | ❌ | ✅ SubagentStart/SubagentStop | ❌ | 完全缺失 | P1 |
| PermissionRequest | ✅ `permission.asked` | ✅ PermissionRequest | ❌ | 完全缺失 | P1 |
| 插件市场 | ✅ npm 插件 | ✅ 官方市场 | ❌ | 完全缺失 | P2 |
| 自定义工具扩展 | ✅ 插件工具（同名覆盖） | ✅ Skill + MCP 工具 | ❌ | 完全缺失 | P1 |

### 2.9 Plan Mode 工作机制

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| 进入方式 | `plan` Agent（内置 primary Agent） | `/plan` + EnterPlanMode 工具 | `AgentMode::Plan` 枚举 | 部分缺口 | P1 |
| 计划编辑 | ✅ 写 `.opencode/plans/*.md` | ✅ `Ctrl+G` 打开编辑器 | ❌ | 完全缺失 | P1 |
| 计划审批 | ✅ 用户确认后切换模式 | ✅ 4 种选项（auto/acceptEdits/default/ultraplan） | ❌ | 完全缺失 | P1 |
| 只读限制 | ✅ plan Agent 默认禁止编辑 | ✅ 仅读取 | ✅ Plan 模式限制 | 已对齐 | — |
| 自动命名 | ❌ | ✅ 批准计划自动命名会话 | ❌ | 完全缺失 | P2 |

### 2.10 TUI / 终端交互

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| TUI 框架 | 自研多窗格 TUI（Go） | 终端聊天界面（Node.js） | ratatui（Rust）⏳ | 部分缺口 | P1 |
| 文件树/Diff 面板 | ✅ 右侧面板 | ✅ 行内 diff | ❌ | 完全缺失 | P1 |
| 多窗格布局 | ✅ 左侧消息 + 右侧文件树 + 底部状态 | ❌ 单面板 | ⏳ 计划中 | 部分缺口 | P1 |
| 键盘导航 | ✅ 纯键盘（Tab/方向键） | ✅ Shift+Tab 切换模式 | ⏳ app.rs 骨架存在 | 部分缺口 | P1 |
| 状态栏 | ✅ 底部状态栏 | ✅ 权限模式指示 | ⏳ | 部分缺口 | P1 |
| 桌面应用 | ✅ Desktop 支持 | ✅ macOS/Windows/Linux | ✅ Tauri v2 | 已对齐 | — |

### 2.11 Web Search / Web Fetch

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| Web Search | ✅ WebSearchTool | ✅ WebSearch（最多 8 次后端搜索） | ❌ | **完全缺失** | **P0** |
| Web Fetch | ✅ WebFetchTool（15分钟缓存） | ✅ WebFetch（Markdown 转换） | ❌ | **完全缺失** | **P0** |
| 域名过滤 | ❌ | ✅ allowed/blocked domains | ❌ | 完全缺失 | P2 |

### 2.12 安全/沙箱

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| Docker 沙箱 | ✅ 原生 Docker 会话 | ❌ | ❌ | 完全缺失 | P2 |
| Git Worktree 隔离 | ❌ | ✅ EnterWorktree/ExitWorktree | ❌ | 完全缺失 | P2 |
| 路径沙箱 | ✅ `safe_path` | ✅ workspace 限制 | ✅ `safe_path` | 已对齐 | — |
| 命令分类器 | ❌ | ✅ auto mode 分类器 | ❌（审批链中无分类器） | 完全缺失 | P1 |
| 磁盘操作保护 | ❌ | ✅ `rm -rf /` 断路器 | ❌（sandbox.rs 存在但未集成） | 完全缺失 | P1 |

### 2.13 Git/GitHub 集成

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| Git 操作工具 | ❌（通过 Bash） | ❌（通过 Bash） | ✅ GitTool | 已对齐 | — |
| 自动 commit | ❌ | ✅ 自动提交 + PR 创建 | ❌ | 完全缺失 | P2 |
| PR 审查 | ✅ `opencode pr` | ✅ `/review` + `/pr_comments` | ❌ | 完全缺失 | P2 |
| Issue 转 PR | ❌ | ✅ Issue → 代码 → 测试 → PR | ❌ | 完全缺失 | P2 |
| Worktree 管理 | ❌ | ✅ EnterWorktree/ExitWorktree | ❌ | 完全缺失 | P2 |

### 2.14 IDE/Slack 集成

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| VS Code 扩展 | ❌（通过终端使用） | ✅ 原生扩展 | ❌ | 完全缺失 | P2 |
| JetBrains 扩展 | ❌ | ✅ 专用插件 | ❌ | 完全缺失 | P2 |
| Slack 集成 | ❌ | ✅ | ❌ | 完全缺失 | P2 |
| GitHub Actions | ✅ `opencode serve` | ✅ | ❌ | 完全缺失 | P2 |

### 2.15 斜杠命令系统

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| 内置命令 | ❌（主要通过 TUI 交互） | ✅ 19 个内置命令（/clear, /compact, /model, /review 等） | ❌ | **完全缺失** | **P0** |
| 自定义命令 | ❌（通过插件） | ✅ `.claude/commands/` + 前置元数据 | ❌ | 完全缺失 | P1 |
| 参数传递 | ❌ | ✅ `$ARGUMENTS` / `$1` / `$2` | ❌ | 完全缺失 | P1 |

### 2.16 记忆/向量系统

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| 向量记忆 | ❌ | ❌ | ✅ Zvec 向量存储 | **goat_rust 独有** | — |
| 本地嵌入 | ❌ | ❌ | ✅ Candle 本地嵌入 | **goat_rust 独有** | — |
| 代码语义索引 | ❌ | ❌ | ✅ code_index.rs | **goat_rust 独有** | — |

### 2.17 运行模式（非交互式 / CI）

| 功能维度 | OpenCode | Claude Code | goat_rust (当前) | 缺口程度 | 优先级 |
|---------|----------|-------------|-----------------|---------|--------|
| 非交互模式 | ✅ `opencode "任务"` | ✅ `claude -p "任务"` | ❌ | 完全缺失 | P1 |
| HTTP Server 模式 | ✅ `opencode serve` | ❌ | ❌ | 完全缺失 | P2 |
| CI/CD 集成 | ✅ | ✅ GitHub Actions | ❌ | 完全缺失 | P2 |
| DontAsk 模式 | ❌ | ✅ `--permission-mode dontAsk` | ❌（Yolo 近似但不完全） | 部分缺口 | P1 |

---

## 三、功能缺口汇总

### P0 级缺口（阻塞性 — 必须优先解决）：共 10 项

| # | 缺口 | 描述 |
|---|------|------|
| 1 | **工具总数不足** | 7 个 vs OpenCode 16 / Claude Code 38，缺少 WebSearch、WebFetch、TaskTool、SkillTool 等核心工具 |
| 2 | **WebSearch 缺失** | 两个竞品标配，goat_rust 完全缺失 |
| 3 | **WebFetch 缺失** | 两个竞品标配，goat_rust 完全缺失 |
| 4 | **TaskTool 缺失** | LLM 无法自行决定创建子 Agent，这是 Agentic 能力的核心 |
| 5 | **Skill 系统缺失** | 两个竞品通过 Skill 扩展 Agent 能力，goat_rust 完全缺失 |
| 6 | **项目上下文配置** | 无 AGENTS.md / CLAUDE.md 加载、无分层配置系统 |
| 7 | **Rules 系统缺失** | 无法按项目/目录自定义规则 |
| 8 | **Hook 事件严重不足** | 4 个 vs Claude Code 30 个，大量关键生命周期缺失 |
| 9 | **斜杠命令系统** | 两个竞品都有丰富的命令系统，goat_rust 完全缺失 |
| 10 | **Plan Mode 流程不完整** | 有 Plan 模式枚举但无 Enter/Exit 工具，无计划审批流程 |

### P1 级缺口（重要 — v1.0 前应完成）：共 12 项

| # | 缺口 | 描述 |
|---|------|------|
| 1 | TodoWrite/TodoRead 工具 | 任务追踪的基础设施 |
| 2 | AskUser/Question 工具 | 向用户澄清歧义的核心交互 |
| 3 | Provider 数量不足 | 仅 2 个 vs OpenCode 20+，需扩展 Gemini、Groq、本地模型 |
| 4 | 模型路由未实现 | model_router.rs 文件存在但未在循环中使用 |
| 5 | 死循环检测 | OpenCode 和 Claude Code 都有 |
| 6 | 权限路径规则 | 无法按路径模式设置权限 |
| 7 | 自动模式分类器 | Claude Code auto mode 的核心差异化能力 |
| 8 | 非交互式运行模式 | `-p` 一次性任务执行 |
| 9 | TUI 实现 | app.rs 骨架存在但未完成 |
| 10 | 自定义 Agent 定义 | 用户无法创建自定义 Agent |
| 11 | MCP 自动配置加载 | 无法从配置文件自动加载 MCP 服务器 |
| 12 | LSP 集成 | OpenCode 和 Claude Code 都有 LSP 支持 |

### P2 级缺口（锦上添花 — v1.5 阶段）：共 14 项

| # | 缺口 | 描述 |
|---|------|------|
| 1 | Extended Thinking | Claude Code 独有能力 |
| 2 | IDE 扩展（VS Code/JetBrains） | Claude Code 的核心生态优势 |
| 3 | GitHub/GitLab PR 自动化 | 自动 commit → PR 创建 |
| 4 | 定时任务 (Cron) | Claude Code 独有 |
| 5 | Notebook 编辑 | Claude Code 独有 |
| 6 | 后台监控 (Monitor) | Claude Code 独有 |
| 7 | 桌面通知推送 | Claude Code 独有 |
| 8 | Docker 沙箱 | OpenCode 独有 |
| 9 | Worktree 隔离 | Claude Code 独有 |
| 10 | 会话 Fork/导出 | 两个竞品都有 |
| 11 | `/init` 自动初始化 | Claude Code 独有 |
| 12 | 崩溃恢复 | Claude Code 成熟实现 |
| 13 | 插件市场 | 两个竞品都有 |
| 14 | Cloud Provider 支持 (Bedrock/Vertex) | OpenCode 独有 |

**统计**：P0: 10 项 | P1: 12 项 | P2: 14 项 | 已对齐: ~15 项

---

## 四、优先级判定矩阵

按 **用户感知度 × 竞品标配度 × 实现复杂度 × 阻塞性** 四维综合评分（1-5 分）：

| 功能 | 用户感知度 | 竞品标配度 | 实现复杂度(逆) | 阻塞性 | **综合分** | 优先级 |
|------|-----------|-----------|---------------|--------|-----------|--------|
| 补充核心工具（WebSearch/WebFetch/Task） | 5 | 5 | 3 | 5 | **4.5** | **P0** |
| 斜杠命令系统 | 5 | 3 | 4 | 3 | **3.8** | **P0** |
| 项目上下文配置（AGENTS.md/Rules） | 4 | 5 | 4 | 4 | **4.3** | **P0** |
| Hook 事件扩展 | 3 | 4 | 3 | 4 | **3.5** | **P0** |
| Skill 系统 | 4 | 4 | 3 | 4 | **3.8** | **P0** |
| Plan Mode 完整流程 | 3 | 4 | 3 | 3 | **3.3** | **P0** |
| TodoWrite/AskUser 工具 | 4 | 5 | 5 (简单) | 3 | **4.3** | **P1** |
| 死循环检测 | 2 | 4 | 4 | 4 | **3.5** | **P1** |
| 权限路径规则 | 3 | 4 | 4 | 3 | **3.5** | **P1** |
| 模型路由 | 3 | 2 | 3 | 3 | **2.8** | **P1** |
| TUI 实现 | 5 | 4 | 2 | 3 | **3.5** | **P1** |
| 非交互模式 | 3 | 4 | 5 (简单) | 3 | **3.8** | **P1** |
| LSP 集成 | 4 | 4 | 2 | 2 | **3.0** | **P1** |
| IDE 扩展 | 5 | 2 | 1 | 1 | **2.3** | **P2** |
| Git 工作流自动化 | 3 | 2 | 3 | 2 | **2.5** | **P2** |
| Extended Thinking | 2 | 1 | 3 | 1 | **1.8** | **P2** |
| Docker 沙箱 | 2 | 2 | 2 | 2 | **2.0** | **P2** |

---

## 五、分阶段里程碑建议

### MVB（最小可用版本）— 目标：可用

**核心目标**：让用户完成基本的编码工作流（读→改→搜→执行），达到可用水平。

| # | 功能 | 类别 |
|---|------|------|
| 1 | **WebSearch + WebFetch 工具** | P0 核心工具 |
| 2 | **TaskTool**（子 Agent 创建） | P0 核心工具 |
| 3 | **AskUserQuestion 工具** | P1 核心交互 |
| 4 | **斜杠命令系统**（最少 /help /clear /model /compact） | P0 命令系统 |
| 5 | **AGENTS.md / .goat 项目配置加载** | P0 项目上下文 |
| 6 | **非交互模式**（`goat "任务"` 单次执行） | P1 运行模式 |
| 7 | **死循环检测** | P1 安全 |

**里程碑指标**：工具 ≥ 12 个，可完成日常编码任务，支持单项目配置。

### V1.0（功能对齐）— 目标：功能对标 OpenCode

**核心目标**：功能不再明显落后于 OpenCode，用户有迁移理由。

| # | 功能 | 类别 |
|---|------|------|
| 1 | **Skill 系统**（SKILL.md 加载 + 工具化） | P0 扩展性 |
| 2 | **TodoWrite/TodoRead 工具** | P1 任务追踪 |
| 3 | **Hook 事件扩展**（+10 个关键事件） | P0 生命周期 |
| 4 | **完整 Plan Mode**（EnterPlanMode/ExitPlanMode 工具 + 审批流） | P0 工作流 |
| 5 | **权限路径规则**（glob 匹配） | P1 安全 |
| 6 | **Provider 扩展**（Gemini + Ollama 本地模型） | P1 多模型 |
| 7 | **模型路由**（按 Agent/task 自动选择模型） | P1 智能化 |
| 8 | **TUI 完成**（ratatui 多窗格界面） | P1 体验 |
| 9 | **自定义 Agent 定义** | P1 扩展性 |
| 10 | **MCP 配置自动加载** | P1 MCP |
| 11 | **受保护路径机制** | P1 安全 |

**里程碑指标**：工具 ≥ 20 个，功能对齐 OpenCode，TUI 可用，多模型支持。

### V1.5（超越）— 目标：差异化竞争

**核心目标**：利用 Rust + Tauri 的独特优势，在某些维度超越竞品。

| # | 功能 | 差异化点 |
|---|------|---------|
| 1 | **向量记忆 + 代码语义搜索** | Zvec + Candle 本地嵌入（竞品无） |
| 2 | **LSP 集成**（Rust LSP 原生优势） | 与 OpenCode 对齐 |
| 3 | **自动模式分类器**（本地小模型） | Candle 运行本地分类器，无需 API 调用 |
| 4 | **Tauri 桌面应用完整版**（系统托盘 + 快捷键） | 15MB vs Claude Code Desktop 200MB+ |
| 5 | **Docker/Worktree 沙箱** | 安全隔离 |
| 6 | **IDE 扩展**（VS Code 插件） | 生态扩展 |
| 7 | **Git 工作流自动化**（commit/PR） | 对标 Claude Code |
| 8 | **插件市场**（Rust WASM 插件） | 生态建设 |
| 9 | **Extended Thinking**（借助模型自身） | 对标 Claude Code |
| 10 | **定时任务 / Routine 系统** | 对标 Claude Code Routines |

**里程碑指标**：35+ 工具，向量记忆可用，桌面应用发布，IDE 扩展发布，部分维度超越竞品。

---

## 六、差异化竞争策略

### 6.1 goat_rust 的核心优势（已具备/规划中）

| 优势 | 说明 | 竞品对比 |
|------|------|---------|
| **极致轻量** | Rust 编译 → 单 exe 15-25MB，内存 80-150MB | OpenCode (Go) ~40MB，Claude Code (Node.js) ~200MB+ |
| **本地向量记忆** | Zvec + Candle 离线嵌入 → 语义代码搜索 | 两个竞品都无此能力 |
| **Flow Pipeline** | 实现→diff→审查→修复 自动化流水线 | 两个竞品无内置 Flow |
| **Git 专用工具** | 内置 GitTool，不依赖 Shell | 两个竞品通过 Shell 调用 git |
| **bypass_immune** | 即使 YOLO 模式下也能阻止高危操作 | Claude Code 只有 `rm -rf /` 断路器 |
| **Tauri 桌面应用** | 原生系统托盘 + WebView，15MB 安装包 | Claude Code Desktop 基于 Electron，200MB+ |
| **本地模型推理** | Candle 运行本地嵌入/分类器 | OpenCode 支持 Ollama 但不内置模型 |

### 6.2 竞品弱点可超越的方向

| 竞品弱点 | goat_rust 超越策略 |
|---------|-------------------|
| **Claude Code 锁定 Anthropic** | goat_rust 多 Provider 自由切换 |
| **Claude Code $20/月起** | goat_rust 免费 + 支持本地/免费模型 |
| **Claude Code 无 LSP** | goat_rust 原生集成 LSP（Rust 生态优势） |
| **Claude Code 无本地模型** | goat_rust Candle + Ollama 双路径 |
| **OpenCode Agentic 成功率低 8%** | goat_rust 通过 Flow Pipeline + ReAct 优化 |
| **OpenCode MCP 生态较新** | goat_rust MCP 原生支持 client+server |
| **两者均无向量记忆** | goat_rust Zvec 代码语义搜索 |

### 6.3 核心竞争主张

> **「比 Claude Code 更自由，比 OpenCode 更可靠」**
>
> goat_rust = 本地优先（Rust 单二进制 15MB）+ 多模型自由（不锁定供应商）+ 向量记忆（代码语义搜索）+ Tauri 桌面应用

**目标用户画像**：
1. 注重隐私/安全的开发者（不能在云端共享代码）
2. 成本敏感的独立开发者/学生（避免 $20-100/月订阅）
3. 需要语义代码搜索的大型项目维护者
4. 偏好本地模型的 AI 爱好者

---

## 附录：数据来源

- OpenCode GitHub: https://github.com/sst/opencode
- OpenCode 架构分析: https://morsewayne.github.io/programming_journey/docs/ai/opencode/02-opencode-architecture.html
- OpenCode 官方文档: https://opencode.ai/docs/
- Claude Code 官方文档: https://code.claude.com/docs/
- Claude Code 工具参考: https://code.claude.com/docs/en/tools-reference
- Claude Code Hooks 参考: https://code.claude.com/docs/en/hooks
- Claude Code 权限模式: https://code.claude.com/docs/en/permission-modes
- OpenCode vs Claude Code 对比: https://www.openaitoolshub.org/en/blog/opencode-vs-claude-code
- goat_rust 源码: `goat_rust/rgoat-core/src/`（现场分析）
