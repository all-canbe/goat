# 对话管理系统 (ConversationManager)

## 变更内容

### 1. 新增 ConversationManager 模块
`subagent_demo/conversation_manager.py`

- **SQLite 持久化**：使用 `conversations.db` 文件存储所有对话历史和元数据
- **WAL 模式**：启用 SQLite WAL（Write-Ahead Logging）提升并发读写性能
- **消息角色映射**：`human → user`, `ai → assistant`, `system → system`, `tool → tool`
- **Token 估算**：基于字符数粗略估算（`len(text) // 4`），无需额外依赖

### 2. 会话管理
- **自动创建会话**：启动时自动创建默认会话，无需手动操作
- **`/sessions`**：列出所有会话（显示 session_id、标题、消息数、创建时间）
- **`/session [id]`**：切换当前会话（不带参数则列出所有会话 ID）
- **`/session_rename <id> <name>`**：重命名会话
- **`/session_delete <id>`**：删除会话及其所有消息

### 3. 上下文窗口管理
- **`get_context()`**：构建 LLM 上下文时自动进行 Token 窗口裁剪
- **策略**：保留所有 SystemMessage，按时间倒序保留最新消息直到达到 `max_tokens`（默认 128K）
- 首次 LLM 调用时注入历史上下文，避免每次对话都是冷启动

### 4. 对话持久化
- **`add_message(BaseMessage)`**：每次 LLM 调用和工具执行结果自动写入 SQLite
- **`inject_subagent_result()`**：子 Agent 完成时可注入结果到对话上下文
- **会话统计**：自动追踪 `message_count`、`token_count`、`updated_at`
- 退出自动关闭数据库连接

### 5. 搜索与导出
- **`/search <关键词>`**：基于 SQL `LIKE` 模糊搜索历史消息内容
- **`/export [session_id] [format]`**：导出会话（`text` 纯文本 / `json` 结构化格式）

### 6. CLI 集成
- `_do_chat()` 改为使用 `self.conversations.get_context()` 获取历史上下文
- `_show_status()` 增加对话系统状态显示（当前会话 ID、消息数、Token 用量）
- `_cleanup()` 中关闭 ConversationManager 的数据库连接

## 修改的文件

| 文件 | 变更 |
|------|------|
| `subagent_demo/conversation_manager.py` | **新增** — 对话管理系统核心模块 |
| `subagent_demo/__init__.py` | 导出 ConversationManager、SessionInfo、MessageRecord |
| `main.py` | 集成对话管理到 CLI |
| `tips/roadmap.md` | 更新优先级路径，移除 TUI |

## 未完成 / 已知待改进

- [ ] **Token 精确计数**：当前使用字符估算，可集成 `tiktoken` 实现精确计数
- [ ] **Skills 集成**：`SkillRegistry` 已注册技能但未在 Agent 循环中使用
- [ ] **子 Agent 自动结果注入**：目前 `inject_subagent_result` 方法已定义但未在 `collect_results` 流程中自动调用
- [ ] **上下文摘要**：超出窗口时仅裁剪，未实现摘要合并策略

## 使用示例

```
# 启动后自动创建会话，对话自动持久化

# 查看所有会话
/sessions

# 切换会话
/session a1b2c3d4

# 搜索历史消息
/search 配置

# 导出当前会话为 JSON
/export json

# 重命名会话
/session_rename a1b2c3d4 我的分析会话
```

## 修改意义

- **不再丢失上下文**：每次对话自动保存到 SQLite，切换会话后完整恢复上下文
- **Token 窗口裁剪**：长对话超出模型 context window 时自动丢弃最早的非系统消息
- **历史可检索**：通过 `/sessions` 和 `/search` 可回溯任何历史对话
- **数据可移植**：`conversations.db` 标准 SQLite 格式，可用任何 SQLite 工具查看
