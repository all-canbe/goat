# TUI → React Web 前端迁移设计方案

## 1. 背景与动机

### 1.1 当前问题

当前项目使用 Python Textual 框架构建 TUI（终端用户界面），存在以下核心问题：

- **IME 输入不稳定**：Textual 8.2.7 在 Windows Terminal 中无法正确处理中文输入法（IME），composition 字符和 commit 字符都作为普通 Key 事件到达，导致中文输入出现拼音残留
- **终端兼容性差**：不同终端模拟器（Windows Terminal、WezTerm、Alacritty 等）对 IME 的处理方式不同，无法保证一致体验
- **Textual 无 IME 支持**：框架层面没有 IME composition 事件，无法在应用层可靠解决

### 1.2 迁移目标

将 TUI 替换为 React 18+ Web 前端，利用浏览器原生优异的 IME 支持和丰富的 UI 能力，提供稳定的图形化使用体验。

### 1.3 核心原则

- **后端不变**：Python 后端（EventBus、ConversationManager、LLM 调用、MCP 集成、工具系统、安全沙箱）全部保留
- **前端只做展示**：React 前端仅负责 UI 渲染和用户交互，业务逻辑全在后端
- **CLI 保持不变**：`goat cli` 命令继续可用，与 Web 前端并行
- **体验升级**：Web 前端作为主要使用模式，图形化体验优于终端

---

## 2. 技术栈

### 2.1 后端（新增部分）

| 组件 | 选型 | 理由 |
|------|------|------|
| Web 框架 | FastAPI | 异步原生支持，WebSocket 成熟，自动 API 文档 |
| WebSocket | fastapi.WebSocket | 原生集成，支持双向流式通信 |
| 静态文件服务 | FastAPI StaticFiles | 直接 serve 前端构建产物 |

### 2.2 前端（全新）

| 组件 | 选型 | 理由 |
|------|------|------|
| 构建工具 | Vite 5 | 快速 HMR，TypeScript 原生支持 |
| UI 框架 | React 18 | 生态成熟，组件化开发 |
| 语言 | TypeScript 5 | 类型安全 |
| 样式 | Tailwind CSS 3 | 原子化 CSS，与黑红主题完美配合 |
| 组件库 | shadcn/ui + Radix | 基于 Tailwind 的无障碍组件，代码可控 |
| 状态管理 | Zustand | 轻量、简洁、TypeScript 友好 |
| Markdown 渲染 | react-markdown + rehype-highlight | 代码高亮，支持流式渲染 |
| WebSocket 客户端 | 原生 WebSocket API | 浏览器内置，无需额外依赖 |

---

## 3. 项目结构

```
my-tui-main/
├── my_tui/                    # Python 后端
│   ├── api/                   # 新增：FastAPI 后端
│   │   ├── __init__.py
│   │   ├── server.py          # FastAPI app 实例，路由注册，静态文件服务
│   │   ├── websocket.py       # WebSocket 连接管理，消息路由
│   │   └── routes.py          # REST API 路由
│   ├── core/                  # 现有：EventBus 等
│   ├── conversation/          # 现有：会话管理
│   ├── agent/                 # 现有：技能系统
│   ├── tools/                 # 现有：工具系统
│   ├── tui/                   # 现有：TUI 代码（保留，CLI 使用）
│   └── ...
├── frontend/                  # 新增：React 前端
│   ├── public/
│   ├── src/
│   │   ├── components/
│   │   │   ├── layout/
│   │   │   │   ├── AppLayout.tsx
│   │   │   │   ├── Sidebar.tsx
│   │   │   │   └── StatusBar.tsx
│   │   │   ├── chat/
│   │   │   │   ├── ChatArea.tsx
│   │   │   │   ├── MessageList.tsx
│   │   │   │   ├── UserMessage.tsx
│   │   │   │   ├── AssistantMessage.tsx
│   │   │   │   ├── ToolCallCard.tsx
│   │   │   │   └── StreamingBubble.tsx
│   │   │   ├── input/
│   │   │   │   └── InputPanel.tsx
│   │   │   ├── session/
│   │   │   │   ├── SessionList.tsx
│   │   │   │   └── SessionItem.tsx
│   │   │   └── ui/            # shadcn/ui 组件
│   │   ├── stores/
│   │   │   ├── chatStore.ts
│   │   │   ├── sessionStore.ts
│   │   │   ├── configStore.ts
│   │   │   └── connectionStore.ts
│   │   ├── hooks/
│   │   │   ├── useWebSocket.ts
│   │   │   └── useAutoScroll.ts
│   │   ├── lib/
│   │   │   └── ws-client.ts    # WebSocket 客户端封装
│   │   ├── types/
│   │   │   └── index.ts        # 共享类型定义
│   │   ├── App.tsx
│   │   ├── main.tsx
│   │   └── index.css           # Tailwind + 自定义主题
│   ├── package.json
│   ├── tailwind.config.ts
│   ├── tsconfig.json
│   ├── vite.config.ts
│   └── components.json         # shadcn/ui 配置
├── pyproject.toml
└── main.py
```

---

## 4. 启动方式

### 4.1 用户命令

```bash
# Web 前端模式（主要使用模式）
goat web
# → FastAPI 启动 → serve frontend/dist/ → 自动打开浏览器

# CLI 模式（保持不变）
goat cli
# → 原始 TUI 模式
```

### 4.2 前端构建与集成

```
前端开发时：  cd frontend && npm run dev     → Vite dev server (localhost:5173)
前端构建时：  cd frontend && npm run build   → 输出到 frontend/dist/
打包发布时：  pip install 时自动构建前端，或预构建 frontend/dist/ 打入包中
运行时：     FastAPI mount frontend/dist/ 为静态文件，goat web 一键启动
```

---

## 5. API 协议设计

### 5.1 WebSocket 端点

```
ws://localhost:8000/ws
```

### 5.2 消息格式

所有消息为 JSON，结构如下：

```typescript
interface WSMessage {
  type: string;
  payload: unknown;
  timestamp?: number;
}
```

### 5.3 后端 → 前端事件

| type | payload | 说明 |
|------|---------|------|
| `chat.stream` | `{ token: string, messageId: string }` | LLM 流式 token |
| `chat.response` | `{ messageId: string, content: string }` | LLM 完整响应 |
| `chat.error` | `{ messageId: string, error: string }` | 错误信息 |
| `tool.start` | `{ toolName: string, args: object }` | 工具调用开始 |
| `tool.progress` | `{ toolName: string, data: string }` | 工具调用进度 |
| `tool.complete` | `{ toolName: string, result: string }` | 工具调用完成 |
| `tool.require_approval` | `{ toolName: string, args: object }` | 需要用户审批 |
| `status.update` | `{ model: string, provider: string, mode: string }` | 状态更新 |
| `system.notify` | `{ level: string, message: string }` | 系统通知 |

### 5.4 前端 → 后端操作

| type | payload | 说明 |
|------|---------|------|
| `chat.send` | `{ text: string, sessionId: string }` | 发送消息 |
| `chat.cancel` | `{}` | 取消当前生成 |
| `tool.approve` | `{ toolCallId: string }` | 批准工具调用 |
| `tool.reject` | `{ toolCallId: string }` | 拒绝工具调用 |

### 5.5 REST API

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/sessions` | 获取会话列表 |
| POST | `/api/sessions` | 新建会话 |
| DELETE | `/api/sessions/{id}` | 删除会话 |
| GET | `/api/sessions/{id}/messages` | 获取会话历史消息 |
| GET | `/api/config` | 获取当前配置 |
| POST | `/api/config` | 更新配置 |
| GET | `/api/health` | 健康检查 |

---

## 6. 前端组件设计

### 6.1 组件树

```
App
├── AppLayout
│   ├── Sidebar
│   │   ├── AppLogo (goat 图标 + 标题)
│   │   ├── SessionList
│   │   │   └── SessionItem (×N)
│   │   └── NewSessionButton
│   ├── ChatArea
│   │   ├── MessageList
│   │   │   ├── UserMessage
│   │   │   ├── AssistantMessage (Markdown 渲染)
│   │   │   ├── ToolCallCard
│   │   │   │   ├── ToolHeader (工具名 + 状态)
│   │   │   │   ├── ToolArgs (折叠的参数)
│   │   │   │   └── ToolResult (折叠的结果)
│   │   │   └── StreamingBubble
│   │   └── InputPanel
│   │       ├── TextArea (原生 textarea，中文 IME 完美支持)
│   │       ├── AttachmentButton
│   │       └── SendButton
│   └── StatusBar
│       ├── ModelInfo
│       ├── ModeIndicator (Plan / Agent / YOLO)
│       ├── TokenCount
│       └── ConnectionStatus
```

### 6.2 关键组件职责

#### Sidebar
- 显示会话列表，支持切换/新建/删除
- 宽度可折叠，默认 260px
- 活跃会话红色高亮

#### ChatArea
- 消息列表，自动滚动到底部
- 支持 Markdown 渲染（代码高亮、表格、列表）
- 流式输出时显示打字光标动画
- 工具调用以卡片形式展示，可折叠

#### InputPanel
- **原生 `<textarea>` 组件**，浏览器天然支持中文 IME，无需任何额外处理
- `Shift+Enter` 换行，`Enter` 发送
- 支持粘贴文本和文件
- 发送按钮在流式输出时变为取消按钮

#### StatusBar
- 显示当前模型、Provider、权限模式
- Token 使用统计
- WebSocket 连接状态指示器

### 6.3 Zustand Store 设计

```typescript
// chatStore.ts
interface ChatStore {
  messages: Message[];
  streamingContent: string;
  isStreaming: boolean;
  addMessage: (msg: Message) => void;
  appendStreamToken: (token: string) => void;
  clearStream: () => void;
}

// sessionStore.ts
interface SessionStore {
  sessions: Session[];
  activeSessionId: string | null;
  setActiveSession: (id: string) => void;
  createSession: () => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  loadSessions: () => Promise<void>;
}

// configStore.ts
interface ConfigStore {
  model: string;
  provider: string;
  mode: 'plan' | 'agent' | 'yolo';
  // ... 其他配置
}

// connectionStore.ts
interface ConnectionStore {
  status: 'connecting' | 'connected' | 'disconnected';
  reconnect: () => void;
}
```

---

## 7. 数据流

### 7.1 发送消息流程

```
用户输入 → InputPanel.onSubmit()
  → chatStore.isStreaming = true
  → wsClient.send({ type: "chat.send", payload: { text, sessionId } })
  → Python WebSocket Handler
    → EventBus → ConversationManager → PromptEngine → LLM
    → 流式返回 → ws.send({ type: "chat.stream", payload: { token } })
  → 前端 ws.onmessage
    → chatStore.appendStreamToken(token)
    → MessageList 实时更新
  → LLM 完成
    → ws.send({ type: "chat.response", payload: { messageId, content } })
  → 前端 ws.onmessage
    → chatStore.addMessage(assistantMsg)
    → chatStore.isStreaming = false
```

### 7.2 工具调用流程

```
LLM 发起工具调用
  → ws.send({ type: "tool.start", payload: { toolName, args } })
  → 前端显示 ToolCallCard (状态: 执行中)
  
  → 如需要审批:
    ws.send({ type: "tool.require_approval", payload: { ... } })
    → 前端显示审批对话框
    → 用户点击批准/拒绝
    → ws.send({ type: "tool.approve", payload: { toolCallId } })

  → 工具执行完毕:
    ws.send({ type: "tool.complete", payload: { toolName, result } })
    → 前端 ToolCallCard 更新 (状态: 完成，显示结果)
```

---

## 8. 视觉设计

### 8.1 配色方案

继承自 TUI 的 `RedBlackTheme`，通过 Tailwind CSS 自定义主题实现：

```typescript
// tailwind.config.ts
colors: {
  bg:           '#0a0a0a',  // 最深背景
  surface:      '#1a1a1a',  // 面板背景
  'surface-light':  '#2a2a2a',  // 悬浮背景
  'surface-lighter': '#3a3a3a', // 更亮背景
  primary:      '#cc0000',  // 主色：红
  'primary-light': '#ff1a1a', // 亮红
  'primary-dim': '#880000',   // 暗红
  'primary-glow': '#ff3333',  // 发光红
  text:         '#e0e0e0',  // 正文
  'text-dim':   '#888888',  // 辅助文字
  'text-darker': '#555555', // 更暗文字
  'text-bright': '#ffffff', // 高亮文字
  success:      '#00cc66',  // 成功绿
  warning:      '#ff6600',  // 警告橙
  error:        '#ff0000',  // 错误红
  border:       '#3a0a0a',  // 暗红边框
  'border-light': '#5a1a1a', // 亮红边框
  'border-focus': '#cc0000', // 聚焦边框（红）
  scrollbar:    '#4a0a0a',  // 滚动条
  'scrollbar-hover': '#6a1a1a', // 滚动条悬浮
  'selection-bg': '#330000', // 选中背景
  'selection-fg': '#ff6666', // 选中文字
  'mode-plan':  '#ff6600',  // 计划模式
  'mode-agent': '#cc0000',  // 代理模式
  'mode-yolo':  '#ff0000',  // YOLO 模式
}
```

### 8.2 设计风格

- **暗色终端美学**：深黑背景 + 暗红边框 + 发光效果
- **字体**：monospace（JetBrains Mono / Fira Code），保持终端感
- **边框**：暗红圆角边框，聚焦时红色发光
- **动画**：流式输出打字光标闪烁，消息淡入，工具卡片展开/折叠
- **响应式**：侧边栏可折叠，适配不同屏幕宽度

---

## 9. 实现计划

### 9.1 阶段一：后端 API 层

1. 创建 `my_tui/api/` 模块
2. 实现 FastAPI 应用初始化（`server.py`）
3. 实现 WebSocket 连接管理（`websocket.py`）
4. 实现 REST API 路由（`routes.py`）
5. 将 EventBus 事件桥接到 WebSocket 消息
6. 静态文件服务配置

### 9.2 阶段二：前端项目搭建

1. Vite + React + TypeScript 项目初始化
2. Tailwind CSS + shadcn/ui 配置
3. 黑红主题自定义
4. WebSocket 客户端封装
5. Zustand Store 实现

### 9.3 阶段三：核心组件开发

1. AppLayout、Sidebar、StatusBar 布局组件
2. ChatArea、MessageList、消息渲染组件
3. InputPanel 输入组件（原生 textarea）
4. SessionList 会话管理组件
5. ToolCallCard 工具调用卡片组件
6. StreamingBubble 流式输出组件

### 9.4 阶段四：集成与测试

1. 前后端联调
2. `goat web` 启动命令
3. 中文 IME 输入测试
4. 流式输出测试
5. 粘贴功能测试

### 9.5 阶段五：优化与后续

1. 命令菜单（`/` 触发）
2. 搜索栏（`Ctrl+F`）
3. 编辑面板（文件编辑）
4. MCP 配置界面
5. 主题切换（暗色/亮色）

---

## 10. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 前后端 WebSocket 断连 | 流式输出中断 | 自动重连 + 重连后恢复状态 |
| 长消息渲染性能 | 页面卡顿 | 虚拟滚动 + 消息分页加载 |
| 构建产物体积过大 | 加载慢 | 代码分割 + 懒加载 |
| 浏览器兼容性 | 部分用户无法使用 | 明确支持的浏览器版本 |

---

## 11. 成功标准

1. ✅ 中文 IME 输入正常，无拼音残留
2. ✅ 粘贴功能正常
3. ✅ 中英文混合输入正常
4. ✅ LLM 流式输出实时显示
5. ✅ 工具调用生命周期可视化
6. ✅ `goat web` 一键启动，自动打开浏览器
7. ✅ `goat cli` 命令不受影响
8. ✅ 现有 Python 测试全部通过