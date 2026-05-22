# 对话与上下文管理

## 会话模型

- `ConversationManager` 管理多会话生命周期
- SQLite 持久化，支持创建/切换/分叉/删除/导出
- `MessageRecord` 含 role / content / token_count / session_id
- 支持断点保存与恢复 (`save_checkpoint` / `get_last_checkpoint`)

## 上下文压缩

- `CompactionConfig` 控制压缩策略参数
- `compaction_threshold_ratio=0.7` 触发压缩
- 三层压缩: micro_compact → collapse → summary_chain
- `PrefixCacheManager` 管理前缀缓存

## 提示词引擎

- `PromptEngine` 基于 `prompt_templates.py` 的模板字典渲染
- `render_system(role, **kwargs)` → 角色系统提示词
- `render_main_system(role, skills, cwd, model, provider)` → 主 Agent 系统提示词
- 模板变量用 `{variable}` 语法，引擎负责替换

## 数据流

```
用户输入 → HumanMessage → ConversationManager.add_message
  → compress_context → LLM.astream(messages)
  → tool_calls → 审批 → 执行 → ToolMessage → 循环
```

- 消息顺序: `SystemMessage → 历史消息 → 最新 HumanMessage`
- 工具调用结果立即转为 `ToolMessage` 追加到上下文