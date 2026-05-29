# 架构规则

## 模块职责

```
my_tui/
├── agent/       — Agent 循环、SubAgent 管理、角色系统、Skill 注册、Flow 流水线
├── api/         — FastAPI 路由、WebSocket 管理、Chat Handler
├── audit/       — 审计日志管理
├── browser/     — Playwright 浏览器自动化
├── conversation — 对话持久化、上下文压缩、提示词渲染
├── core/        — 取消令牌、事件总线、Token 跟踪、工作空间
├── hooks/       — 生命周期钩子系统
├── mcp/         — MCP 客户端/服务器、工具桥接、SSE 服务器
├── memory/      — 跨会话持久化记忆
├── provider/    — LLM Provider 抽象与工厂
├── security/    — 工具审批、沙箱、ML 分类
├── tasks/       — 持久化后台任务
├── tools/       — 内置工具实现 (文件/Git/Shell/Web/Browser)
└── tui/         — Textual TUI 界面

frontend/
├── components/  — React UI 组件 (聊天/布局/编辑器/搜索)
├── hooks/       — React Hooks (WebSocket/键盘/自动滚动)
├── stores/      — Zustand 状态仓库
└── lib/         — WebSocket 客户端封装
```

## 依赖方向

```
tui_main / main
  → conversation, agent, tools, security, provider, tasks, core
  → tui (仅 tui_main)
agent
  → conversation (prompt_engine), tools (tools), security (approval), core (cancellation, event_bus)
api         → conversation, provider, mcp, core (event_bus)
mcp         → tools, core
tools       → core (cancellation)
conversation → core (event_bus), conversation (prompt_templates)
hooks       → 无内部依赖 (独立生命周期层)
audit       → 无内部依赖
browser     → 无内部依赖
memory      → core
tasks       → core
provider    → 无内部依赖
frontend    → api (通过 WebSocket / REST)
```

禁止反向依赖：tools 不引用 agent，tui 不引用 agent，api 不引用 agent。

## 初始化顺序

### CLI / TUI 模式

1. ProviderConfig → create_llm
2. EventBus
3. ConversationManager (创建默认会话)
4. SubAgentManager
5. SkillRegistry (加载 skills/ 或回退内置)
6. TokenTracker
7. ToolApprovalSystem
8. DurableTaskManager (注册任务类型 → start → recover)
9. ModelRouter (TUI 模式额外步骤)
10. HookLifecycleSystem
11. FlowPipeline

### Web 模式

1. FastAPI app 创建 (CORS + Router)
2. web_chat_handler.initialize() (Provider + Conversation + Agent)
3. create_event_bridge (EventBus → WebSocket 广播)
4. ws_manager.set_chat_handler
5. 静态前端挂载 (frontend/dist)