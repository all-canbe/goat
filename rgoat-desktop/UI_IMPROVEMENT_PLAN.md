# RGoat Desktop UI 改进计划

> 目标：将 Desktop 应用的用户体验提升至与原 Goat Web (Python + React) 同等或更优水平

---

## 一、当前状态分析

### 1.1 现状总结

| 维度 | 当前状态 | 评估 |
|------|---------|------|
| 基础布局 | 纯纵向排列：标题栏 → 工具栏 → 消息区 → 输入框 | ❌ 无侧边栏、无状态栏 |
| 消息渲染 | 纯文本 pre-wrap，无 Markdown，无代码高亮 | ❌ 功能缺失 |
| 工具调用 | 简单颜色区分（黄=开始、绿=结果），不可展开 | ❌ 不直观 |
| 会话管理 | 仅 "Clear" / "New Session" 按钮，无列表 | ❌ 基本不可用 |
| 流式输出 | 不支持，消息以整块到达 | ❌ 核心功能缺失 |
| 审批 UI | 仅文字提示，无交互对话框 | ❌ 基本不可用 |
| 主题设计 | 单暗色主题，无亮色支持 | ⚠️ 可接受 |
| 生产力功能 | 无命令面板、无文件树、无快捷键 | ❌ 缺失 |
| JS ↔ Rust IPC | 事件格式不匹配，数据通路错误 | ❌ 根因导致不可用 |
| 配置向导 | 功能正常（Setup 屏幕） | ✅ 可用 |

### 1.2 核心 Bug — 事件格式不匹配

**Rust 端发送格式**（`lib.rs` 第 71-75 行）：
```rust
let payload = serde_json::json!({
    "eventType": format!("{:?}", event.event_type),
    "source": event.source,
    "data": event.data,   // ← 字符串, 不是结构体
});
```

**JS 端期望格式**（`index.html` 第 137-151 行）：
```js
function handleAgentEvent(payload) {
  const ae = typeof data === 'string' ? JSON.parse(data) : data;
  switch (ae.type) {        // ← 应该是 eventType
    case 'started':           // ← 应该是 Started (Debug 格式)
    case 'thought':           // ← 应该是 LlmStreamChunk
    // ae.content, ae.tool_name 等字段不存在于 payload.data 中
  }
}
```

**问题**：
1. 字段名不匹配：Rust 发 `eventType`，JS 读 `type`
2. EventType 值是 Rust Debug 格式（如 `Started`, `LlmStreamChunk`, `ToolCallResult`），而非 snake_case
3. `event.data` 是 `serde_json::Value` 序列化后的字符串，不是扁平的 `{ content, tool_name, ... }` 结构
4. JS 需要从 `payload.data.data` 中解析实际的结构化信息

### 1.3 数据通路架构问题

```
当前：Agent → EventBus → lib.rs thread → app.emit("agent-event", payload) → JS listen
                                    ↓
                               payload = { eventType: "DebugStr", source: "...", data: "..." }
                                    ↓
                              JS 无法正确解析 data 字段内容
```

**根本问题**：Bridge 层只是透传 `EventBus` 的原始事件，没有做格式转换/结构化。

---

## 二、与原 Goat Web 对比

### 2.1 功能对比矩阵

| 功能 | 原 Goat Web | RGoat Desktop (当前) | 差距 |
|------|------------|---------------------|------|
| **侧边栏** | 会话列表 + 文件树 + 任务列表 | 无 | 🔴 致命 |
| **消息渲染** | ReactMarkdown + 代码高亮复制 | 纯文本 | 🔴 致命 |
| **工具调用卡片** | 展开/折叠 参数 + 结果 | 简单颜色行 | 🔴 致命 |
| **流式输出** | WebSocket 逐 token | 不支持 | 🔴 致命 |
| **审批对话框** | 风险级别 + 参数 + diff + Y/N 键 | 仅文字 | 🔴 致命 |
| **Ask User** | 底部弹窗 + 输入框 | 不支持 | 🔴 致命 |
| **命令面板** | `/` 60+ 命令 | 不支持 | 🟡 重要 |
| **模式切换** | Agent/Plan/Flow/YOLO | 仅下拉框 | 🟡 重要 |
| **状态栏** | Provider/模式/Token/上下文% | 仅 Provider 名 | 🟡 重要 |
| **文件编辑器** | 可拖拽 + 缩放 + 保存 | 不支持 | 🟢 次要 |
| **子 Agent 面板** | 状态 + 进度 | 不支持 | 🟢 次要 |
| **主题切换** | 暗色/亮色 | 仅暗色 | 🟢 次要 |
| **消息搜索** | Ctrl+F + F3 导航 | 不支持 | 🟢 次要 |
| **配置向导** | 服务端配置 | Setup 屏幕 ✅ | ✅ 持平 |

### 2.2 关键体验差距

原 Goat Web 是一个**生产级 IDE 辅助面板**，当前 Desktop 是一个**概念验证聊天窗口**。

---

## 三、总体改进策略

### 3.1 技术路线选择

考虑到以下因素：
- 原 Goat Web 使用 React + Tailwind + Zustand（37 个组件文件）
- Tauri Desktop 本质是 WebView，可嵌入任意 HTML
- 维护一个单文件 2000+ 行 HTML 是不可持续的

**推荐方案：Phase 1 (立即修复) + Phase 2 (渐进式 React 前端)**

```
Phase 1: 紧急修复 → 让当前 UI 可用
Phase 2: 前端重构 → 引入轻量级 React 前端
Phase 3: 功能完善 → 补齐所有原版功能
Phase 4: 体验优化 → 独立创新，超越原版
```

### 3.2 架构演进

```
Phase 1 (修复)                      Phase 2+ (重构)
┌─────────────────────┐            ┌──────────────────────────────┐
│  index.html (单文件)  │            │  frontend/ (Vite + React)     │
│  ┌───────────────┐   │            │  ┌───────────────────────┐   │
│  │ HTML + CSS    │   │            │  │ App.tsx               │   │
│  │ + Vanilla JS  │   │     →      │  │ ├─ Sidebar.tsx        │   │
│  └───────┬───────┘   │            │  │ ├─ ChatArea.tsx       │   │
│          │            │            │  │ │  ├─ MessageList.tsx │   │
│  Tauri IPC Commands  │            │  │ │  ├─ ToolCallCard    │   │
│          │            │            │  │ │  └─ ApprovalDialog │   │
│  Rust Backend        │            │  │ ├─ InputPanel.tsx     │   │
└─────────────────────┘            │  │ └─ StatusBar.tsx      │   │
                                    │  └───────────────────────┘   │
                                    │  Tauri IPC Commands          │
                                    │  Rust Backend                │
                                    └──────────────────────────────┘
```

---

## 四、Phase 1: 紧急修复（预计 2-3 小时）

### 4.1 修复事件格式匹配

**目标**：让消息、工具调用、审批等事件正确显示

**Rust 端改动**（`lib.rs` event bridge）：

不再直接透传 `EventBus` 原始事件，而是在 bridge 层做格式转换：

```rust
// 当前（有问题）：
let payload = serde_json::json!({
    "eventType": format!("{:?}", event.event_type),
    "source": event.source,
    "data": event.data,
});

// 改为结构化转发（方案）：
// 解析 event.data JSON 字符串，提取结构化字段
let data: serde_json::Value = serde_json::from_str(&event.data)
    .unwrap_or(serde_json::Value::Null);

let payload = match event.event_type {
    EventType::Started => serde_json::json!({
        "type": "started",
        "mode": data.get("mode"),
        "prompt": data.get("prompt"),
    }),
    EventType::LlmStreamChunk => serde_json::json!({
        "type": "thought",
        "content": data.get("content"),
    }),
    EventType::ToolCallStarted => serde_json::json!({
        "type": "tool_call",
        "tool_name": data.get("tool_name"),
        "tool_id": data.get("tool_id"),
    }),
    EventType::ToolCallResult => serde_json::json!({
        "type": "tool_result",
        "tool_name": data.get("tool_name"),
        "success": data.get("success"),
        "output": data.get("output"),
    }),
    EventType::ApprovalRequired => serde_json::json!({
        "type": "approval_required",
        "tool_name": data.get("tool_name"),
        "args": data.get("args"),
        "risk": data.get("risk"),
    }),
    EventType::Message => serde_json::json!({
        "type": "message",
        "content": data.get("content"),
    }),
    EventType::Finished => serde_json::json!({
        "type": "finished",
        "answer": data.get("answer"),
    }),
    EventType::Error => serde_json::json!({
        "type": "error",
        "message": data.get("message"),
    }),
};
```

**JS 端对齐**：使用 `payload.type` 匹配 switch case。

### 4.2 修复流式输出支持

**Rust 端**：Agent 调用 LLM 时需逐步发送 token，或至少发送 `LlmStreamChunk` 事件给 EventBus。

**JS 端**：收到 `"thought"` 类型事件时，需要流式累积到同一个消息框中（而非每次创建新消息）：

```js
let streamingContent = '';
let streamingEl = null;

function handleAgentEvent(payload) {
  switch(payload.type) {
    case 'thought':
      if (!streamingEl) {
        streamingEl = createMessageEl('assistant');
        messagesEl.appendChild(streamingEl);
      }
      streamingContent += payload.content || '';
      streamingEl.innerHTML = renderMarkdown(streamingContent);
      scrollToBottom();
      break;
    case 'message':
      // Commit: finalize the streaming message
      streamingContent = '';
      streamingEl = null;
      if (payload.content) {
        addMessage('assistant', payload.content);
      }
      break;
    // ...
  }
}
```

### 4.3 添加基本的 Markdown 渲染

引入一个轻量级 Markdown 渲染库（内联或 CDN），用于消息渲染和 code block 的语法高亮 + 复制按钮。

**临时方案**：在 `<head>` 中添加 marked.js CDN link：
```html
<script src="https://cdn.jsdelivr.net/npm/marked/marked.min.js"></script>
```

### 4.4 修复审批交互

当前审批只显示文本。需要：
1. 新增 `approval_required` 事件类型的弹窗
2. Y/N 键盘快捷键 + 按钮
3. 显示风险级别颜色、工具参数、diff

---

## 五、Phase 2: 前端重构（预计 1-2 天）

### 5.1 创建独立前端项目

```
rgoat-desktop/
├── frontend/                    # 新建
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── tailwind.config.ts
│   ├── postcss.config.js
│   ├── index.html
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       ├── index.css
│       ├── types/index.ts
│       ├── lib/
│       │   └── tauri-bridge.ts   # 封装 Tauri IPC 调用
│       ├── hooks/
│       │   ├── useAgentEvents.ts # 监听 agent-event + 状态管理
│       │   └── useAutoScroll.ts
│       ├── stores/
│       │   ├── chatStore.ts      # 消息列表 + 流式状态
│       │   ├── configStore.ts    # Provider 配置状态
│       │   └── sessionStore.ts   # 会话列表
│       └── components/
│           ├── layout/
│           │   ├── AppLayout.tsx
│           │   ├── Sidebar.tsx
│           │   └── StatusBar.tsx
│           ├── chat/
│           │   ├── ChatArea.tsx
│           │   ├── MessageList.tsx
│           │   ├── UserMessage.tsx
│           │   ├── AssistantMessage.tsx
│           │   ├── ToolCallCard.tsx
│           │   ├── SystemMessage.tsx
│           │   └── ErrorMessage.tsx
│           ├── input/
│           │   └── InputPanel.tsx
│           ├── approval/
│           │   └── ApprovalDialog.tsx
│           ├── setup/
│           │   └── SetupScreen.tsx
│           └── command/
│               └── CommandPalette.tsx
├── src/                         # Rust 源码保持不变
│   ├── lib.rs
│   ├── main.rs
│   └── commands.rs
├── dist/                        # 旧版 (Phase 1 使用)
├── tauri.conf.json
└── Cargo.toml
```

### 5.2 技术选型

| 类别 | 选择 | 理由 |
|------|------|------|
| 框架 | React 18 + TypeScript | 与原版一致，无学习成本 |
| 构建 | Vite 5 | 快速 HMR，Tauri 官方推荐 |
| 样式 | Tailwind CSS 3 | 与原版一致，class 复用 |
| 状态 | Zustand 5 | 轻量，与原版一致 |
| Markdown | react-markdown + remark-gfm + rehype-highlight | 与原版一致 |
| 图标 | lucide-react | 与原版一致 |
| Tauri IPC | `@tauri-apps/api` (v2) | 官方 SDK |

### 5.3 主题设计系统

沿用原 Goat Web 的颜色方案并做微调：

```css
:root {
  --color-bg: #0a0a0a;
  --color-surface: #1a1a1a;
  --color-surface-light: #2a2a2a;
  --color-primary: #e94560;
  --color-primary-light: #ff6b81;
  --color-text: #e0e0e0;
  --color-text-muted: #888;
  --color-success: #4ecca3;
  --color-warning: #f0c040;
  --color-error: #e94560;
  --color-border: #333;
  --color-hover: #2a2a3e;
  --font-mono: 'JetBrains Mono', 'Fira Code', 'Consolas', monospace;
  --font-sans: 'Inter', 'Segoe UI', system-ui, sans-serif;
  --radius: 8px;
  --radius-sm: 4px;
}
```

### 5.4 Tauri Build 配置更新

```json
// tauri.conf.json
{
  "build": {
    "frontendDist": "frontend/dist",
    "beforeBuildCommand": "npm run build",
    "beforeDevCommand": "npm run dev"
  }
}
```

---

## 六、Phase 2 详细组件规格

### 6.1 AppLayout（主布局）

```
┌────────────────────────────────────────────────────┐
│ Sidebar (260px)    │ Main Content (flex-1)          │
│                    │ ┌──────────────────────────┐   │
│ ┌────────────────┐ │ │ MessageList              │   │
│ │ Logo + New Btn │ │ │ ┌──────────────────────┐ │   │
│ ├────────────────┤ │ │ │ UserMessage          │ │   │
│ │ SessionList    │ │ │ │ AssistantMessage     │ │   │
│ │ ├─ Session 1   │ │ │ │ ToolCallCard         │ │   │
│ │ ├─ Session 2   │ │ │ │ ToolCallCard         │ │   │
│ │ └─ Session 3   │ │ │ │ ...                  │ │   │
│ ├────────────────┤ │ │ └──────────────────────┘ │   │
│ │ FileTree       │ │ ├──────────────────────────┤   │
│ │ TaskList       │ │ │ InputPanel               │   │
│ └────────────────┘ │ └──────────────────────────┘   │
├────────────────────┴────────────────────────────────┤
│ StatusBar                                          │
│ ● Provider │ ● Mode │ ● Status │ ● Tokens │ Theme │
└────────────────────────────────────────────────────┘
```

### 6.2 Sidebar（侧边栏）

```tsx
function Sidebar() {
  return (
    <aside className="w-[260px] h-full flex flex-col bg-[var(--color-surface)] border-r border-[var(--color-border)]">
      {/* Logo + New Session */}
      <div className="p-4 flex items-center justify-between">
        <h1 className="text-lg font-bold text-[var(--color-primary)]">RGoat</h1>
        <button onClick={createNewSession} title="New Session">+</button>
      </div>

      {/* Tabs: Sessions | Files | Tasks */}
      <div className="flex border-b border-[var(--color-border)]">
        <TabButton active={true} label="Sessions" />
        <TabButton label="Files" />
        <TabButton label="Tasks" />
      </div>

      {/* Session List */}
      <SessionList sessions={sessions} activeId={activeSessionId}
        onSelect={switchSession} onDelete={deleteSession} onRename={renameSession} />

      {/* File Tree (when Files tab active) */}
      <FileTree files={workspaceFiles} onAddToChat={addFile} />

      {/* Task List (when Tasks tab active) */}
      <TaskList tasks={backgroundTasks} />
    </aside>
  );
}
```

### 6.3 MessageList（消息列表）

消息类型与渲染卡片：

| 消息类型 | 渲染组件 | 样式要点 |
|---------|---------|---------|
| `user` | `UserMessage` | 右对齐，蓝色气泡，支持附件 chips |
| `assistant` | `AssistantMessage` | 左对齐，Markdown 渲染，代码块复制按钮 |
| `tool_call` | `ToolCallCard` | 展开/折叠，参数 JSON，结果文本，运行中动画 |
| `tool_result` | 合并到 ToolCallCard | 状态图标（✓成功/✗失败），结果展开 |
| `thought` | 流式气泡 | 灰色斜体，打字动画，think 标记 |
| `system` | `SystemMessage` | 居中，小字，灰色 |
| `error` | `ErrorMessage` | 红色边框，错误图标 |

**流式渲染逻辑**（`chatStore.ts`）：

```ts
interface ChatState {
  messages: Message[];
  isStreaming: boolean;
  streamingContent: string;
  streamingToolCalls: ToolCall[];  // 流中累积的工具调用

  appendToken(token: string): void;
  addToolCall(tc: ToolCall): void;
  updateToolResult(toolId: string, result: ToolResult): void;
  commitStream(): void;  // finalized assistant message
}
```

### 6.4 ToolCallCard（工具调用卡片）

```tsx
function ToolCallCard({ toolCall }: { toolCall: ToolCall }) {
  const [expanded, setExpanded] = useState(false);
  const [showResult, setShowResult] = useState(false);

  return (
    <div className="tool-call-card">
      {/* Header - always visible */}
      <div className="flex items-center gap-2 cursor-pointer" onClick={() => setExpanded(!expanded)}>
        <StatusIcon status={toolCall.status} /> {/* 🔄running ✓done ✗error */}
        <span className="font-mono text-sm">{toolCall.name}</span>
        <span className="text-xs text-muted">{summarizeArgs(toolCall.args)}</span>
        <ChevronIcon expanded={expanded} />
      </div>

      {/* Expanded: show args */}
      {expanded && (
        <pre className="args-block"><code>{JSON.stringify(toolCall.args, null, 2)}</code></pre>
      )}

      {/* Result */}
      {toolCall.status !== 'running' && (
        <div className={`result-block ${toolCall.status}`}>
          <div className="flex items-center gap-2 cursor-pointer" onClick={() => setShowResult(!showResult)}>
            <ResultIcon status={toolCall.status} />
            <span className="text-xs">{summarize(toolCall.result, 100)}</span>
          </div>
          {showResult && <pre>{toolCall.result}</pre>}
        </div>
      )}
    </div>
  );
}
```

### 6.5 ApprovalDialog（审批对话框）

```tsx
function ApprovalDialog({ approval, onApprove, onDeny }: Props) {
  return (
    <Overlay>
      <div className="approval-card">
        <h2>Approval Required</h2>
        <RiskBadge level={approval.risk} /> {/* low/medium/high/critical */}
        <p>{approval.description}</p>
        <ToolCallCard toolCall={approval} readonly />

        {approval.diff && (
          <DiffViewer content={approval.diff} />
        )}

        <div className="actions">
          <button onClick={onApprove}>Approve (Y)</button>
          <button onClick={onDeny} className="secondary">Deny (N)</button>
        </div>
      </div>
    </Overlay>
  );
}
```

### 6.6 InputPanel（输入面板）

```tsx
function InputPanel() {
  return (
    <div className="input-panel">
      {/* Attachment chips */}
      {attachments.map(a => <AttachmentChip key={a.id} file={a} onRemove={removeAttachment} />)}

      <div className="flex gap-2">
        <textarea
          ref={inputRef}
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}  // Enter send, Shift+Enter newline
          placeholder="Ask anything... (Enter to send, Shift+Enter for newline)"
          rows={2}
          disabled={isProcessing}
        />
        <button onClick={send} disabled={!input.trim() || isProcessing}>
          {isProcessing ? <StopIcon /> : <SendIcon />}
        </button>
      </div>
    </div>
  );
}
```

### 6.7 StatusBar（状态栏）

```tsx
function StatusBar() {
  return (
    <footer className="status-bar">
      {/* Provider & Model */}
      <Badge>{providerName} / {modelName}</Badge>

      {/* Mode */}
      <ModeSelector modes={['Agent', 'Plan', 'Flow', 'YOLO']} active={currentMode} onChange={switchMode} />

      {/* Connection Status */}
      <ConnectionDot status={connectionStatus} />

      {/* Status Text */}
      <span>{statusText}</span>

      {/* Token Usage */}
      <TokenDisplay input={tokenInput} output={tokenOutput} cost={cost} />

      {/* Context Usage Ring */}
      <ContextRing percentage={contextPct} />

      {/* Theme Toggle */}
      <ThemeToggle />
    </footer>
  );
}
```

### 6.8 CommandPalette（命令面板）

按 `/` 唤起，搜索过滤：

```tsx
const COMMANDS = [
  { id: 'new-session', label: 'New Session', shortcut: 'Ctrl+N', group: 'Sessions' },
  { id: 'clear', label: 'Clear Messages', group: 'Chat' },
  { id: 'mode-agent', label: 'Switch to Agent Mode', shortcut: 'Ctrl+1', group: 'Mode' },
  { id: 'mode-plan', label: 'Switch to Plan Mode', shortcut: 'Ctrl+2', group: 'Mode' },
  { id: 'mode-yolo', label: 'Switch to YOLO Mode', shortcut: 'Ctrl+3', group: 'Mode' },
  { id: 'theme-toggle', label: 'Toggle Theme', shortcut: 'Ctrl+T', group: 'Appearance' },
  // ... 20+ commands
];
```

---

## 七、Phase 3: 功能完善（预计 3-5 天）

### 7.1 Rust 后端增强

| 功能 | 新 Tauri Command | 说明 |
|------|-----------------|------|
| 流式输出 | `send_prompt_stream` | 改为逐个 token 发送 `agent-event` |
| 审批响应 | `respond_approval` | JS → Rust 传递 Y/N 决策 |
| Ask User 响应 | `respond_question` | JS → Rust 传递用户回答 |
| 会话 CRUD | `rename_session`, `delete_session`, `export_session` | 完善会话管理 |
| 文件树 | `list_workspace_files` | 返回项目文件树 |
| 后台任务 | `list_tasks`, `cancel_task` | 任务管理 |
| 模式切换 | `set_mode` | 运行时切换 AgentMode |
| 暂停/取消 | `cancel_run`, `pause_run` | Agent 执行控制 |

### 7.2 流式输出改造（Rust 端重点）

当前 `send_prompt` 一次性后台运行 Agent，前端只能等 Finished 事件。

**改进方案**：Agent 内部 `LlmProvider` 调用返回 `Stream`，EventBus 在每个 chunk 到达时立即 emit `LlmStreamChunk` 事件。Bridge 层逐条转发到前端。

```
Agent.think() → LLM.chat_stream() → for each chunk:
  → event_bus.emit(LlmStreamChunk { content: token })
  → bridge thread: app.emit("agent-event", { type: "thought", content: token })
  → JS: appendToStream(token)
```

### 7.3 文件树功能

通过 `list_workspace_files` command 返回项目目录树（JSON），前端渲染为可展开的文件树，支持：
- 右键菜单 "Add to Chat"（将文件路径加入对话上下文）
- 文件图标按类型区分

### 7.4 会话管理完善

- **SessionList 右侧按钮**：Rename / Delete / Export
- **搜索栏**：按名称过滤会话
- **持久化**：关闭应用后会话保留

---

## 八、Phase 4: 体验优化（预计 2-3 天）

### 8.1 独立创新功能

在原版基础上增加 Desktop 独有的体验：

1. **系统托盘 + 通知**：Agent 完成后系统通知
2. **窗口分屏**：可拖出独立的文件编辑器/终端窗口
3. **本地文件搜索**：集成 ripgrep 快速搜索项目文件
4. **全局快捷键**：`Alt+Space` 唤出迷你输入框
5. **多 Provider 实时切换**：切换后即时生效（不需要重启）

### 8.2 键盘快捷键体系

```
Enter          → 发送消息
Shift+Enter    → 换行
Ctrl+N         → 新建会话
Ctrl+L         → 清空消息
Ctrl+K         → 唤起命令面板
Ctrl+1~4       → 切换 Agent/Plan/Flow/YOLO 模式
Ctrl+T         → 切换主题
Ctrl+F         → 搜索消息
F3             → 下一条搜索结果
Esc            → 关闭弹窗/命令面板
Y/N            → 审批弹窗时快捷批准/拒绝
```

### 8.3 性能优化

- 虚拟滚动（消息列表 > 1000 条时）
- 会话列表分页
- 大文件 diff 预览分段加载
- Rust 端 SQLite 查询优化

---

## 九、实施优先级

### 第一阶段：立即可用（Phase 1）

| 序号 | 任务 | 优先级 | 状态 |
|------|------|--------|------|
| P1 | 修复 EventType 字段名不匹配 | 🔴 P0 | 待实现 |
| P1 | 修复 data 字段格式，正确解析 | 🔴 P0 | 待实现 |
| P2 | 添加流式消息累积渲染 | 🟡 P1 | 待实现 |
| P3 | 添加基本 Markdown 渲染 | 🟡 P1 | 待实现 |
| P4 | 添加审批弹窗 UI | 🟡 P1 | 待实现 |
| P5 | 添加工具调用展开/折叠 | 🟢 P2 | 待实现 |

### 第二阶段：基准体验（Phase 2）

| 序号 | 任务 | 优先级 | 状态 |
|------|------|--------|------|
| P6 | 创建 Vite + React 前端项目 | 🔴 P0 | 待实现 |
| P7 | 实现 AppLayout（侧边栏+主区+状态栏） | 🔴 P0 | 待实现 |
| P8 | 实现 ChatArea + MessageList | 🔴 P0 | 待实现 |
| P9 | 实现 ToolCallCard 组件 | 🔴 P0 | 待实现 |
| P10 | 实现 ApprovalDialog | 🔴 P0 | 待实现 |
| P11 | 实现 StatusBar | 🟡 P1 | 待实现 |
| P12 | 实现 SetupScreen（配置向导） | 🟡 P1 | 待实现 |
| P13 | 集成到 Tauri build 并测试 | 🔴 P0 | 待实现 |

### 第三阶段：功能完整（Phase 3）

| 序号 | 任务 | 优先级 | 状态 |
|------|------|--------|------|
| P14 | Rust: 流式输出改造 | 🔴 P0 | 待实现 |
| P15 | Rust: 审批响应 command | 🔴 P0 | 待实现 |
| P16 | Rust: 会话管理 CRUD commands | 🟡 P1 | 待实现 |
| P17 | Rust: 文件树 command | 🟡 P1 | 待实现 |
| P18 | SessionList 组件 | 🟡 P1 | 待实现 |
| P19 | CommandPalette 组件 | 🟢 P2 | 待实现 |
| P20 | FileTree 组件 | 🟢 P2 | 待实现 |

### 第四阶段：体验加分（Phase 4）

| 序号 | 任务 | 优先级 | 状态 |
|------|------|--------|------|
| P21 | 系统通知 | 🟢 P2 | 待实现 |
| P22 | 全局快捷键 | 🟢 P2 | 待实现 |
| P23 | 虚拟滚动性能优化 | 🟢 P2 | 待实现 |
| P24 | 更多创新功能 | 🟢 P2 | 待实现 |

---

## 十、风险与注意点

1. **Tauri v2 API 兼容性**：`@tauri-apps/api` v2 的 event API 可能与 v1 不同，需确认 `listen()` 签名
2. **Appropriate `withGlobalTauri`**：确保 JS 端能访问 `window.__TAURI__`
3. **CORS 限制**：WebView 内 CDN 资源可能被 CSP 拦截，Markdown 库应本地打包
4. **构建链路**：`frontend/` 应在 `src/` 同级，`tauri.conf.json` 中 `beforeBuildCommand` 要能正常运行 npm
5. **TypeScript 编译**：`beforeBuildCommand` 中需包含 `tsc` 类型检查
6. **React 版本**：使用 React 18（而非 19），与 Tauri 生态兼容性更好

---

## 附录 A: 当前代码 Bug 清单

| 文件 | 行号 | 问题 | 严重度 |
|------|------|------|--------|
| `lib.rs` | 71-75 | 事件 format 为 Debug 格式（如 `Started`）而非 snake_case | 🔴 |
| `lib.rs` | 74 | `event.data` 为 Value 直接序列化，未扁平化 | 🔴 |
| `index.html` | 139 | `ae.type` 应为 `payload.eventType` 或 `payload.type` | 🔴 |
| `index.html` | 143-150 | 所有 `case 'xxx'` 值与 Rust Debug 格式不匹配 | 🔴 |
| `index.html` | 141 | `ae.content` 不存在于当前 payload 结构中 | 🔴 |
| `commands.rs` | 68-73 | `send_prompt` 无流式支持，前端只能等 Finished | 🟡 |
| `commands.rs` | 144-169 | `configure_provider` 保存后需重启，体验差 | 🟢 |
| `index.html` | 整体 | 单文件架构不可维护 | 🟡 |

## 附录 B: 参考文件路径

- 原 Goat Web 前端：`E:\WorkBuddyOutput\2026-06-25-10-54-30\my-tui-main\frontend\src\`
- 当前 Desktop 前端：`E:\WorkBuddyOutput\2026-06-25-10-54-30\goat_rust\rgoat-desktop\dist\index.html`
- Rust 事件桥接：`E:\WorkBuddyOutput\2026-06-25-10-54-30\goat_rust\rgoat-desktop\src\lib.rs`
- Rust IPC 命令：`E:\WorkBuddyOutput\2026-06-25-10-54-30\goat_rust\rgoat-desktop\src\commands.rs`
- EventBus 类型：`E:\WorkBuddyOutput\2026-06-25-10-54-30\goat_rust\rgoat-core\src\core\event_bus.rs`
- Agent 主循环：`E:\WorkBuddyOutput\2026-06-25-10-54-30\goat_rust\rgoat-core\src\agent\react.rs`
