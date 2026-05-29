# 代码风格

## Python 约定

- 文件头: `from __future__ import annotations`
- 枚举: `class RoleType(Enum):`，而非字符串常量或类常量
- 数据容器: `@dataclass` 或 `pydantic.BaseModel`，不写手动的 `__init__`
- 类型注解: 所有函数和方法的参数/返回值必须标注类型
- 导入顺序: 标准库 → 第三方 → 项目内部 (空行分隔)
- 行宽: 100 字符

## 命名规范

| 类别 | Python | TypeScript |
|------|--------|------------|
| 文件名/目录 | 蛇形小写 `subagent_manager.py` | PascalCase `ChatArea.tsx` |
| 类名 | PascalCase `SubAgentManager` | PascalCase `WSClient` |
| 函数/方法 | 蛇形小写 `load_skill_from_directory` | camelCase `appendStreamToken` |
| 常量 | 大写蛇形 `MAX_AGENT_TURNS` | 大写蛇形 `MUTATION_TOOLS` |
| 枚举成员 | 大写蛇形 `RoleType.GENERAL` | — |
| 接口/类型 | — | PascalCase `PendingApproval` |
| 私有函数 | 下划线前缀 `_build_subagent_tools` | — |

## 代码组织

- 一个文件一个主要类/模块，不超过 500 行
- 函数不超过 60 行
- 用 `# =====` 注释块分隔文件内的大段
- 工具函数: `@tool` 装饰器 (langchain_core) + pydantic.BaseModel 入参
- dataclass 默认用 `field(default_factory=...)` 避免可变默认值

## 错误处理

- 不捕获 `BaseException` 或裸 `except:`
- 异步代码用 `asyncio.CancelledError` 处理取消
- 工具调用异常捕获后转为 `ToolMessage` 返回给 LLM
- 审批拒绝后发送 `ToolMessage(content=消息)`，不中断流程

## TypeScript / React 约定

- 函数组件 + Hooks，不使用 class 组件
- 状态管理: Zustand store，`create<StoreInterface>((set, get) => ({...}))`
- 样式: Tailwind CSS 工具类，不写自定义 CSS（`index.css` 除外）
- 路径别名: `@/` 映射到 `src/`（在 `vite.config.ts` 中配置）
- 类型定义集中在 `types/index.ts`，用 `interface` 定义对象类型
- WebSocket 消息类型用字符串字面量分发 (`chat.stream`, `chat.response`, `tool.require_approval`)