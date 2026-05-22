# DeepSeek-TUI 子 Agent 并行调度原理

基于对 [subagent/mod.rs](https://raw.githubusercontent.com/Hmbown/DeepSeek-TUI/main/crates/tui/src/tools/subagent/mod.rs) (~3500 LOC) 和 [task_manager.rs](https://raw.githubusercontent.com/Hmbown/DeepSeek-TUI/main/crates/tui/src/task_manager.rs) 的源码分析。

---

## 1. 总体架构：三层调度模型

```
┌─────────────────────────────────────────────┐
│           dispatcher CLI (会话入口)            │
│           启动 TUI + 注入消息                   │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│         TUI Runtime (主 Agent Loop)           │
│  ratatui 渲染 + Agent 循环 + 工具调度           │
│                                               │
│  ┌───────────────────────────────────────┐   │
│  │      SubAgentManager (共享状态)        │   │
│  │  Arc<RwLock<HashMap<id, SubAgent>>>   │   │
│  │  + concurrency_cap (默认10, 最大20)    │   │
│  │  + session_boot_id (会话边界隔离)       │   │
│  └───────────────────────────────────────┘   │
│          │              │           │         │
│    ┌─────▼──┐    ┌─────▼──┐   ┌───▼─────┐   │
│    │Agent 1 │    │Agent 2 │   │Agent N  │   │
│    │depth=1 │    │depth=2 │   │depth=3  │   │
│    └────────┘    └────────┘   └─────────┘   │
└──────────────────────────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│       DurableTaskManager (持久化后台)          │
│  SQLite 存储 + bounded worker pool (2~8)      │
│  长时间运行任务独立于 TUI session 生命周期       │
└──────────────────────────────────────────────┘
```

---

## 2. 核心数据结构

### 2.1 `SubAgentManager` — 共享状态容器

```rust
// 全局共享句柄
type SharedSubAgentManager = Arc<RwLock<SubAgentManager>>;

struct SubAgentManager {
    subagents: HashMap<String, SubAgent>,   // 所有子Agent
    workspace_path: PathBuf,                 // 工作区根路径
    state_path: PathBuf,                     // subagents.v1.json 持久化路径
    max_subagents: usize,                    // 并发上限 (默认10, 硬顶20)
    session_boot_id: Uuid,                   // 会话启动ID，隔离跨session的Agent
    max_spawn_depth: usize,                  // 最大嵌套深度 (默认3)
}
```

**设计要点**：`Arc<RwLock<T>>` 是 Rust 中最经典的共享可变状态模式：
- **读操作**（如 `list`、`running_count`）获取共享锁（`read()`），允许多个读并发
- **写操作**（如 `spawn`、`update_status`）获取独占锁（`write()`），保证原子性

### 2.2 `SubAgentRuntime` — 子Agent 运行时上下文

```rust
struct SubAgentRuntime {
    client: ModelClient,              // 与 LLM 通信的客户端
    model: SubagentModelConfig,       // 可独立配置的子Agent模型
    spawn_depth: usize,               // 当前嵌套深度 (0 = 根, 1/2/3 = 子)
    cancel_token: CancellationToken,  // 级联取消令牌
    message_tx: mpsc::UnboundedSender<SubagentEvent>,  // 事件通道（发往UI）
    parent_completion_tx: Option<oneshot::Sender<()>>,  // 仅 depth==1 通知父级
    tools_to_allow: Option<Vec<String>>,  // 按角色限制可用工具
}
```

---

## 3. 并发控制机制

### 3.1 并发上限检查（Concurrency Cap）

```rust
fn running_count(&self) -> usize {
    self.subagents.values()
        .filter(|a| a.status == SubAgentStatus::Running && !a.task_handle.is_finished())
        .count()
}
```

**spawn 入口的检查逻辑**（伪代码）：

```
spawn():
  1. computing = count(status=Pending|Running|Computing)
  2. if computing >= max_subagents:
       → 返回 "达到并发上限，请等待或用 agent_close 释放"
  3. if spawn_depth >= max_spawn_depth:
       → 返回 "达到最大嵌套深度"
  4. 验证 cwd 存在且在工作区内
  5. 获取 write lock，插入新 Agent (status=Pending)
  6. tokio::spawn(run_subagent_task(runtime))
  7. 立即返回 (非阻塞)
```

**关键设计**：`agent_open` 返回后父 Agent 继续工作，不等待子 Agent 完成。这是并行调度的基础——父 Agent 和所有子 Agent 在 tokio 运行时中并发执行。

### 3.2 级联取消树（Cancellation Tree）

```rust
// 每个子Agent从父级创建子令牌
let child_token = parent_cancel_token.child_token();

// 取消根Agent → 所有子孙级联取消
root_token.cancel();
// → child_token.is_cancelled() == true
// → grandchild_token.is_cancelled() == true
```

这是 tokio 的 `CancellationToken` 原生特性：`child_token()` 创建一个与父令牌链接的新令牌，父令牌被取消时，所有子令牌自动标记为已取消。子 Agent 在自己的 loop 中周期检查 `cancel_token.is_cancelled()`，一旦为 true 则优雅退出。

---

## 4. 并行执行模型

### 4.1 主 Agent Loop 中的调度

```
主 Agent Loop (每轮):
  1. 接收 LLM 响应 (streaming)
  2. 解析 tool_call:
     - agent_open  → spawn 子Agent (tokio::spawn) → 立即返回 → 继续工作
     - agent_eval  → 向指定子Agent发送消息 → 继续工作
     - agent_close → 取消子Agent → 标记 Completed/Cancelled
     - 其他工具     → 正常执行
  3. 检查子Agent消息通道 (message_rx):
     - 如果有 Pending 消息 → 注入到主对话
     - 如果有 Completion 信号 → 合并结果
  4. 下一轮 LLM 调用（包含子Agent产生的新消息）
```

### 4.2 子 Agent 独立执行循环

```rust
async fn run_subagent_task(runtime: SubAgentRuntime) {
    // 1. 状态 -> Running
    manager.update_status(id, Running);

    // 2. 注入系统消息 (role-specific prompt)
    conversation.push(system_message);

    loop {
        // 3. 检查取消令牌
        if runtime.cancel_token.is_cancelled() {
            manager.update_status(id, Cancelled);
            return;
        }

        // 4. 调用 LLM
        let response = runtime.client.chat(&conversation).await;

        // 5. 如果有 tool_call → 执行工具
        //    (工具白名单限制：如 reviewer 不能 spawn 新 Agent)
        for tool_call in response.tool_calls {
            if is_allowed(&tool_call.name, &runtime.tools_to_allow) {
                execute_tool(tool_call).await;
            }
            // agent_open 在这里递归 spawn 更深层的子Agent
        }

        // 6. 如果 LLM 说 done → 完成
        if response.finish_reason == Stop {
            break;
        }
    }

    // 7. 完成通知 (仅 depth==1)
    if let Some(tx) = runtime.parent_completion_tx {
        let _ = tx.send(());
    }

    manager.update_status(id, Completed);
}
```

---

## 5. 嵌套深度控制

```
depth=0 (主Agent)
  ├── depth=1 (子Agent) → 可以 spawn depth=2
  │   ├── depth=2 (孙Agent) → 可以 spawn depth=3
  │   │   └── depth=3 → 不能 spawn (达到 max_spawn_depth=3)
  │   └── depth=2 → ...
  └── depth=1 → ...
```

**为什么限制深度？**
1. **防止无限递归**：如果没有深度限制，Agent 可以无限 spawn，迅速耗尽 token 和系统资源
2. **控制复杂度**：每加深一层，结果合并的复杂度指数增长
3. **prefix-cache 有效性**：每个子 Agent 使用父 Agent 的上下文分叉，但深度过深时代理链的语义会漂移

---

## 6. 父级完成通知机制

```rust
// depth == 1 的 Agent: 持有 parent_completion_tx
// depth >= 2 的 Agent: parent_completion_tx = None
```

**为什么只有 depth==1 才通知？**

```
主Agent
  ├── Agent-A (depth=1) → 完成后通过 oneshot 通知主Agent ✓
  │   ├── Agent-A1 (depth=2) → 不通知主Agent ✗
  │   │   └── Agent-A1a (depth=3) → 不通知主Agent ✗
  │   └── Agent-A2 (depth=2) → 不通知主Agent ✗
  └── Agent-B (depth=1) → 完成后通过 oneshot 通知主Agent ✓
```

如果没有这个限制，10+ 个深层 Agent 同时向主 Agent 发送完成通知会造成消息风暴。主 Agent 只需要知道第一层子任务完成了，子任务内部的协调由该子任务自己管理。

---

## 7. 会话边界隔离（Session Boundary）

```rust
struct SubAgent {
    session_boot_id: Uuid,  // 创建该 Agent 时的 session ID
    // ...
}
```

**规则**：
- TUI 每次启动生成新的 `session_boot_id`
- 加载 `subagents.v1.json` 时，所有 `status == Running` 的旧 session Agent → 标记为 `Interrupted`
- 列出 Agent 时默认只显示当前 session 的 Agent（可通过参数查看历史）
- `agent_open` 只会匹配当前 session 的 Agent

**意义**：当用户关闭 TUI 再重新打开时，之前正在运行的子 Agent 不会丢失状态，而是标记为中断。用户可以重新调度或忽略它们。

---

## 8. 持久化与恢复

```rust
// subagents.v1.json 结构
{
  "version": 1,
  "subagents": [
    {
      "id": "uuid",
      "name": "🐐 Ibex (explore)",
      "agent_type": "explore",
      "status": "Completed",
      "spawn_depth": 1,
      "parent_id": "parent-uuid",
      "session_boot_id": "session-uuid",
      "created_at": "...",
      "output": "..."  // 最终输出（符合 output contract）
    }
  ]
}
```

写入策略：**原子写入** — 先写临时文件，再 `rename` 覆盖，避免文件损坏。

---

## 9. 7 种角色类型与工具白名单

| 角色 | 枚举值 | 系统提示词 | 允许的工具 |
|------|--------|-----------|-----------|
| General | `General` | 通用助手 | 全部工具 |
| Explore | `Explore` | 代码库探索专家 | glob, grep, read, ls |
| Plan | `Plan` | 任务分解与规划 | 只读工具, write |
| Review | `Review` | 代码审查 | 只读工具（**不能** spawn） |
| Implementer | `Implementer` | 代码实现 | write, edit, bash（**不能** spawn） |
| Verifier | `Verifier` | 测试与验证 | bash, read, test 工具（**不能** spawn） |
| Custom | `Custom(prompt)` | 用户自定义 | 用户指定 |

---

## 10. DurableTaskManager（持久化任务管理器）

与 `SubAgentManager` 不同，`TaskManager` 管理的是**跨 session 的长时间运行任务**：

```
┌────────────────────────────────────┐
│        DurableTaskManager          │
│  bounded_worker_pool (2~8 workers) │
│  SQLite 持久化                      │
│  TaskRecord (schema versioning)    │
│                                    │
│  任务类型：                          │
│  - background task (脱离TUI运行)     │
│  - scheduled task                  │
│  - batch processing                │
└────────────────────────────────────┘
```

与 `SubAgentManager` 的对比：

| 维度 | SubAgentManager | DurableTaskManager |
|------|----------------|-------------------|
| 生命周期 | TUI session 内 | 跨 session |
| 存储 | JSON 文件 | SQLite |
| 并发模型 | tokio::spawn | bounded worker pool |
| 取消 | CancellationToken 树 | 手动 cancel |
| 状态恢复 | Interrupted 标记 | SQL 查询恢复 |

---

## 11. 总结：并行调度的核心设计原则

1. **共享状态 + 读写锁**：`Arc<RwLock<SubAgentManager>>` 允许多读单写，读操作（list/count）不阻塞，写操作（spawn/update）互斥
2. **非阻塞 spawn**：`agent_open` 只做 `tokio::spawn` + 立即返回，父 Agent 继续工作
3. **级联取消**：tokio `CancellationToken::child_token()` 实现一键取消整棵子任务树
4. **深度限制**：`max_spawn_depth=3` 防止无限递归和 token 爆炸
5. **会话隔离**：`session_boot_id` 区分不同 TUI session 的 Agent，避免误操作
6. **通知分层**：只有 `depth==1` 通知父级，避免消息风暴
7. **原子持久化**：临时文件 + rename，保证状态文件不会损坏