# Prompt 模板引擎 (PromptEngine)

## 变更内容

### 1. 新增 PromptEngine 核心
`subagent_demo/prompt_engine.py`

- **模板渲染引擎**：基于正则 `{variable}` 替换，仅替换已知变量，保留 prompt 中的其他花括号
- **内置变量**：`{cwd}`、`{os}`、`{language}`、`{agent_id}`、`{agent_name}`、`{depth}`、`{skills}`、`{model}`
- **渲染方法**：
  - `render_system(role, **vars)` — 渲染角色 system prompt + agent_suffix
  - `render_main_system(role, skills, **vars)` — 渲染主 Agent system prompt（含技能列表）
  - `render_with_tools(role, tools, **vars)` — 渲染带工具列表的 system prompt

### 2. 新增模板定义
`subagent_demo/prompt_templates.py`

- **集中管理**：所有角色 prompt 集中在 `TEMPLATES` 字典中，不再分散在代码各处
- **版本控制**：`TEMPLATES["version"] = 2`，支持后续版本迁移
- **7 个角色**：general / explore / plan / implementer / review / verifier / custom
- **公共部分**：`agent_suffix`（Agent ID/名称/深度）自动追加到每个角色 prompt 末尾
- **skills_header**：主 Agent 对话时自动注入可用技能列表
- **tool_descriptions**：集中管理的工具描述字典

### 3. 模板风格变化（Claude Code 精简适配风格）

| 维度 | 旧风格（删除约 1500 字） | 新风格 |
|------|------------------------|--------|
| 身份声明 | "你是一个通用 AI 助手，可以处理各种类型的任务。" | "你是通用 AI 助手。" |
| 工具说明 | "你可以使用所有可用工具，也可以创建子 Agent..." | 列表格式，一行一个工具 |
| 工作流 | 详细的"探索策略/实现准则/审查维度"段落 | 精简为 2-3 行核心方法 |
| 输出格式 | 完整 Markdown 模板示例 | 仅输出格式的标题骨架 |
| 冗余说明 | "当遇到复杂任务时，你可以使用..." | 直接"复杂任务用 agent_spawn 并行" |
| 后缀拼接 | 代码中手动拼接 agent_id/name/depth | 模板内置 `{agent_id}` 变量自动渲染 |

**精简效果**：每个角色的 prompt 从原来的 30-50 行缩减到 15-25 行，内容密度更高、指令更精准。

### 4. RoleDefinition 瘦身
`subagent_demo/subagent_roles.py`

- **删除**：约 230 行硬编码的 `system_prompt` 字符串（占源文件的 90% 内容）
- `get_role()` 现在通过 `PromptEngine.render_system()` 动态生成 system_prompt
- 支持传入额外变量 `get_role(role_type, cwd=..., agent_id=...)`

### 5. 集成到 Agent 循环

| 位置 | 变更 |
|------|------|
| `main.py` `_do_chat()` | 使用 `prompt_engine.render_main_system("general", skills=[...])` 替代手工 f-string 拼接 |
| `subagent_runtime.py` `_run_agent_loop()` | 使用 `prompt_engine.render_system(role_type, agent_id=..., agent_name=..., depth=...)` 替代手动 system_prompt + 后缀拼接 |
| `main.py` `_init_skills()` | 移除不再使用的 `system_prompt_template` 参数 |

## 修改的文件

| 文件 | 变更 |
|------|------|
| `subagent_demo/prompt_engine.py` | **新增** — 模板引擎核心 |
| `subagent_demo/prompt_templates.py` | **新增** — Claude Code 风格模板定义 |
| `subagent_demo/subagent_roles.py` | 删除 ~230 行硬编码 prompt，改用 PromptEngine 动态生成 |
| `subagent_demo/subagent_runtime.py` | 使用 PromptEngine 渲染子 Agent system prompt |
| `subagent_demo/__init__.py` | 导出 PromptEngine、prompt_engine、PROMPT_TEMPLATES |
| `main.py` | `_do_chat()` 使用引擎渲染；`_format_skills_prompt()` 已移除（被引擎接管）；skills 注册移除冗余 `system_prompt_template` |
| `tips/roadmap.md` | 更新总结表，添加 Prompt 模板引擎条目 |

## 使用示例

```python
from subagent_demo.prompt_engine import engine

# 主 Agent prompt（含技能列表）
prompt = engine.render_main_system("general", skills=["code_explorer — 探索", "code_writer — 编写"])

# 子 Agent prompt（含身份信息）
prompt = engine.render_system("explore", agent_id="a1b2", agent_name="蓝鲸", depth="1")

# 查看所有角色
for role in engine.list_roles():
    print(role["name"], role["display"])
```

## 模板变量一览

| 变量 | 来源 | 说明 |
|------|------|------|
| `{cwd}` | `Path.cwd()` | 当前工作目录 |
| `{os}` | `platform.system()` | 操作系统名称 |
| `{language}` | TEMPLATES["language"] | 回复语言指令 |
| `{agent_id}` | kwargs | Agent UUID 前 8 位 |
| `{agent_name}` | kwargs | Agent 显示名称 |
| `{depth}` | kwargs | 嵌套深度 |
| `{skills}` | kwargs | 格式化后的技能列表 |
| `{model}` | kwargs | 模型名称（预留） |

## 修改意义

- **Prompt 与代码分离**：修改 Agent 行为不再需要改 Python 代码，只需编辑 `prompt_templates.py`
- **风格统一**：所有角色使用一致的模板格式，消除旧版中 7 个角色 prompt 风格不一致的问题
- **指令精简**：参考 Claude Code 风格，用更少的 token 传达更精准的指令（平均每角色节约 40% 的 token 用量）
- **可维护性**：`subagent_roles.py` 从 259 行缩减到 101 行，核心逻辑更容易理解
- **可扩展**：新增角色只需在 `prompt_templates.py` 中添加一条模板记录
