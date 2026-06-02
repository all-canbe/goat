# 🐐 Goat — 山羊主题 Agent 工具

> 子 Agent 并行调度 · 多模式审批 · 对话管理 · Web/TUI/CLI 三端入口

Goat 是一个面向开发者的 AI Agent 工具集，提供可扩展的子 Agent 编排、Tool 审批系统、MCP 协议、Flow 实现-审查-修复闭环，以及 CLI / TUI / Web 三种交互形态。

---

## ✨ 核心特性

- **多模式交互** — Agent / Plan / YOLO / Flow 四种运行模式，覆盖从只读规划到自动执行的全谱系
- **子 Agent 并行调度** — 基于 LangGraph 的 SubAgent 系统，支持角色权限隔离、嵌套深度控制、并发上限
- **三层模型路由** — 主 Agent / 子 Agent / 审查模型可独立配置，支持不同 Provider 与连通性自动测试
- **工具审批沙箱** — 决策模型 BLOCK / ASK / ALLOW，配合 ML 分类与规则引擎，三平台沙箱（Linux/macOS/Windows）
- **MCP 协议支持** — stdio 与 SSE/HTTP 双向通信，可作为 MCP Server 暴露工具，亦可连接外部 MCP
- **持久化对话** — SQLite 存储会话/消息/Checkpoint，支持断点恢复、Fork、Token 跟踪与上下文压缩
- **持久化后台任务** — 4 Worker 并发，任务可提交/取消/暂停/恢复/恢复中断
- **生命周期钩子** — 6 类事件（session / pre-tool / post-tool / permission / stop），支持命令/HTTP/Prompt/MCP/Agent 五种处理器
- **Web 前端** — React 18 + Zustand 5 + Tailwind，WebSocket 实时消息流，文件树、命令面板、审批对话框
- **Skill 系统** — 内置 5 个 Skill（explore/writer/reviewer/tester/planner），支持外部 Skill 安装（npx skills add）

---

## 🏗️ 架构概览

```
goat/
├── agent/         Agent 循环、SubAgent 管理、角色系统、Skill 注册、Flow 流水线
├── api/           FastAPI 路由、WebSocket 管理、Chat Handler
├── audit/         审计日志
├── browser/       Playwright 浏览器自动化
├── conversation/  对话持久化、上下文压缩、提示词渲染
├── core/          取消令牌、事件总线、Token 跟踪、工作空间
├── hooks/         生命周期钩子
├── mcp/           MCP 客户端/服务器、工具桥接、SSE、速率限制
├── memory/        跨会话持久化记忆
├── provider/      LLM Provider 抽象（OpenAI 兼容 / Anthropic）
├── security/      工具审批、沙箱（Linux/macOS/Windows）
├── tasks/         持久化后台任务
└── tools/         内置工具（文件 / Git / Shell / Web / Browser）
```

模块依赖方向（禁止反向依赖）：

```
main → conversation, agent, tools, security, provider, tasks, core
agent      → conversation, tools, security, core
api        → conversation, provider, mcp, core
mcp        → tools, core
tools      → core
hooks      → 无内部依赖
```

---

## 🚀 快速开始

### 环境要求

- **Python** >= 3.10
- **Node.js** >= 18（仅 Web 模式）
- **操作系统** Windows / macOS / Linux

### 安装

```bash
# 1. 克隆仓库
git clone https://github.com/all-canbe/my-tui.git
cd my-tui

# 2. 创建虚拟环境
python -m venv venv
# Windows
.\venv\Scripts\activate
# macOS / Linux
source venv/bin/activate

# 3. 安装依赖
pip install -r requirements.txt

# 4.（可选）安装 Playwright 浏览器
playwright install chromium
```

### 配置 LLM

首次启动会进入交互式配置，按提示输入：

- **Provider** 类型（OpenAI 兼容 / Anthropic）
- **Base URL**
- **API Key**
- **模型名称**

配置会保存到 `~/.goat/setting.json`，下次启动自动加载。

也可通过环境变量配置（优先级高于 setting.json）：

```bash
export API_KEY=your-api-key
export BASE_URL=https://api.openai.com/v1
export MODEL=gpt-4o
```

### 运行

| 命令 | 模式 | 说明 |
|------|------|------|
| `goat` | CLI | 交互式命令行 |
| `goat_tui` | TUI | 基于 Textual 的终端界面 |
| `goat web` | Web | FastAPI + React 前端（需先构建前端） |
| `goat mcp serve` | MCP | 启动 stdio MCP 服务器 |
| `goat mcp serve-sse` | MCP | 启动 SSE/HTTP MCP 服务器 |
| `goat mcp connect` | MCP | 连接已配置的 MCP 服务器 |
| `goat mcp list` | MCP | 列出 MCP 连接 |
| `goat mcp add '<json>'` | MCP | 添加 MCP 服务器 |

也可直接通过 Python 入口运行：

```bash
python main.py                  # CLI
python tui_main.py              # TUI
python main.py web              # Web
```

### Web 模式前端构建

首次运行 Web 模式前需要构建前端：

```bash
cd frontend
npm install
npm run build
cd ..
python main.py web
```

开发模式（带热更新）：

```bash
# 终端 1：启动后端
python main.py web

# 终端 2：启动前端 dev server
cd frontend
npm run dev
```

---

## 🎮 CLI 命令

### 运行模式

| 模式 | 命令 | 行为 |
|------|------|------|
| Agent（默认） | `/agent` | 每次写/删/执行操作询问确认 |
| Plan | `/plan` | 只读探索 + 制定计划，完成后询问执行 |
| YOLO | `/yolo` | 全部自动批准（安全守卫仍生效） |
| Flow | `/flow` | 复杂任务自动实现→审查→修复（最多 3 轮） |

### 常用命令

| 命令 | 功能 |
|------|------|
| `/spawn <角色> <任务>` | 启动子 Agent |
| `/list` | 列出所有子 Agent |
| `/collect [ids]` | 收集 Agent 结果 |
| `/cancel <id>` | 取消 Agent |
| `/sessions` | 列出对话历史 |
| `/session [id]` | 切换会话 |
| `/new [title]` | 新建会话 |
| `/resume` | 恢复上次断点 |
| `/fork <id> [turn]` | 分叉会话 |
| `/search <关键词>` | 搜索历史消息 |
| `/export [id] [format]` | 导出会话（text/json） |
| `/skills [list]` | 列出已注册技能 |
| `/skills install <url>` | 安装外部 Skill |
| `/roles` | 列出可用角色 |
| `/provider` | 切换 LLM Provider |
| `/model` | 三层模型配置菜单 |
| `/cost` | Token 用量与成本 |
| `/status` | 系统状态总览 |
| `/help` | 显示帮助 |
| `/quit` | 退出 |

---

## 🛠️ 内置工具

| 工具 | 功能 | 类型 |
|------|------|------|
| `list_files` | 列出目录 | 只读 |
| `read_file` | 读取文件 | 只读 |
| `write_file` | 写入文件 | 写入 |
| `delete_file` | 删除文件 | 写入 |
| `move_file` / `copy_file` | 移动/复制 | 写入 |
| `glob_search` | Glob 搜索 | 只读 |
| `search_code` | 代码搜索 (ripgrep) | 只读 |
| `execute_command` | Shell 命令 | 执行 |
| `web_search` / `web_fetch` | Web 搜索/抓取 | 只读 |
| `git_status` / `git_diff` / `git_log` | Git 查询 | 只读 |
| `git_commit` | Git 提交 | 写入 |
| `file_edit` / `file_grep` | 文件编辑/搜索 | 写入/只读 |

---

## 🤖 Agent 角色

| 角色 | 允许工具 | 可 spawn |
|------|----------|----------|
| `general` | 全部 | 是 |
| `explore` | list_files, read_file, search_code | 否 |
| `plan` | list_files, read_file, write_file | 是 |
| `implementer` | read_file, write_file, search_code, execute_command, delete_file | 是 |
| `review` | read_file, search_code, execute_command | 否 |
| `verifier` | read_file, execute_command, search_code | 否 |

---

## 📦 依赖

### Python

| 包 | 用途 |
|----|------|
| `textual` | TUI 终端界面 |
| `langchain` / `langchain-core` / `langchain-openai` / `langchain-anthropic` | LLM 框架 |
| `langgraph` | Agent 工作流 |
| `pydantic` | 数据验证 |
| `mcp` | MCP 协议 |
| `fastapi` / `uvicorn` | Web 后端 |
| `playwright` | 浏览器自动化 |
| `python-dotenv` | 环境变量 |

### 前端

| 包 | 用途 |
|----|------|
| `react` / `react-dom` | UI 框架 |
| `react-markdown` / `rehype-highlight` | Markdown 渲染 |
| `zustand` | 状态管理 |
| `lucide-react` | 图标库 |
| `tailwindcss` | CSS 框架 |
| `vite` / `typescript` | 构建工具 |

---

## 🧪 测试

```bash
pytest
```

测试框架：pytest（asyncio_mode = auto）

| 文件 | 关注点 |
|------|--------|
| `tests/test_full_flow.py` | 端到端流程 |
| `tests/test_llm_flow.py` | LLM 流程 |
| `tests/test_sandbox.py` | 沙箱 |
| `tests/test_ime_compatibility.py` | 输入法兼容 |
| `tests/test_textual_input.py` | 终端输入 |

---

## 🔒 安全说明

- **审批系统**：所有写入类工具（write/delete/execute/git_commit）默认需要用户确认（Agent 模式）
- **沙箱**：跨平台沙箱（Linux namespace / macOS sandbox-exec / Windows Job Object）
- **安全守卫**：ML 分类 + 规则引擎，识别危险命令模式
- **YOLO 模式**：仅在确认环境可信时使用；安全守卫始终生效

---

## 📂 持久化文件

所有用户数据保存在 `~/.goat/` 目录：

| 文件 | 用途 |
|------|------|
| `setting.json` | Provider 配置、模型、hooks、MCP |
| `conversations.db` | 会话与消息 |
| `tasks.db` | 后台任务 |
| `memories.db` | 跨会话记忆 |
| `audit.db` | 工具调用审计 |
| `subagents.json` | 子 Agent 状态 |

---

## 📜 许可证

本项目以 Goat (山羊) 主题呈现。详细许可见 LICENSE。

---

## 🤝 贡献

欢迎提交 Issue 与 Pull Request。开发规则请参考 `openspec/change/` 下的变更提案。
