# 流式输出系统 (EventBus) + Skills 集成

## 变更内容

### 1. 新增 EventBus 事件总线
`subagent_demo/event_bus.py`

- **Pub/Sub 模式**：基于 `asyncio.Queue` 的发布/订阅事件系统
- **事件类型**：`STATUS_CHANGE` / `TOOL_CALL` / `TOOL_RESULT` / `LLM_RESPONSE` / `COMPLETED` / `ERROR`
- **过滤订阅**：支持按 `agent_id` 过滤，订阅所有 Agent 或指定 Agent
- **事件格式**：`SubagentEvent` 包含 `event_type`、`agent_id`、`agent_name`、`payload`、`depth`、`timestamp`

### 2. 子 Agent 执行循环集成事件发布
`subagent_demo/subagent_runtime.py`

- 新增 `_publish()` 辅助函数，简化事件发布
- 在以下关键节点发布事件：

| 节点 | 事件类型 |
|------|---------|
| 子 Agent 启动 | `STATUS_CHANGE` |
| LLM 返回内容 | `LLM_RESPONSE` |
| 调用工具 | `TOOL_CALL` |
| 工具返回结果 | `TOOL_RESULT` |
| 任务取消 | `STATUS_CHANGE` |
| 任务完成 | `COMPLETED` |
| 发生错误 | `ERROR` |

### 3. CLI 实时进度展示
`main.py`

- 新增 `_event_listener()` 后台 Task，监听所有事件并实时输出
- 事件展示格式：`  🔄 [Agent名称] 开始执行: 任务描述...`
- AgentContext 创建时全部传入 `event_bus`，确保所有子 Agent 事件可透传

### 4. Skills 系统集成到 Agent 提示词
`main.py`

- 新增 `_format_skills_prompt()` 方法，将 `SkillRegistry` 中的技能列表格式化为 system prompt
- 在 `_do_chat()` 的系统提示词末尾注入技能信息，使 LLM 了解可用技能
- 技能信息以易读的列表形式呈现（名称、描述、对应角色）
- 子 Agent 通过 `_run_agent_loop` 也可使用 `skill_registry`（context 已传入）

### 5. 取消提示词中的冗余数据
- 清理 `_show_status()` 中未使用的 `sessions` 变量

## 修改的文件

| 文件 | 变更 |
|------|------|
| `subagent_demo/event_bus.py` | **新增** — 事件总线核心模块 |
| `subagent_demo/subagent_runtime.py` | AgentContext 增加 event_bus 字段；_run_agent_loop 增加事件发布；_build_subagent_tools / run_subagent 透传 event_bus |
| `subagent_demo/__init__.py` | 导出 EventBus、EventType、SubagentEvent；修复 MAX_AGENT_TURNS 导出 |
| `main.py` | 创建 EventBus；启动事件监听 Task；_do_chat 传入 event_bus；_format_skills_prompt 注入技能信息；_do_spawn 透传 event_bus；_show_status 增加事件总线状态；_cleanup 清理监听 Task |

## 使用效果

启动后，当有子 Agent 运行时，CLI 会实时显示类似输出：

```
🤖 > 分析这个项目

🤖 主 Agent 正在处理: 分析这个项目...

  🔧 [主 Agent ]: 调用 agent_spawn(探索项目结构)
  🔄 [🔍 蓝鲸 (代码探索)]: 开始执行: 探索项目结构
  🔧 [🔍 蓝鲸 (代码探索)]: 调用 list_files(目录结构)
  📋 [🔍 蓝鲸 (代码探索)]: 返回目录列表...
  🔧 [🔍 蓝鲸 (代码探索)]: 调用 read_file(读取配置文件)
  📋 [🔍 蓝鲸 (代码探索)]: 返回文件内容...
  ✅ [🔍 蓝鲸 (代码探索)]: 探索完成
  🔧 [主 Agent ]: 调用 agent_collect
...
```

## 修改意义

- **子 Agent 不再黑盒**：用户可以看到每个子 Agent 的实时进展，了解当前在做什么
- **事件驱动架构**：EventBus 为后续 TUI 界面提供了基础事件通道
- **Skills 不再是摆设**：LLM 现在能在对话上下文中看到可用技能列表，可以智能推荐给用户
- **透传设计**：`event_bus` 从主 Agent 透传到所有子 Agent（无论多少层嵌套），事件链完整
