# goat_rust MVB（最小可用版本）产品需求文档

> 版本: v1.0  
> 作者: 许清楚（产品经理）  
> 日期: 2025-06-25  
> 项目: goat_rust — Rust 轻量级本地 AI 编码助手

---

## 1. 项目信息

| 项 | 值 |
|---|-----|
| **Language** | 中文（文档）/ Rust（代码） |
| **Programming Language** | Rust 2021 Edition |
| **Project Name** | `goat_rust` |
| **Workspace** | rgoat-core(lib) + rgoat-tui(bin) + rgoat-desktop(bin) |
| **原始需求** | 合并两份优化文档的 P0/MVB 项，完成从"可用原型"到"可对外发布 MVB"的升级 |

---

## 2. 产品定义

### 2.1 产品目标

| # | 目标 | 衡量标准 |
|---|------|---------|
| G1 | **能力完整**：补齐竞品标配的联网搜索、子 Agent 调度、规则/技能系统，使 MVB 具备独立完成复杂编码任务的能力 | 所有 P0 项交付并通过集成测试 |
| G2 | **安全可信**：实现路径沙箱验证、死循环检测，确保本地 Agent 在用户可控范围内运行 | 路径越权被阻断、连续重复 ToolCall 被截断 |
| G3 | **体验流畅**：Anthropic 流式响应、斜杠命令交互、取消/暂停控制，达到日常使用标准 | TUI 中 Anthropic 流式输出无卡顿，斜杠命令全部可用 |

### 2.2 用户故事

#### 模块 A：规则与技能系统（Rules & Skills）

| # | 用户故事 | 验收标准 |
|---|---------|---------|
| US-A1 | As a 开发者，I want goat 自动加载项目中的 AGENTS.md / .goat 规则文件，so that Agent 行为符合团队约定 | 启动时按"目录 > 项目根 > 全局 ~/.goat"优先级加载，规则内容注入 system prompt |
| US-A2 | As a 开发者，I want goat 加载预定义 skills 并在需要时调用，so that Agent 可执行专业化任务流程 | skills 目录扫描 → 注入 tools 列表 → LLM 可调用 skill |

#### 模块 B：联网能力（WebSearch & WebFetch）

| # | 用户故事 | 验收标准 |
|---|---------|---------|
| US-B1 | As a 开发者，I want goat 能搜索最新技术文档/API 用法，so that 编码时使用最新 API | `web_search` 工具注册，DuckDuckGo API 返回 Top 10 结果 |
| US-B2 | As a 开发者，I want goat 能抓取网页内容并转为可读文本，so that 直接引用文档内容 | `web_fetch` 工具注册，HTML→Markdown 转换，15min 内重复 URL 命中缓存 |

#### 模块 C：子 Agent 调度（TaskTool）

| # | 用户故事 | 验收标准 |
|---|---------|---------|
| US-C1 | As a 开发者，I want goat 能将复杂任务拆解为子任务并并行执行，so that 多文件重构等场景效率提升 | `task` 工具注册，LLM 可创建子 Agent，复用 SubAgentRuntime |
| US-C2 | As a 开发者，I want 子 Agent 执行结果汇总到主对话，so that 我能看到完整执行链路 | 子 Agent 完成的产物回传到主 Agent 上下文 |

#### 模块 D：斜杠命令系统

| # | 用户故事 | 验收标准 |
|---|---------|---------|
| US-D1 | As a TUI 用户，I want 输入 `/help` 查看所有命令，so that 无需查阅外部文档 | `/help` 输出命令列表及说明 |
| US-D2 | As a TUI 用户，I want `/clear` 清空上下文、`/model <name>` 切换模型、`/compact` 压缩对话、`/resume <id>` 恢复会话、`/plan` 进入规划模式 | 所有命令在 TUI 输入框中触发并正确执行 |

#### 模块 E：Provider 与安全

| # | 用户故事 | 验收标准 |
|---|---------|---------|
| US-E1 | As a Claude API 用户，I want Anthropic 流式响应，so that 长回复时实时看到生成内容 | `AnthropicProvider::chat_stream` 正确实现 SSE 解析 |
| US-E2 | As a 用户，I want 沙箱阻止 Agent 访问工作空间外的敏感路径，so that 代码执行安全可控 | `is_path_allowed()` 基于 workspace root 做前缀匹配 + 黑名单路径拦截 |
| US-E3 | As a 桌面用户，I want 通过 UI 按钮取消/暂停正在运行的 Agent，so that 失控 Agent 可立即终止 | `cancel` 和 `pause` IPC 命令通过 Tauri invoke 调用 |

#### 模块 F：稳定性保障

| # | 用户故事 | 验收标准 |
|---|---------|---------|
| US-F1 | As a 用户，I want Agent 陷入死循环时自动检测并终止，so that 不浪费 API Token | ToolCallDeduper 检测连续重复调用 ≥3 次 → 自动 Abort |
| US-F2 | As a CLI 用户，I want `goat "任务描述"` 单次执行后退出，so that 可集成到 CI/脚本 | 非交互模式参数解析 → 执行 → 输出结果 → 退出码 |

---

## 3. 技术规范

### 3.1 需求池

#### P0 — Must Have（MVB 必须交付）

| ID | 模块 | 需求 | 来源 | 当前状态 |
|----|------|------|------|---------|
| P0-01 | agent/react | 实现 project rules 分层加载（AGENTS.md / .goat） | Doc1-T1 + Doc2-#6 | `build_system_prompt` 传入空数组 |
| P0-02 | agent/react | 实现 skills 加载与注入 | Doc1-T2 | `build_system_prompt` 传入空数组 |
| P0-03 | tools | 新增 WebSearch 工具（DuckDuckGo API） | Doc1-T10 + Doc2-#1 | 未实现 |
| P0-04 | tools | 新增 WebFetch 工具（reqwest + HTML→MD + 缓存） | Doc1-T11 + Doc2-#2 | 未实现 |
| P0-05 | tools | 新增 TaskTool（子 Agent 创建，复用 SubAgentRuntime） | Doc2-#3 | 未实现 |
| P0-06 | provider | 实现 AnthropicProvider::chat_stream（SSE 流式） | Doc1-T8 | 当前返回 `Err` |
| P0-07 | security | 实现 is_path_allowed() 路径验证 | Doc1-T21 | 当前恒返回 `true` |
| P0-08 | desktop | 增加取消/暂停 Agent 的 IPC 命令（cancel/pause） | Doc1-T26 | 未实现 |
| P0-09 | tui/cli | 斜杠命令系统（/help /clear /model /compact /resume /plan） | Doc2-#5 | 未实现 |

#### P1 — Should Have（MVB 建议包含）

| ID | 模块 | 需求 | 来源 |
|----|------|------|------|
| P1-01 | tools | AskUserQuestion 工具（交互/TUI 回调 + 非交互 CLI 参数） | Doc1-T13/28 + Doc2-#4 |
| P1-02 | agent | 死循环检测：ToolCallDeduper + 截断检测 | Doc2-+ |
| P1-03 | cli | 非交互模式（`goat "任务"` 单次执行） | Doc2-#7 |
| P1-04 | tools | Shell 超时控制 | Doc1-T16 |
| P1-05 | agent | ContextCompressor 集成到 ReAct 循环 | Doc1-T23 |
| P1-06 | memory | Zvec 持久化向量存储（依赖链长，从 P0 降级） | Doc1-T3 |

#### P2 — Nice to Have

| ID | 模块 | 需求 | 来源 |
|----|------|------|------|
| P2-01 | tui | TUI Index 命令实现 | Doc1-T25 |
| P2-02 | agent | 交互式 Decision::Ask（丰富审批交互） | Doc1-T28 |

### 3.2 技术实现要点

#### 3.2.1 Rules 分层加载（P0-01）

```
加载优先级（后加载覆盖先加载）:
1. ~/.goat/rules.md          ← 全局规则
2. <project>/.goat/rules.md  ← 项目规则
3. <project>/AGENTS.md       ← 项目约定（兼容 OpenCode 生态）
4. <cwd>/.goat/rules.md      ← 当前目录规则（递归向上查找）

实现路径: agent/react.rs → build_system_prompt() → rules 参数
```

#### 3.2.2 Skills 加载（P0-02）

```
Skills 目录扫描: ~/.goat/skills/ + <project>/.goat/skills/
每个 skill 目录结构: {name}/SKILL.md (描述+prompt)
加载后注入为 LLM 可调用的工具定义
```

#### 3.2.3 WebSearch（P0-03）

```
方案: DuckDuckGo Instant Answer API (api.duckduckgo.com)
依赖: reqwest + serde_json
无 API Key 要求，免费使用
速率限制: 建议内置 1 req/s 节流
```

#### 3.2.4 WebFetch（P0-04）

```
方案: reqwest GET → html2md 转换 → 15min LRU 缓存
缓存 Key: URL 的 SHA256
缓存存储: 内存 HashMap (后续可升级 SQLite)
```

#### 3.2.5 TaskTool（P0-05）

```
方案: 复用现有 SubAgentRuntime
接口: task(description, context_files[]) → task_result
子 Agent 独立上下文窗口，结果以结构化格式回传
```

#### 3.2.6 Anthropic 流式（P0-06）

```
方案: Anthropic Messages API Streaming (SSE)
端点: POST /v1/messages with stream: true
Headers: anthropic-version: 2023-06-01, anthropic-beta: messages-2023-12-15
事件类型: message_start, content_block_start, content_block_delta, content_block_stop, message_delta, message_stop
```

#### 3.2.7 路径沙箱（P0-07）

```
方案: workspace root 前缀匹配 + 硬编码黑名单
黑名单路径（始终拒绝）:
  - C:\Windows, C:\Windows\System32
  - /etc, /usr, /bin, /boot
  - ~/.ssh, ~/.gnupg
允许: workspace + 子路径
```

#### 3.2.8 斜杠命令（P0-09）

```
命令解析: TUI 输入框前置拦截，以 '/' 开头触发
/help     → 列出所有命令 + 简要说明
/clear    → 清空当前会话上下文
/model    → 切换 LLM 模型（/model gpt-4o）
/compact  → 触发上下文压缩
/resume   → 恢复历史会话（/resume <session_id>）
/plan     → 切换到 Plan Mode
```

### 3.3 UI 设计草案

#### TUI 斜杠命令交互

```
┌─────────────────────────────────────────────────────┐
│ goat > /help                                         │
│                                                       │
│   Available Commands:                                │
│   /help      Show this help                          │
│   /clear     Clear current conversation              │
│   /model     Switch model (/model <name>)            │
│   /compact   Compress conversation context           │
│   /resume    Resume a session (/resume <id>)         │
│   /plan      Enter plan mode                         │
│                                                       │
│ [Enter] to continue                                  │
└─────────────────────────────────────────────────────┘
```

#### 子 Agent 调度流程（TaskTool）

```
主 Agent                         子 Agent 1         子 Agent 2
  │                                │                  │
  │── task("重构 auth.rs") ──────→│                  │
  │── task("重构 db.rs") ──────────────────────────→│
  │                                │                  │
  │                            执行中...           执行中...
  │                                │                  │
  │←── result: "完成" ───────────│                  │
  │←── result: "完成" ────────────────────────────│
  │                                │                  │
  │── 汇总结果，继续推理 ──────→   │                  │
```

#### 非交互模式 CLI

```bash
# 单次执行
$ goat "为 src/lib.rs 添加单元测试"
[goat] 分析中...
[goat] 已添加 3 个测试用例到 src/lib.rs
[goat] cargo test 通过 ✓

# 带参数
$ goat --model claude-3.5-sonnet --mode plan "设计用户认证模块"
```

---

## 4. 待确认问题

| # | 问题 | 选项 | 影响范围 |
|---|------|------|---------|
| Q1 | Zvec 向量存储是否延期到 P2？当前依赖链长且 MVB 阶段可先用内存向量存储替代 | A) 延期 P2 B) 保持 P1 但仅做接口预留 | memory 模块 |
| Q2 | DuckDuckGo API 有速率限制，是否需要同时支持可选的 Google/Bing API（需 API Key）作为后备？ | A) 仅 DuckDuckGo B) 支持多搜索引擎配置 | WebSearch 工具 |
| Q3 | TaskTool 的子 Agent 是否需要独立的审批模式？还是继承主 Agent 的审批设置？ | A) 继承主 Agent B) 可配置独立模式 | security/approval |
| Q4 | Skills 的存储格式？是否兼容 OpenCode/Cursor 的 `.cursor/rules` 格式？ | A) 独立 .goat/skills/ 格式 B) 兼容竞品格式 | Skills 系统 |
| Q5 | 非交互模式的审批策略？所有操作自动批准还是需要预先授权？ | A) 自动批准（CI 友好）B) CLI 参数控制 | CLI 非交互模式 |
| Q6 | MVB 是否保留对 Tauri 桌面的支持？当前 P0 中 desktop IPC 命令需要 Tauri 前端配合 | A) MVB 首推 TUI，Desktop 仅保证不崩溃 B) Desktop 完整支持 | rgoat-desktop |
