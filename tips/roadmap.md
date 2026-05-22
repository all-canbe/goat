# DeepSeek-TUI 个人版 — 实现评估与开发路线图

基于对 [subagent.md](./subagent.md) 参考文档和当前 Python 代码实现的全面对比分析。

---

## 一、SubAgent 系统实现评估

### 1.1 已完整实现 ✅

| 模块 | 参考文档章节 | 当前实现 |
|------|-------------|---------|
| SubAgentManager 共享状态容器 | §2.1 | `SubAgentManager` 类，asyncio.Lock 保护，支持并发上限/深度上限/session 隔离 |
| AgentContext 运行时上下文 | §2.2 | `AgentContext` dataclass，包含 llm/depth/cancel_token/completion_event |
| 并发上限检查 | §3.1 | `computing_count()` + `max_concurrent` 检查，硬顶 20 |
| 级联取消树 | §3.2 | `CancellationToken` 完整实现 `child_token()` / `cancel()` / `is_cancelled()` |
| 非阻塞 spawn | §4 | `asyncio.create_task()` 立即返回 |
| 子 Agent 独立执行循环 | §4.2 | `_run_agent_loop()` 含取消检查/工具执行/完成通知 |
| 嵌套深度控制 | §5 | `max_spawn_depth` 检查（默认 3） |
| 父级完成通知 | §6 | `asyncio.Event` 仅 depth==1 通知主 Agent |
| 会话边界隔离 | §7 | `session_boot_id` + `_load_state` 跨 session 标记 INTERRUPTED |
| JSON 持久化 | §8 | 临时文件 + rename 原子写入 |
| 7 种角色类型 | §9 | General/Explore/Plan/Implementer/Review/Verifier/Custom |
| 工具白名单 | §9 | 每角色独立 `allowed_tools` + `can_spawn` 控制 |

### 1.2 部分实现 ⚠️

| 模块 | 问题 |
|------|------|
| Skills 系统集成 | `SkillRegistry` 已注册技能但在 Agent 循环中**从未使用**，`_run_agent_loop` 未调用 `skill_registry` |

### 1.3 未实现 ❌

| 模块 | 参考文档章节 | 缺失程度 |
|------|-------------|---------|
| TUI 界面系统 | §1 总体架构 | **完全缺失** — 当前只有 `input()`/`print()` CLI |
| DurableTaskManager | §10 | **完全缺失** — 无 SQLite 持久化后台任务管理 |
| 子 Agent 实时流式输出 | §4 | 子 Agent 输出是最终一次性收集，无流式推送 |
| 跨 session 任务恢复 | §7 | 旧 session 仅标记 INTERRUPTED，无法恢复执行 |
| Prompt 模板管理系统 | §9 | 角色 prompt 硬编码在 `subagent_roles.py`，无模板变量/版本管理 |
| 事件总线 (Event Bus) | §2.2 | 无 `SubagentEvent` 事件通道用于 UI 更新 |
| 子 Agent 调试/暂停/恢复 | §4 | 不支持 pause/resume，仅支持 cancel |
| 结构化输出契约 | §9 | 角色 prompt 中有格式示例，但无程序化验证 |

---

## 二、架构现状总览

```
┌──────────────────────────────────────────────┐
│            CLI (main.py，入口层)                │
│  命令行交互 /help /spawn /list /collect        │
│  input() + print() 纯文本交互                   │
├──────────────────────────────────────────────┤
│          SubAgentManager (核心调度)             │
│  asyncio.Lock + HashMap<id, SubAgent>         │
│  并发上限(默认10) + 深度上限(默认3)               │
│  session_boot_id 会话隔离                       │
│  JSON 原子持久化                                │
├──────────────────────────────────────────────┤
│     Agent Loop (执行层)                        │
│  主 Agent / 子 Agent 共享同一套 _run_agent_loop │
│  LLM 调用 → 工具执行 → 循环直到完成               │
│  级联取消 CancellationToken 树                  │
├──────────────────────────────────────────────┤
│        角色系统 + 工具系统 (支撑层)               │
│  7 种角色 + 5 种内置工具 + 工具白名单              │
│  SkillRegistry (已注册但未在 Agent 循环中使用)    │
└──────────────────────────────────────────────┘
```

### 关键短板

1. **交互层是 CLI 而非 TUI** — 无法展示子 Agent 并行执行的实时状态树
2. **无对话管理** — 每次 `/chat` 是独立的，无对话历史和上下文管理
3. **无流式输出** — 子 Agent 执行结果只能通过 `/collect` 事后拉取
4. **无后台任务** — 所有 Agent 随 session 销毁，不支持长期运行任务

---

## 三、下一步建议实现模块

按照**优先级从高到低**排序，每个模块独立可交付，可根据个人需求调整顺序。

---

### 优先级 1 ▸ 对话管理系统 (Conversation Manager)

**为什么优先：** 当前每次对话是 stateless 的，这是所有高级功能的基础。

**参考：** DeepSeek-TUI 的 `conversation_manager.rs`（对话历史管理 + 上下文窗口）

**核心职责：**
```
┌─────────────────────────────────────────┐
│           ConversationManager            │
│  ┌─────────────────────────────────┐   │
│  │  MessageStore (消息存储)         │   │
│  │  - messages: list[Message]      │   │
│  │  - session_id: str              │   │
│  │  - 自动 Token 计数 & 窗口裁剪    │   │
│  └─────────────────────────────────┘   │
│  ┌─────────────────────────────────┐   │
│  │  ContextBuilder (上下文构建)     │   │
│  │  - 主对话 + 子 Agent 结果注入    │   │
│  │  - 摘要合并 (超过窗口时自动摘要)  │   │
│  └─────────────────────────────────┘   │
│  ┌─────────────────────────────────┐   │
│  │  HistoryManager (历史管理)       │   │
│  │  - SQLite 持久化所有对话         │   │
│  │  - 按日期/关键词搜索历史会话     │   │
│  │  - 导出对话记录                  │   │
│  └─────────────────────────────────┘   │
└─────────────────────────────────────────┘
```

**核心依赖：** `aiosqlite` 或 `sqlite3`

**核心接口：**
```python
class ConversationManager:
    async def create_session(self) -> str        # 创建新会话
    async def add_message(self, msg: Message)    # 添加消息（自动裁剪上下文窗口）
    async def get_context(self) -> list[Message]  # 获取当前上下文（已裁剪）
    async def inject_subagent_result(self, agent_id: str, output: str)  # 注入子Agent结果
    async def list_sessions(self) -> list[SessionInfo]  # 列出历史会话
    async def load_session(self, session_id: str)       # 加载历史会话
```

**设计要点：**
- 上下文窗口策略：FIFO + Token 计数裁剪，超出上限保留 SystemMessage + 最新 N 轮
- 子 Agent 结果注入：完成时自动插入到对话上下文中
- 会话元数据：保存模型、token 用量、子 Agent 数量等

**参考文件：** 查看 DeepSeek-TUI 的 `crates/tui/src/conversation_manager.rs`

---

### 优先级 2 ▸ 流式输出系统 (Streaming System)

**为什么优先：** 当前子 Agent 执行过程是黑盒，用户看不到中间进展。

**核心架构：**
```
┌──────────┐     Event Channel      ┌──────────┐
│ Agent A  │─── SubagentEvent ─────▶│   UI     │
│          │   (流式消息/状态变更)    │  (展示)  │
├──────────┤                        ├──────────┤
│ Agent B  │─── SubagentEvent ─────▶│ 实时显示  │
│          │   (工具调用/结果/进度)   │  执行过程 │
└──────────┘                        └──────────┘
```

**事件类型：**
```python
@dataclass
class SubagentEvent:
    agent_id: str
    type: EventType  # STREAM_CHUNK | STATUS_CHANGE | TOOL_CALL | TOOL_RESULT | ERROR
    payload: str
    timestamp: float
```

**核心接口：**
```python
class EventBus:
    def publish(self, event: SubagentEvent)
    def subscribe(self, agent_id: str | None) -> AsyncIterator[SubagentEvent]
    def subscribe_all(self) -> AsyncIterator[SubagentEvent]
```

---

### 优先级 3 ▸ DurableTaskManager（持久化后台任务）

**参考：** DeepSeek-TUI 的 `task_manager.rs` + subagent.md §10

**核心职责：** 管理跨 session 的长时间运行任务，与 SubAgentManager 生命周期解耦。

```
┌──────────────────────────────────────────────┐
│           DurableTaskManager                  │
│  bounded_worker_pool (asyncio.Semaphore +     │
│  asyncio.Queue, 默认 4 workers)                │
│  SQLite 持久化 TaskRecord                      │
│                                               │
│  任务类型：                                     │
│  - batch: 批量处理（如批量重构所有文件）            │
│  - scheduled: 定时任务                          │
│  - background: 后台监控（如文件变更监听）          │
└──────────────────────────────────────────────┘
```

**核心依赖：** `aiosqlite`

**核心接口：**
```python
class DurableTaskManager:
    async def submit(self, task: Task) -> str           # 提交任务
    async def cancel(self, task_id: str)                 # 取消任务
    async def get_status(self, task_id: str) -> TaskStatus # 查询状态
    async def list_tasks(self) -> list[TaskRecord]       # 列出所有任务
    async def recover(self) -> list[TaskRecord]          # 恢复中断的任务
```

**与 SubAgentManager 的对比：**
| 维度 | SubAgentManager | DurableTaskManager |
|------|----------------|-------------------|
| 生命周期 | TUI session 内 | 跨 session |
| 存储 | JSON 文件 | SQLite |
| 并发模型 | asyncio.create_task | bounded worker pool |
| 取消 | CancellationToken 树 | 手动 cancel |
| 状态恢复 | INTERRUPTED 标记 | SQL 查询后恢复执行 |

---

### 可选延伸模块（上述完成后考虑）

| 模块 | 价值 | 工作量 |
|------|------|--------|
| Prompt 模板引擎 | 基于 Jinja2 的角色 prompt 管理 + 版本控制 | 小 |
| 结构化输出验证 | 子 Agent 输出按 schema 校验（Pydantic） | 中 |
| Token 用量监控 | LLM 调用的 Token 统计 + 成本估算 | 小 |
| 插件系统 | 动态加载外部工具/角色包 | 大 |
| 多模型路由 | 主 Agent 用强模型、子 Agent 用轻量模型 | 中 |
| 测试框架 | 子 Agent 场景的集成测试 | 中 |

---

## 四、推荐执行路径（已排除 TUI 界面，保持 CLI）

```
现在 (subagent 核心完成)
  │
  ├── ✅ 第 1 步: 对话管理系统 (Conversation Manager)
  │    ├── SQLite 持久化
  │    ├── 上下文窗口管理（Token 计数 + 自动裁剪）
  │    ├── 子 Agent 结果注入
  │    └── 历史会话管理（列表/加载/搜索/导出）
  │
  ├── ✅ 第 2 步: 流式输出系统 (Streaming + EventBus)
  │    ├── EventBus 事件总线
  │    ├── 子 Agent 实时输出 → CLI 展示
  │    └── Skills 集成到 Agent 循环
  │
  ├── ✅ 第 3 步: DurableTaskManager
  │    ├── SQLite 持久化
  │    ├── Bounded worker pool
  │    └── 跨 session 恢复
  │
  └── ▸ 可选延伸模块
```

> TUI 界面系统（Textual）已搁置，保持当前 CLI 交互方式。

---

## 五、总结

| 维度 | 状态 |
|------|------|
| SubAgent 核心调度 | ✅ 完整实现（并发控制/级联取消/深度限制/会话隔离） |
| 角色与工具系统 | ✅ 完整实现（7 种角色/工具白名单/角色特定 prompt） |
| 持久化 | ✅ 已完成（SQLite 对话 + JSON Agent 状态） |
| 流式输出与事件系统 | ✅ 已完成（EventBus 事件总线 + CLI 实时展示） |
| Skills 深度集成 | ✅ 已注入 Agent 提示词 |
| Prompt 模板引擎 | ✅ 已完成（模板分离 + 变量注入 + 版本管理 + Claude Code 精简风格） |
| 交互体验 | ⚠️ CLI 模式，有对话历史管理但无 TUI |
| 后台任务 | ✅ 已完成（DurableTaskManager: SQLite + Worker Pool + 跨 session 恢复） |

| 多模型 Provider | ✅ 已完成（Provider 抽象层：OpenAI 兼容 + Anthropic / /provider 和 /model 命令） |
| Git 集成工具 | ✅ 已完成（git_status / git_diff / git_log / git_commit，已注册到 General + Implementer 角色） |
| Token 用量监控与成本估算 | ✅ 已完成（TokenTracker: 模型定价表、按轮次跟踪、/cost 命令、/status 显示） |
| 持久化会话保存/恢复 | ✅ 已完成（checkpoint save/resume、/resume、/fork 命令、自动断点提示） |

**当前项目状态：** 三个核心模块 + Prompt 模板引擎 + 多模型 Provider + Git 工具 + Token 用量监控 + 会话持久化全部完成。**下一步可从可选延伸模块中选择：结构化输出契约、Web 搜索、插件系统等。**
