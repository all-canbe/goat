# Agent / SubAgent 系统

## 角色系统

```
general     — 通用助手 (可 spawn 子 Agent)
explore     — 代码探索 (只读工具)
plan        — 任务规划 (只读 + write)
implementer — 代码实现 (读写 + 命令)
review      — 代码审查 (只读)
verifier    — 测试验证 (只读 + 命令)
custom      — 自定义
```

- 角色定义在 `subagent_roles.py` 的 `ROLE_REGISTRY` 字典中
- `RoleType` 枚举值作为角色唯一标识
- `RoleDefinition` 含 `allowed_tools` + `can_spawn` + `max_spawn_depth`
- `can_spawn=True` 的角色绑 `_build_subagent_tools`

## Agent 循环

- `_run_agent_loop` 在 `subagent_runtime.py` 中
- 固定 `MAX_AGENT_TURNS` 轮次上限
- 每轮: LLM 调用 → tool_calls → 审批 → 执行 → ToolMessage 返回
- `CancellationToken` 跨所有层级传递以实现级联取消

## SubAgent 管理

- `SubAgentManager` 管理全部 SubAgent 生命周期
- 并发上限 `max_concurrent`，嵌套深度上限 `max_spawn_depth`
- SubAgent 结果通过 `collect_results` 汇总
- 级联取消: 取消父 Agent 时自动取消所有子孙

## 上下文传递

- `AgentContext` 承载 agent_id / llm / tools / skill_registry / approval_system 等
- 主 Agent 用 `main_agent_id = "main"` 标识
- SubAgent 使用 `prompt_engine.render_system(role_type.value)` 作为系统提示