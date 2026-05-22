# 架构规则

## 模块职责

```
my_tui/
├── agent/       — Agent 循环、SubAgent 管理、角色系统、Skill 注册
├── conversation — 对话持久化、上下文压缩、提示词渲染
├── core/        — 取消令牌、事件总线、Token 跟踪
├── hooks/       — 生命周期钩子系统
├── provider/    — LLM Provider 抽象与工厂
├── security/    — 工具审批、沙箱、ML 分类
├── tasks/       — 持久化后台任务
├── tools/       — 内置工具实现 (文件/Git/Shell/Web)
├── tui/         — Textual TUI 界面
```

## 依赖方向

```
tui_main / main
  → conversation, agent, tools, security, provider, tasks, core
  → tui (仅 tui_main)
agent
  → conversation (prompt_engine), tools (tools), security (approval), core (cancellation, event_bus)
tools       → core (cancellation)
conversation → core (event_bus), conversation (prompt_templates)
hooks       → 无内部依赖 (独立生命周期层)
```

禁止反向依赖：tools 不引用 agent，tui 不引用 agent。

## 初始化顺序

1. ProviderConfig → create_llm
2. EventBus
3. ConversationManager (创建默认会话)
4. SubAgentManager
5. SkillRegistry (加载 skills/ 或回退内置)
6. TokenTracker
7. ToolApprovalSystem
8. DurableTaskManager (注册任务类型 → start → recover)