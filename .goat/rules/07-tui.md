# TUI 界面

## 组件结构

```
TuiApp (Textual App)
├── Header              — 标题栏 (Provider / 模型 / 状态)
├── ChatPanel           — 对话消息流 (RichLog)
├── InputPanel          — 输入区域 (TextArea)
├── SidePanel           — 侧边面板 (Agent 列表 / 帮助)
├── StatusBar           — 底部状态栏
└── CommandMenu         — 命令选择浮层
```

## 交互流程

1. 用户输入 → `user_input_queue` → `_process_input_loop`
2. LLM 流式输出 → `EventBus.LLM_STREAM` → `bridge.py` → `TUIState.streaming_content`
3. 输出完成 → `EventBus.LLM_RESPONSE` → `ChatPanel` 追加消息
4. 审批请求 → `approval_request_queue` → 内联审批 UI

## 状态模型

- `TUIState` 集中管理所有响应式状态
- `MessageRole` 枚举: USER / ASSISTANT / TOOL_CALL / TOOL_RESULT / ERROR / SYSTEM
- `MessageData` 含 role / content / agent_name / timestamp
- Bridge 层隔离 EventBus 和 TUI 组件，防止直接耦合

## 主题

- 红黑主题 (`RED_BLACK_THEME`) 为默认
- 颜色定义在 `theme.py` 中，使用 Textual CSS
- 组件不硬编码颜色值，使用主题变量