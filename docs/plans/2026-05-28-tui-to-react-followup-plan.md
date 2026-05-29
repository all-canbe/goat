# TUI → React 后续迁移方案

> 基于当前已完成的基础迁移（FastAPI 后端 + React 前端骨架），规划后续 4 个阶段的迁移内容。

---

## 当前已完成的迁移基础

### 后端已就绪
| 组件 | 状态 |
|------|------|
| FastAPI 服务器 + CORS + 静态文件服务 | ✅ |
| WebSocket 连接管理（WSManager） | ✅ |
| REST API 路由（sessions、health） | ✅ 但返回 stub 数据 |
| EventBus → WebSocket 事件桥接 | ✅ LLM 流式/响应/错误/完成 |
| `goat web` 命令 | ✅ |

### 前端已就绪
| 组件 | 状态 |
|------|------|
| Vite + React 18 + TypeScript | ✅ |
| Tailwind CSS 黑红主题 | ✅ |
| Zustand Stores（chat/session/connection） | ✅ |
| WSClient（自动重连、消息分发） | ✅ |
| AppLayout + Sidebar + StatusBar | ✅ |
| ChatArea + MessageList + 消息组件 | ✅ |
| InputPanel（原生 textarea、IME 完美） | ✅ |
| SessionList + SessionItem | ✅ |
| ToolCallCard（折叠参数/结果） | ✅ |

### 待迁移的核心功能

基于 TUI 分析，以下功能尚未迁移：

**审批系统**：PermissionMode（plan/agent/yolo）、工具调用审批、规则引擎
**模式切换**：Tab 键循环切换模式（Plan→Agent→Yolo→Flow）
**命令系统**：`/` 触发命令菜单，含 30+ 条命令
**MCP 配置**：MCP 服务器 CRUD、JSON 配置管理
**文件编辑**：内联文件编辑面板（保存/取消）
**搜索功能**：Ctrl+F 搜索消息历史，高亮
**子 Agent 面板**：侧边栏显示子 Agent 状态、后台任务
**文件树**：工作区文件浏览
**Provider 配置**：模型/API Key 管理
**审批对话框**：工具调用需要用户批准时的弹窗

---

## 阶段五：核心交互链路（P0）

> 目标：让 Web 前端能完整运行一个对话流程，包括权限控制、模式切换、工具审批。

### Task 5.1：WebSocket 协议扩展

**后端修改**：
- [ ] `my_tui/api/websocket.py` — 新增事件类型支持：
  - `mode.change` → 前端切换权限模式（plan/agent/yolo）
  - `tool.require_approval` → 需要用户审批
  - `config.update` → Provider/模型配置变更
  - `session.switch` → 切换活跃会话
- [ ] 新增 `status.update` 完整 payload：`{ model, provider, mode, tokenCount, toolRunning }`

**前端修改**：
- [ ] `types/index.ts` — 扩展 WSMessage 类型，新增 `PermissionMode`
- [ ] `connectionStore.ts` — 新增 `mode` 字段

### Task 5.2：权限模式切换

**后端**：
- [ ] `my_tui/api/websocket.py` — 处理 `mode.change` 消息
- [ ] 桥接 `TUIBridge.set_mode()` 方法

**前端**：
- [ ] `StatusBar.tsx` — 显示当前模式标签（Plan/Agent/Yolo），可点击切换
- [ ] 模式颜色：Plan=橙色、Agent=红色、Yolo=亮红
- [ ] Tab 键切换模式（与 Enter 不冲突）

### Task 5.3：工具调用审批流

**后端**：
- [ ] `my_tui/api/websocket.py` — 新增事件 `tool.require_approval`
  - payload: `{ toolCallId, toolName, args, description }`
- [ ] 新增处理 `tool.approve` / `tool.reject`（已存在 stub）
- [ ] 桥接 `ApprovalPipeline` 审批管道

**前端**：
- [ ] `ApprovalDialog.tsx` — 审批对话框组件
  - 显示工具名、参数、安全评分
  - 批准/拒绝按钮，红色强调
  - 支持记住选择
- [ ] `stores/chatStore.ts` — 新增 `pendingApprovals` 状态
- [ ] Modal 弹窗，使用 shadcn/ui Dialog 组件

### Task 5.4：配置面板

**后端**：
- [ ] `my_tui/api/routes.py` — 完善 `GET/POST /api/config`
- [ ] 返回真实配置：Provider、模型、API 端点等

**前端**：
- [ ] `stores/configStore.ts` — 新增配置状态
  - `model`, `provider`, `apiEndpoint`, `mode`
- [ ] `SettingsPanel.tsx` — 配置面板组件
  - Provider 下拉选择
  - 模型输入
  - 保存/重置按钮

### Task 5.5：会话管理完善

**后端**：
- [ ] `my_tui/api/routes.py` — 替换 stub，对接 `ConversationManager`
- [ ] `GET /api/sessions` → 返回真实会话列表
- [ ] `POST /api/sessions` → 创建真实会话
- [ ] `GET /api/sessions/{id}/messages` → 返回真实历史消息

**前端**：
- [ ] `sessionStore.ts` — 完善状态同步
- [ ] 创建新会话时清空聊天区域
- [ ] 切换会话时加载历史消息

---

## 阶段六：配置与 MCP（P1）

> 目标：支持 MCP 服务器配置、Provider 管理、系统设置。

### Task 6.1：MCP 配置界面

**后端**：
- [ ] `my_tui/api/routes.py` — 新增 MCP 配置 REST API：
  - `GET /api/mcp/servers` — 列出 MCP 服务器
  - `POST /api/mcp/servers` — 添加 MCP 服务器
  - `PUT /api/mcp/servers/{name}` — 更新 MCP 服务器
  - `DELETE /api/mcp/servers/{name}` — 删除 MCP 服务器
- [ ] 桥接 `mcp_config.py` 的 `load_config()` / `save_config()`

**前端**：
- [ ] `stores/mcpStore.ts` — MCP 配置状态
  - `servers: McpServer[]`
  - `addServer()`, `updateServer()`, `removeServer()`
- [ ] `McpConfigPage.tsx` — MCP 配置页面
  - 服务器列表（卡片式）
  - 新增/编辑表单（名称、命令、参数、环境变量）
  - 连接测试按钮
  - 删除确认对话框
- [ ] Tailwind 黑红样式表单

### Task 6.2：Provider 配置管理

**后端**：
- [ ] `my_tui/api/routes.py` — Provider 配置 API：
  - `GET /api/providers` — 列出可用 Provider
  - `POST /api/providers` — 配置 Provider
- [ ] 桥接 `provider.py` 的配置管理

**前端**：
- [ ] `ProviderConfig.tsx` — Provider 配置组件
  - Provider 选择（OpenAI, Anthropic, 本地等）
  - API Key 输入（密码模式）
  - 模型名称输入
  - 端点 URL 输入

---

## 阶段七：高级功能（P1）

> 目标：功能完整性与 TUI 对齐。

### Task 7.1：搜索功能

**TUI 参考**：`search_bar.py`、`chat_panel.py` 的 toggle_search

**前端**：
- [ ] `SearchBar.tsx` — 搜索栏组件
  - Ctrl+F 触发（覆盖浏览器默认行为）
  - 输入框 + 搜索结果计数
  - Enter 跳到下一个，Shift+Enter 跳到上一个
  - 当前匹配项高亮（红色背景）
  - Esc 关闭搜索
- [ ] 消息列表支持搜索高亮
- [ ] `stores/chatStore.ts` — 新增搜索相关状态

### Task 7.2：文件编辑面板

**TUI 参考**：`edit_panel.py`

**前端**：
- [ ] `FileEditor.tsx` — 文件编辑组件
  - 代码编辑器（textarea + monospace 字体）
  - 文件路径显示
  - 保存/取消按钮
  - 保存时通过 WebSocket 发送 `file.save`
  - 取消时确认丢弃更改
- [ ] Modal 或 Sidebar 模式展示

### Task 7.3：命令菜单

**TUI 参考**：`command_menu.py`

**前端**：
- [ ] `CommandPalette.tsx` — 命令面板组件
  - `/` 触发（类似 VSCode Command Palette）
  - 模糊搜索过滤命令
  - 命令分类：模式切换、文件操作、会话管理、技能管理、MCP 管理
  - 选中执行命令（通过 WebSocket 或本地操作）
- [ ] 使用 shadcn/ui Command 组件

### Task 7.4：子 Agent 状态面板

**TUI 参考**：`side_panel.py`

**前端**：
- [ ] Sidebar 扩展：新增子 Agent 状态区域
  - 活跃子 Agent 列表
  - 每个 Agent 显示名称、状态、进度
  - 可点击展开详情
- [ ] 后台任务列表（completed、running）
- [ ] WebSocket 事件：`subagent.start`, `subagent.complete`, `subagent.progress`

### Task 7.5：文件树浏览

**TUI 参考**：`side_panel.py` 的文件树部分

**前端**：
- [ ] `FileTree.tsx` — 文件树组件
  - Sidebar 底部区域
  - 展开/折叠目录
  - 点击文件在编辑器中打开
  - 右键菜单（新建、删除、重命名）
- [ ] WebSocket 事件：`file.tree`（请求文件树）、`file.select`

---

## 阶段八：优化与完善（P2）

> 目标：打磨体验，补齐细节。

### Task 8.1：快捷键系统

| 快捷键 | 功能 |
|--------|------|
| `Tab` | 循环切换权限模式 |
| `Ctrl+C` | 取消流式生成 |
| `Ctrl+F` | 搜索 |
| `Ctrl+R` | 重新生成 |
| `Ctrl+Shift+F` | 全屏切换 |
| `F3` / `Shift+F3` | 搜索下一个/上一个 |
| `Esc` | 关闭弹窗/取消 |
| `Y` / `N` | 审批对话框确认/拒绝 |
| `Ctrl+T` | 新建会话 |

### Task 8.2：Markdown 渲染优化

- [ ] 代码语法高亮（rehype-highlight 主题匹配黑红）
- [ ] Diff 渲染（+绿色/-红色）
- [ ] 表格样式
  - [ ] 自动换行/滚动
- [ ] 图片渲染（base64 或 URL）
- [ ] 数学公式（可选项）

### Task 8.3：主题切换

- [ ] Tailwind CSS 暗色/亮色双主题
- [ ] CSS 变量切换
- [ ] localStorage 持久化
- [ ] StatusBar 切换按钮

### Task 8.4：Token 统计展示

- [ ] `StatusBar` 扩展：显示 Token 消耗、上下文使用率
- [ ] 进度条组件表示上下文窗口利用率
- [ ] WebSocket 事件：`token.update`

### Task 8.5：UI 动效

- [ ] 消息列表淡入动画
- [ ] 工具卡片展开/折叠动画
- [ ] 模式切换过渡
- [ ] 代码块复制按钮
- [ ] 文本选中样式（红色调，与 TUI 一致）

---

## 推荐优先级

```
▸▸▸▸▸ 立即开始 ▸▸▸▸▸

阶段五：核心交互链路
  ├── Task 5.3 工具审批流 ← 关键：让对话能走通
  ├── Task 5.1 WebSocket 协议扩展 ← 基础
  ├── Task 5.2 权限模式切换 ← 基础
  ├── Task 5.5 会话管理完善 ← 基础
  └── Task 5.4 配置面板 ← 基础

▸▸▸▸▸ 接下来 ▸▸▸▸▸

阶段六：配置与 MCP
  ├── Task 6.1 MCP 配置界面 ← 高频使用
  └── Task 6.2 Provider 配置管理

阶段七：高级功能
  ├── Task 7.1 搜索功能
  ├── Task 7.2 文件编辑
  ├── Task 7.3 命令菜单
  ├── Task 7.4 子 Agent 面板
  └── Task 7.5 文件树

▸▸▸▸▸ 最后打磨 ▸▸▸▸▸

阶段八：优化完善
  ├── Task 8.1 快捷键
  ├── Task 8.2 Markdown 渲染
  ├── Task 8.3 主题切换
  ├── Task 8.4 Token 统计
  └── Task 8.5 动效
```

---

## 数据流交互图

```
┌─────────────────────────────────────────────────────────┐
│                        前端 (React)                       │
│                                                          │
│  InputPanel  ──chat.send──┐                              │
│  ModeSwitch  ──mode.change┤                              │
│  ApprovalDlg ──tool.approval├─ WSClient ──WebSocket──→   │
│  SearchBar   ──search.next─┘                              │
│                                                          │
│  ┌─ status.update ←──┐                                  │
│  │  chat.stream  ←───┤                                  │
│  │  chat.response ←──┤  WSClient ←──WebSocket──         │
│  │  tool.*       ←───┘                                  │
│  └─────────────────┘                                      │
└─────────────────────┬─────────────────────────────────────┘
                      │ WebSocket
                      ▼
┌─────────────────────────────────────────────────────────┐
│                   后端 (FastAPI)                          │
│                                                          │
│  WSManager.handle_message()                              │
│    ├── chat.send    → EventBus ──→ ConversationManager   │
│    ├── mode.change  → EventBus ──→ TUIBridge.set_mode()  │
│    └── tool.approve → EventBus ──→ ApprovalPipeline      │
│                                                          │
│  EventBusBridge._poll_loop()                             │
│    ├── LLM_STREAM   ──→ ws.broadcast({chat.stream})      │
│    ├── TOOL_START   ──→ ws.broadcast({tool.start})       │
│    └── ASK_USER     ──→ ws.broadcast({tool.require...})  │
│                                                          │
│  REST API:                                               │
│    GET /api/sessions    ──→ ConversationManager          │
│    GET /api/config      ──→ ProviderConfig               │
│    GET /api/mcp/servers ──→ MCPConfig                    │
└─────────────────────────────────────────────────────────┘
```