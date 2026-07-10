# RGoat P0 问题彻查与修复计划

> 调查时间：2026-07-07  
> 调查范围：rgoat-core / rgoat-tui / rgoat-desktop 全工作空间  
> 参考对标：[CodeWhale](https://github.com/Hmbown/CodeWhale)（保持当前黑红风格）

---

## 目录

- [第一部分：P0 级 Panic 崩溃](#第一部分p0-级-panic-崩溃)
- [第二部分：TUI / 桌面端启动隔离](#第二部分tui--桌面端启动隔离)
- [第三部分：对接问题彻查](#第三部分对接问题彻查)
- [第四部分：UI/UX 对齐 CodeWhale](#第四部分uiux-对齐-codewhale)
- [修复优先级总表](#修复优先级总表)

---

## 第一部分：P0 级 Panic 崩溃

### P0-1  Backspace 数组越界（已知崩溃）

**现象**：`rgoat tui` 运行时 panic：
```
thread 'main' panicked at rgoat-tui\src\app.rs:975:61:
index out of bounds: the len is 1 but the index is 1
```

**根因**：`submit()` 函数（`app.rs:337-380`）有**两条提前返回路径**不重置 `input_cursor`：

```rust
async fn submit(&mut self) {
    let prompt = std::mem::take(&mut self.input);  // ← input 被清空
    if prompt.trim().is_empty() {
        return;                                     // ← 路径A：cursor 未重置！
    }
    if prompt.starts_with('/') {
        self.handle_command(&prompt).await;
        return;                                     // ← 路径B：cursor 未重置！
    }
    // ... 仅正常路径到达 ↓
    self.input_cursor = 0;                          // ← 只有这里重置
}
```

**触发链路**：
1. 输入 `"/help"` → Enter → `input` 被清空，`input_cursor = 5`（陈旧）
2. 输入字符 `'a'` → `input = "a"`（1 个字符），`input_cursor = 6`
3. 按 Backspace → 守卫 `!input.is_empty() && input_cursor > 0` 通过 → `char_indices[5]` → **越界 panic**（`char_indices` 长度只有 1）

**修复方案**：

在 `submit()` 的两条提前返回路径前补齐状态重置：

```rust
async fn submit(&mut self) {
    let prompt = std::mem::take(&mut self.input);
    // ★ 无论走哪条路径，都先重置输入状态
    self.input_cursor = 0;
    self.show_command_menu = false;
    self.command_menu_index = 0;

    if prompt.trim().is_empty() { return; }
    if prompt.starts_with('/') {
        self.handle_command(&prompt).await;
        return;
    }
    // ... 正常路径移除重复重置（已在顶部完成）
}
```

同时给 Backspace 处理（`app.rs:975`）加防御性上界检查：

```rust
(KeyCode::Backspace, _) => {
    let char_count = app.input.chars().count();
    if !app.input.is_empty() && app.input_cursor > 0 && app.input_cursor <= char_count {
        let char_indices: Vec<(usize, char)> = app.input.char_indices().collect();
        let (byte_pos, _) = char_indices[app.input_cursor - 1];
        app.input.remove(byte_pos);
        app.input_cursor -= 1;
    }
    app.show_command_menu = app.input.starts_with('/');
}
```

**影响文件**：`rgoat-tui/src/app.rs`  
**修复量**：~15 行

---

### P0-2  /clear 后流式事件越界 Panic

**现象**：执行 `/clear` 后，如果后台 agent 仍在流式输出，下一个 `Message`/`Finished` 事件到达时 panic。

**根因**：`/clear` 命令（`app.rs:418-422`）清空 `self.lines` 但**不重置 `streaming_idx`**：

```rust
"/clear" => {
    self.lines.clear();       // ← lines 变空
    self.scroll_offset = 0;
    // ★ 缺少 self.streaming_idx = None;
    self.add_line(UiElement::System { text: "Cleared.".into() });
}
```

后续 `AgentEvent::Message`（`app.rs:604-607`）使用 `self.streaming_idx` 的旧值直接索引已清空的 `self.lines`：

```rust
if let Some(idx) = self.streaming_idx {
    self.lines[idx] = UiElement::Assistant { text: content.clone() };  // ← 越界！
}
```

同理 `AgentEvent::Finished`（`app.rs:618-624`）的 `self.lines[idx]` 和 `self.lines.remove(idx)` 也会越界。

**修复方案**：

1. `/clear` 补齐重置：
```rust
"/clear" => {
    self.lines.clear();
    self.scroll_offset = 0;
    self.streaming_idx = None;   // ★ 新增
    self.add_line(UiElement::System { text: "Cleared.".into() });
}
```

2. `Message` 和 `Finished` 分支改用安全访问：
```rust
// Message (line 604-607)
if let Some(idx) = self.streaming_idx {
    if let Some(elem) = self.lines.get_mut(idx) {
        *elem = UiElement::Assistant { text: content.clone() };
    } else {
        // idx 失效，重新追加
        self.streaming_idx = None;
        self.add_line(UiElement::Assistant { text: content.clone() });
        self.streaming_idx = Some(self.lines.len() - 1);
    }
}
```

**影响文件**：`rgoat-tui/src/app.rs`  
**修复量**：~30 行

---

### P0-3  Setup Wizard 非 TTY 环境 Panic

**现象**：在管道/非交互环境（如 `echo "" | rgoat tui`）首次运行时 panic。

**根因**：`main.rs` 的 `run_setup_wizard()` 函数有 8 处 `.unwrap()`，集中在 `io::stdout().flush()` 和 `stdin.read_line()` 调用上（`main.rs:294-342`）。非 TTY 环境下这些操作返回 `Err`。

**修复方案**：将 8 处 `.unwrap()` 改为 `?` 传播错误，或检测 TTY 后跳过 wizard：

```rust
use std::io::IsTerminal;

fn run_setup_wizard() -> Result<Settings, SetupError> {
    if !std::io::stdin().is_terminal() {
        return Err(SetupError::NotInteractive);
    }
    // ... 原逻辑，unwrap 改为 ?
}
```

**影响文件**：`rgoat-tui/src/main.rs`  
**修复量**：~20 行

---

## 第二部分：TUI / 桌面端启动隔离

### 隔离现状结论

**调查结果：TUI 和桌面端之间没有任何单实例锁、端口绑定冲突或文件锁。** 两者技术上可以同时启动。

那为什么用户感觉"没有启动隔离"？**根因是共享 SQLite 数据库的并发写入冲突**：

- 两者都打开同一个 `~/.goat/conversations.db`
- 启用了 WAL 模式（允许多读 + 单写）
- **但未设置 `PRAGMA busy_timeout`** — 当两个进程同时写入时，后到的写操作**立即**收到 `SQLITE_BUSY` 错误，不等待重试
- 错误向上传播为 `ConversationError::Database(sqlx::Error)`，导致 agent 运行失败或 TUI 崩溃

### P0-4  SQLite 并发写入冲突（启动隔离根因）

**位置**：`rgoat-core/src/conversation/manager.rs:72-117`

**现状**：
```rust
let pool = SqlitePoolOptions::new()
    .max_connections(5)
    .connect(&format!("sqlite:{}?mode=rwc", db_path.display()))
    .await?;

sqlx::query("PRAGMA journal_mode=WAL").execute(&pool).await?;
sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;
// ★ 缺少 busy_timeout
```

**修复方案**（三管齐下）：

1. **连接字符串加 busy_timeout**：
```rust
.connect(&format!("sqlite:{}?mode=rwc&busy_timeout=5000", db_path.display()))
```

2. **PRAGMA 设置 busy_timeout**：
```rust
sqlx::query("PRAGMA busy_timeout=5000").execute(&pool).await?;
```

3. **写入操作加重试包装**：在 `add_message` / `create_session` 等写方法中加入 `SQLITE_BUSY` 重试逻辑（3 次重试，每次 100ms 退避）。

**影响文件**：`rgoat-core/src/conversation/manager.rs`  
**修复量**：~15 行

---

### P0-5  setting.json 非原子写入竞争

**位置**：`rgoat-core/src/core/config.rs:329-337`

**现状**：`Settings::save()` 用 `std::fs::write` 直接覆盖写入，非原子操作。TUI setup wizard 和 Desktop `configure_provider` 同时写入时可能互相覆盖。

**修复方案**：改为临时文件 + rename 原子写入：
```rust
pub fn save(&self) -> Result<(), ConfigError> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(self)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &content)?;
    std::fs::rename(&tmp, &path)?;  // 原子操作
    Ok(())
}
```

**影响文件**：`rgoat-core/src/core/config.rs`  
**修复量**：~5 行

---

## 第三部分：对接问题彻查

### P1-1  /resume 不恢复消息到 UI

**位置**：`rgoat-tui/src/app.rs:454-481`

**现状**：`/resume <id>` 只调用 `get_session()` 获取元数据（title, message_count），打印一条系统消息。**不调用** `get_messages()` 把历史消息加载回 `self.lines`。UI 仍是空的。

**修复方案**：调用 `get_messages()` 后将消息转换为 `UiElement` 追加到 `self.lines`：
```rust
"/resume" => {
    let session_id = parts.get(1).copied().unwrap_or("");
    if let Ok(messages) = self.conversation.get_messages(session_id).await {
        self.lines.clear();
        self.streaming_idx = None;
        self.session_id = session_id.to_string();
        for msg in messages {
            match msg.role.as_str() {
                "user" => self.add_line(UiElement::User { text: msg.content }),
                "assistant" => self.add_line(UiElement::Assistant { text: msg.content }),
                "tool" => self.add_line(UiElement::System { text: msg.content }),
                _ => {}
            }
        }
    }
}
```

**影响文件**：`rgoat-tui/src/app.rs`  
**修复量**：~30 行

---

### P1-2  TUI 启动不自动恢复上次会话

**位置**：`rgoat-tui/src/app.rs:275-290`（`init()` 方法）

**现状**：每次启动 TUI 都调用 `create_session()` 创建全新会话，不复用上次的会话。

**修复方案**：启动时检查是否有最近的未完成会话（通过 `list_sessions()` 取最近一条），如有则加载其消息；否则创建新会话。可加 `--resume` / `--new` CLI flag 控制行为。

**影响文件**：`rgoat-tui/src/app.rs`, `rgoat-tui/src/main.rs`  
**修复量**：~40 行

---

### P1-3  /compact /list /new 为空壳命令

**位置**：`rgoat-tui/src/app.rs`

| 命令 | 行号 | 现状 |
|------|------|------|
| `/compact` | 448-453 | 只打印提示，不调用 compressor |
| `/list` | 513-515 | 只打印提示，指向 CLI |
| `/new` | 444-447 | 只打印提示，不创建新会话 |

**修复方案**：
- `/compact`：调用 `rgoat-core/src/conversation/compressor.rs` 的压缩逻辑
- `/list`：调用 `conversation.list_sessions()` 渲染到 UI
- `/new`：调用 `conversation.create_session()` 创建新会话并清空 UI

**影响文件**：`rgoat-tui/src/app.rs`  
**修复量**：~60 行

---

### P1-4  rgoat-core 定义但未接线的命令

**位置**：`rgoat-core/src/cli/mod.rs` 的 `InteractiveCommand` 枚举

定义了 `/mode`、`/save`、`/index` 等命令解析，但 TUI 的 `handle_command()` 完全不处理。属于死代码。

**修复方案**：
- `/mode`：接入 TUI（当前用 `/agent` `/plan` 等替代，但应支持 `/mode plan` 格式）
- `/save`：实现导出对话到文件
- `/index`：接入代码索引功能（或暂标记为 TODO）

**影响文件**：`rgoat-tui/src/app.rs`  
**修复量**：~30 行

---

### P1-5  桌面端 AcceptEdits 模式遗漏

**位置**：`rgoat-desktop/frontend/src/components/input/InputPanel.tsx`

**现状**：桌面端 `MODES` 数组只列了 4 种模式（Agent / Plan / Flow / YOLO），遗漏了 `AcceptEdits`。TUI 端 5 种齐全。

**修复方案**：在 `MODES` 数组中补充 `AcceptEdits`。

**影响文件**：`rgoat-desktop/frontend/src/components/input/InputPanel.tsx`  
**修复量**：~1 行

---

### P1-6  tool_card.rs 硬编码主题键索引

**位置**：`rgoat-tui/src/components/tool_card.rs:144`

```rust
let theme = &ts.themes["base16-ocean.dark"];  // ← 裸索引
```

**修复方案**：改用 `get()` + fallback：
```rust
let theme = ts.themes.get("base16-ocean.dark")
    .or_else(|| ts.themes.values().next())
    .unwrap_or_else(|| syntect::highlighting::ThemeSet::load_defaults().themes.values().next().unwrap());
```

**影响文件**：`rgoat-tui/src/components/tool_card.rs`  
**修复量**：~5 行

---

## 第四部分：UI/UX 对齐 CodeWhale

> 原则：保持当前黑红风格，对齐 CodeWhale 的交互模式和功能体验

### 当前已对齐的部分

| 功能 | 状态 |
|------|------|
| 黑红配色主题 | ✅ TUI `theme.rs` + 桌面端 `index.css` |
| 多模式切换（Agent/Plan/Flow/YOLO） | ✅ 5 种模式（桌面端缺 1 种） |
| 斜杠命令系统 | ✅ 15 个命令（部分为空壳） |
| 工具调用卡片 | ✅ 可折叠 ToolCard |
| Diff 视图 | ✅ 带行号+滚动 |
| 审批对话框 | ✅ 模态审批 |
| Provider 切换 | ✅ `/provider` `/model` |
| 流式文本输出 | ✅ Streaming |
| 桌面端 Ctrl+K 命令面板 | ✅ CommandPalette |

### 需要对齐的部分

#### UX-1  Tab 键快速切换模式（CodeWhale 核心交互）

**CodeWhale**：`Tab` 键在 Plan → Agent → YOLO 间循环切换  
**RGoat 现状**：无 Tab 绑定，需输入 `/agent` `/plan` 等命令切换

**方案**：绑定 `Tab` 键循环切换模式，在状态栏显示当前模式高亮。

**影响文件**：`rgoat-tui/src/app.rs`（键盘事件处理）  
**修复量**：~15 行

---

#### UX-2  Work Sidebar — 计划/清单面板

**CodeWhale**：Work sidebar 实时显示 plan 和 checklist 状态  
**RGoat 现状**：TUI 无任何侧边栏

**方案**：TUI 左侧增加可折叠侧边栏（`Ctrl+B` 切换），显示当前 plan 模式生成的步骤清单，支持勾选状态。需配合 Plan 模式 agent 输出结构化 plan 数据。

**影响文件**：`rgoat-tui/src/app.rs`（布局）, 新增 `rgoat-tui/src/components/work_sidebar.rs`  
**修复量**：~200 行（新组件 + 布局重构）

---

#### UX-3  诚实成本显示

**CodeWhale**：状态栏显示真实 $ 成本，未匹配模型显示 "unknown" 而非 $0  
**RGoat 现状**：状态栏只有 token 计数（↑输入 ↓输出），无 $ 换算

**方案**：
1. `rgoat-core` 的 `token_tracker` 增加价格表（按 model 维护 input/output 单价）
2. `StatusBarData` 增加 `cost_usd: f64` 字段
3. 状态栏渲染 `cost: $0.042` 样式
4. 未匹配模型显示 `cost: unknown`

**影响文件**：`rgoat-core/src/core/token_tracker.rs`, `rgoat-tui/src/components/status_bar.rs`, `rgoat-desktop/frontend/src/components/status/StatusBar.tsx`  
**修复量**：~80 行

---

#### UX-4  会话持久化与跨重启恢复

**CodeWhale**：会话跨重启和系统休眠持久化  
**RGoat 现状**：后端 SQLite 可持久化，但 TUI 启动不恢复（见 P1-2）

**方案**：见 P1-1 + P1-2 修复。TUI 启动时自动恢复最近会话或提供选择列表。

---

#### UX-5  /goal 持久化目标

**CodeWhale**：`/goal` 设置持久化目标，代理跨轮次持续工作直到完成  
**RGoat 现状**：无目标概念

**方案**：
1. 新增 `~/.goat/goals/` 目录存储目标状态
2. `/goal <description>` 命令设置目标
3. Agent 循环中注入目标上下文，跨轮次维持
4. 状态栏显示当前目标进度

**影响文件**：`rgoat-core/src/agent/react.rs`, `rgoat-tui/src/app.rs`  
**修复量**：~150 行（中长期功能）

---

#### UX-6  /restore 回滚机制

**CodeWhale**：Side-git 快照，`/restore` 撤销某轮操作不触碰真实 Git 历史  
**RGoat 现状**：无回滚机制

**方案**：每次 agent 执行前在 `~/.goat/snapshots/` 创建 side-git 快照，`/restore` 回滚。

**影响文件**：新增 `rgoat-core/src/security/snapshot.rs`, `rgoat-tui/src/app.rs`  
**修复量**：~200 行（中长期功能）

---

#### UX-7  Shell 命令透传

**CodeWhale**：`! <command>` 前缀直接运行 shell 命令  
**RGoat 现状**：无此功能

**方案**：输入以 `!` 开头时，直接 spawn shell 命令，输出显示在 chat area。

**影响文件**：`rgoat-tui/src/app.rs`（submit 逻辑）  
**修复量**：~25 行

---

#### UX-8  两端配色统一

**现状**：
- TUI 主红：`RGB(220, 38, 38)`（纯红 #DC2626）
- 桌面端主红：`#e94560`（偏粉红）
- TUI success：`RGB(34, 197, 94)`（纯绿）
- 桌面端 success：`#4ecca3`（青绿）

**方案**：统一为同一套色值。建议以 TUI 的为准（更标准的红）：
- `--primary`: `#dc2626`
- `--success`: `#22c55e`
- `--error`: `#ef4444`

**影响文件**：`rgoat-desktop/frontend/src/index.css`  
**修复量**：~5 行

---

## 修复优先级总表

### 立即修复（P0 — 阻断使用）

| 编号 | 问题 | 影响文件 | 修复量 | 风险 |
|------|------|----------|--------|------|
| P0-1 | Backspace 越界 panic（已知崩溃） | `app.rs` | ~15 行 | 用户已遇到 |
| P0-2 | /clear 后流式事件越界 panic | `app.rs` | ~30 行 | 高概率触发 |
| P0-3 | Setup wizard 非 TTY panic | `main.rs` | ~20 行 | 中等概率 |
| P0-4 | SQLite 并发写入冲突（隔离根因） | `manager.rs` | ~15 行 | 高概率触发 |
| P0-5 | setting.json 非原子写入 | `config.rs` | ~5 行 | 低概率但数据丢失 |

### 尽快修复（P1 — 功能残缺）

| 编号 | 问题 | 影响文件 | 修复量 |
|------|------|----------|--------|
| P1-1 | /resume 不恢复消息到 UI | `app.rs` | ~30 行 |
| P1-2 | TUI 启动不恢复上次会话 | `app.rs`, `main.rs` | ~40 行 |
| P1-3 | /compact /list /new 空壳命令 | `app.rs` | ~60 行 |
| P1-4 | 核心库定义未接线的命令 | `app.rs` | ~30 行 |
| P1-5 | 桌面端遗漏 AcceptEdits 模式 | `InputPanel.tsx` | ~1 行 |
| P1-6 | tool_card.rs 硬编码主题索引 | `tool_card.rs` | ~5 行 |

### 体验对齐（UX — CodeWhale 对标）

| 编号 | 功能 | 影响文件 | 修复量 | 阶段 |
|------|------|----------|--------|------|
| UX-1 | Tab 键快速切换模式 | `app.rs` | ~15 行 | 短期 |
| UX-2 | Work Sidebar 计划/清单面板 | 新组件 + 布局 | ~200 行 | 中期 |
| UX-3 | 诚实成本显示 | 多文件 | ~80 行 | 中期 |
| UX-4 | 会话跨重启恢复 | `app.rs`, `main.rs` | ~40 行 | 短期（依赖 P1-2） |
| UX-5 | /goal 持久化目标 | `react.rs`, `app.rs` | ~150 行 | 长期 |
| UX-6 | /restore 回滚机制 | 新模块 | ~200 行 | 长期 |
| UX-7 | Shell 命令透传 | `app.rs` | ~25 行 | 短期 |
| UX-8 | 两端配色统一 | `index.css` | ~5 行 | 短期 |

---

## 建议修复顺序

**第一批（让项目能跑起来）**：P0-1 → P0-2 → P0-4 → P0-3 → P0-5  
**第二批（功能补全）**：P1-1 → P1-2 → P1-3 → P1-5 → P1-6 → P1-4  
**第三批（体验对齐-短期）**：UX-1 → UX-7 → UX-8 → UX-4  
**第四批（体验对齐-中长期）**：UX-3 → UX-2 → UX-5 → UX-6
