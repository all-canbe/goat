# 工具系统

## 工具分类

| 类别 | 工具 |
|------|------|
| 文件操作 | list_files, read_file, write_file, delete_file, move_file, copy_file, get_file_info |
| 搜索 | glob_search, search_code |
| Git | git_status, git_diff, git_log, git_commit |
| Shell | execute_command |
| Web | web_search, web_fetch |
| 浏览器 | browser_navigate, browser_click, browser_type, browser_screenshot, browser_evaluate |
| Agent 管理 | agent_spawn, agent_eval, agent_list, agent_collect, agent_cancel |

## 工具实现规范

- 使用 `@tool` 装饰器 (langchain_core.tools)
- 参数用 `pydantic.BaseModel` 定义 (非原生类型)
- 返回 `str` (LLM 友好的文本)
- `BUILTIN_TOOLS` 字典集中注册所有工具
- 不可变操作优先，不产生副作用

## 安全与审批

- `execute_command` 有严格黑名单: rm -rf /, sudo, dd, chmod 777, chown, mkfs 等
- `write_file` / `delete_file` / `move_file` 受 `ToolApprovalSystem` 管控
- `PermissionMode` 控制审批强度
- `SafetyGuard` 在 YOLO 模式下仍生效 (黑名单不可绕过)
- `MLClassifier` 在 AUTO 模式做两阶段分类

## 命令安全

- 黑名单匹配即拒绝，YOLO 模式下也不可绕过
- 环境变量注入检测 (`API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY` 等)
- 交互式命令禁止执行
- 路径参数自动 `expanduser().resolve()` 归一化