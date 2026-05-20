# 持久化后台任务管理器 (DurableTaskManager)

## 变更内容

### 1. 新增 DurableTaskManager 模块
`subagent_demo/durable_task_manager.py`

- **SQLite 持久化**：使用 `tasks.db` 存储所有任务记录，跨 session 持久
- **Bounded Worker Pool**：基于 `asyncio.Semaphore` + `asyncio.Queue` 的有界并发池（默认 4 workers）
- **任务类型**：
  - `BATCH` — 批量处理任务（如批量重构、执行命令）
  - `BACKGROUND` — 后台监控任务
  - `SCHEDULED` — 定时任务（预留）
- **任务状态机**：`PENDING → RUNNING → COMPLETED/FAILED/CANCELLED/PAUSED`

### 2. 核心接口

| 方法 | 说明 |
|------|------|
| `submit(name, description, metadata)` | 提交任务，返回 `(task_id, error)` |
| `cancel(task_id)` | 取消任务 |
| `pause(task_id)` | 暂停运行中的任务 |
| `resume(task_id)` | 恢复暂停的任务 |
| `get_status(task_id)` | 查询单个任务状态 |
| `list_tasks(status, limit)` | 按状态筛选列出任务 |
| `recover()` | 恢复中断/运行中的任务到队列 |
| `register_task_type(TaskDef)` | 注册可执行的任务类型 |

### 3. 内置任务类型

| 任务名 | 说明 | 参数 (metadata) |
|--------|------|----------------|
| `explore` | 探索代码库目录结构 + 读取关键文件 | `directory`, `targets[]` |
| `batch_run` | 批量执行 shell 命令 | `commands[]` |

### 4. 跨 Session 恢复

- 启动时自动调用 `recover()`，将上次中断的 Running/Pending 任务重新入队
- Running 任务会被标记为 Pending 并添加"已恢复"备注
- 所有任务状态变化实时写入 SQLite

### 5. CLI 集成

| 命令 | 说明 |
|------|------|
| `/task <name> [描述] [metadata:{...}]` | 提交后台任务 |
| `/task_list [status]` | 列出任务（可选按状态筛选） |
| `/task_cancel <id>` | 取消任务 |
| `/task_pause <id>` | 暂停运行中的任务 |
| `/task_resume <id>` | 恢复暂停的任务 |
| `/task_recover` | 手动恢复中断的任务 |

**使用示例：**
```
/task explore 探索 src 目录  metadata:{"directory":"src","targets":["main.py","setting.json"]}
/task batch_run 安装依赖  metadata:{"commands":["pip install -r requirements.txt","python -m test"]}
/task_list running
/task_cancel a1b2c3d4
```

## 修改的文件

| 文件 | 变更 |
|------|------|
| `subagent_demo/durable_task_manager.py` | **新增** — 持久化后台任务管理器 |
| `subagent_demo/__init__.py` | 导出 DurableTaskManager 及相关类型 |
| `main.py` | `__init__` 增加 task_manager；`initialize()` 初始化/注册任务/启动/恢复；添加 6 个 `/task*` 命令处理；`_show_status()` 显示任务池状态；`_cleanup()` 停止任务管理器 |
| `tips/roadmap.md` | 标记第 3 步 ✅ 完成 |

## 与 SubAgentManager 的对比

| 维度 | SubAgentManager | DurableTaskManager |
|------|----------------|-------------------|
| 生命周期 | Session 内 | 跨 Session |
| 存储 | JSON 文件 | SQLite |
| 并发模型 | asyncio.create_task 无上限 | Bounded Worker Pool (Semaphore + Queue) |
| 取消机制 | CancellationToken 级联树 | asyncio.Event |
| 状态恢复 | INTERRUPTED 标记 | SQL 查询 + 重新入队 |
| 主要用途 | LLM 驱动的并行子 Agent | 预设的后台/批量处理任务 |

## 修改意义

- **跨 Session 持久化**：任务提交后即使关闭程序也不会丢失，下次启动自动恢复
- **资源可控**：Bounded Worker Pool 限制并发的后台任务数，防止资源耗尽
- **可扩展**：通过 `register_task_type()` 可注册任意自定义任务函数
- **生命周期分离**：后台任务与 Agent Session 解耦，长时间运行任务不再阻塞主流程
