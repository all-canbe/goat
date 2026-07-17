# RGoat 桌面端设计规范 (Style.md)

> 本文件在视觉风格确定后锁定，作为后续所有 RGoat 桌面端前端开发的硬性约束。
> 未经团队评审，不得修改已锁定的条目。
> 参考对象：OpenCode Desktop v2、Claude Code Desktop（2026.04  redesign）、OpenAI Codex Desktop App。

---

## 1. 设计语言与布局原则

### 1.1 设计关键词

- **Agent Command Center**：为"多任务并行 + 人在回环"设计，信息密度高但不拥挤。
- **IDE 工作台**：左侧导航、中间对话/执行、右侧工具面板的三栏布局为默认形态。
- **暗色优先**：默认暗色主题，亮色主题仅作为可切换模式存在。
- **克制的强调色**：品牌色仅用于关键操作和状态指示，不大面积铺色。

### 1.2 布局原则

- **三栏基准布局**：
  - 左侧边栏：会话/项目导航、搜索、新建入口（宽度 `260px`，可折叠）。
  - 中央画布：对话时间线、执行日志、当前任务主视图（弹性填充）。
  - 右侧面板：文件树、Diff 审阅、变更列表、预览、终端等辅助视图（默认 `320px`，可折叠/隐藏）。
- **栅格系统**：以 `4px` 为基准间距单位，使用 `4/8/12/16/20/24/32/48/64`。
- **密度适中**：单屏信息量控制在 3 秒内可扫视完成；密集信息区使用卡片/分组/留白区分，不依赖重边框。
- **标题栏整合**：窗口标题栏与应用标题栏合并，放置标签页/搜索/全局设置入口，避免多层 header 堆叠。
- **底部 Composer**：输入框固定在底部中央，采用悬浮/卡片式 composer，承载模式选择、模型选择、附件、发送按钮。
- **对齐与层级**：同类元素左对齐、顶部标题左对齐；通过字号/字重/颜色明度建立层级，不依赖过多背景色块。

### 1.3 圆角与阴影

- 圆角：`4px`（小标签/图标按钮）、`8px`（按钮/输入框/卡片）、`12px`（悬浮面板/对话框）、`16px`（Composer 容器）。
- 阴影：使用柔和单层投影，仅用于悬浮层（dropdown、dialog、toast）。
  - 示例：`0 4px 24px rgba(0, 0, 0, 0.24)`。
- 不使用毛玻璃效果（避免与暗色主题冲突）。

---

## 2. 色彩系统

### 2.1 设计 Token（CSS 变量）

```css
:root,
.dark {
  /* 背景 */
  --bg: #0c0c0c;              /* 主背景 */
  --bg-elevated: #111111;     /* 提升层背景 */
  --surface: #151515;         /* 侧边栏/面板/输入框背景 */
  --surface-hover: #1f1f1f;   /* hover 状态 */
  --surface-active: #27272a;  /* active/selected 状态 */

  /* 文字 */
  --text: #fafafa;            /* 主文字 */
  --text-secondary: #a1a1aa;  /* 次要文字 */
  --text-tertiary: #71717a;   /* 禁用/占位文字 */

  /* 边框与分隔 */
  --border: #27272a;          /* 边框 */
  --border-strong: #3f3f46;   /* 强调边框 */
  --divider: rgba(255,255,255,0.08); /* 细分割线 */

  /* 品牌/强调 */
  --primary: #6366f1;         /* 主色调：indigo */
  --primary-hover: #818cf8;
  --primary-active: #4f46e5;
  --primary-subtle: rgba(99, 102, 241, 0.12); /* 选中背景 */

  /* 语义色 */
  --success: #22c55e;
  --success-subtle: rgba(34, 197, 94, 0.12);
  --warning: #f59e0b;
  --warning-subtle: rgba(245, 158, 11, 0.12);
  --error: #ef4444;
  --error-subtle: rgba(239, 68, 68, 0.12);
  --info: #38bdf8;
}

.light {
  --bg: #ffffff;
  --bg-elevated: #fafafa;
  --surface: #f4f4f5;
  --surface-hover: #e4e4e7;
  --surface-active: #d4d4d8;
  --text: #18181b;
  --text-secondary: #52525b;
  --text-tertiary: #a1a1aa;
  --border: #e4e4e7;
  --border-strong: #d4d4d8;
  --divider: rgba(0,0,0,0.06);
  --primary: #4f46e5;
  --primary-hover: #4338ca;
  --primary-active: #3730a3;
  --primary-subtle: rgba(79, 70, 229, 0.10);
  --success: #16a34a;
  --success-subtle: rgba(22, 163, 74, 0.10);
  --warning: #d97706;
  --warning-subtle: rgba(217, 119, 6, 0.10);
  --error: #dc2626;
  --error-subtle: rgba(220, 38, 38, 0.10);
}
```

### 2.2 使用规则

- 背景层级严格遵循：`--bg` > `--bg-elevated` > `--surface`，禁止在 `--bg` 上直接使用接近 `--surface` 的自定义色。
- 品牌色仅用于：主按钮、当前选中项、发送按钮、关键状态指示、链接。
- 避免高饱和荧光色（如原 `#e94560`）作为主色大面积使用。
- 状态色优先使用 `*-subtle` 作为背景 + 对应实色作为文字/图标，不使用高饱和背景配白色文字。

---

## 3. 字体与排版

### 3.1 字体栈

```css
body {
  font-family: "Inter", "SF Pro Display", system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  font-feature-settings: "cv02", "cv03", "cv04", "cv11";
}

/* 等宽：代码块、token 计数、路径 */
code, .font-mono {
  font-family: "JetBrains Mono", "SF Mono", "Fira Code", ui-monospace, monospace;
}
```

### 3.2 字号梯度

| 层级 | 尺寸 | 行高 | 字重 | 用途 |
|------|------|------|------|------|
| `2xs` | 11px | 14px | 500 | 状态栏、标签、时间戳 |
| `xs` | 12px | 16px | 400/500 | 辅助说明、Badge、会话副标题 |
| `sm` | 13px | 18px | 400/500 | 正文、菜单项、按钮 |
| `base` | 14px | 20px | 400 | 主正文、输入框文字 |
| `lg` | 16px | 24px | 500 | 小标题、会话标题 |
| `xl` | 18px | 26px | 600 | 对话框标题、空状态标题 |
| `2xl` | 20px | 28px | 600 | 品牌/大标题 |

### 3.3 排版规则

- 正文使用 `font-normal`（400），标题使用 `font-medium`（500）或 `font-semibold`（600）。
- 不使用超过 `600` 的字重。
- 文字颜色层级：`--text`（主内容）> `--text-secondary`（描述/时间）> `--text-tertiary`（占位/禁用）。
- 路径、模型名、token 数使用等宽字体。
- 中文内容最小行高不低于 `1.6`。

---

## 4. 图标规范

### 4.1 图标库

- 统一使用 `lucide-react`。
- 禁止在代码、文案、注释、UI 文本中使用 Emoji。

### 4.2 图标尺寸

| 场景 | 尺寸 |
|------|------|
| 内联文字旁 | 14px |
| 按钮内图标 | 16px |
| 标题栏/工具栏 | 18px |
| 空状态/大图标 | 24px |

### 4.3 图标使用规则

- 图标颜色继承父级 `currentColor`，不硬编码。
- 语义不明确的纯图标按钮必须提供 `aria-label` 或 `title`。
- 同一组操作图标尺寸保持一致。
- 状态指示优先使用图标 + 颜色（如绿色圆点），不使用 Emoji 或自定义图形。

---

## 5. 组件规范

### 5.1 按钮

- **Primary 按钮**：圆角 `8px`，背景 `--primary`，hover `--primary-hover`，active `--primary-active`，文字白色。
  - 仅用于发送、确认、保存等核心正向操作。
- **Secondary 按钮**：圆角 `8px`，背景 `--surface-active`，文字 `--text`，hover `--surface-hover`。
  - 用于次要操作。
- **Ghost 按钮**：无背景，hover 背景 `--surface-hover`，用于图标按钮、工具栏。
- **Danger 按钮**：使用 `--error` 背景，仅用于删除、取消执行等破坏性操作。
- 按钮内图标与文字间距 `8px`。
- 图标按钮最小点击区域 `28px × 28px`。

### 5.2 输入框 / Composer

- 输入框使用 `--surface` 背景、`--border` 边框，圆角 `12px`。
- focus 状态使用 `--primary` 边框（`1px`），不使用外发光。
- Composer 固定在底部，水平居中，最大宽度 `840px`，与边缘保持 `16px` 间距。
- Composer 承载：模式选择、附件、文本域、模型选择、发送按钮。
- 文本域高度自适应，最小 `48px`，最大 `200px`。
- Placeholder 使用 `--text-tertiary`。

### 5.3 侧边栏

- 固定宽度 `260px`，折叠时宽度 `0`（隐藏）。
- 顶部为品牌区 + 全局操作（新建会话、设置）。
- 会话列表按时间分组：今天 / 昨天 / 最近 7 天 / 更早。
- 当前会话使用 `--primary-subtle` 背景 + `--primary` 左侧 `2px` 指示条，不使用完整粉色背景。
- 列表项 hover 使用 `--surface-hover`。
- 右键菜单支持重命名、删除、归档。

### 5.4 标签 / Tabs

- 使用 pill 风格或下划线风格，禁止原方案中把 tab 按钮填充实色作为 active 状态。
- Active 状态：背景 `--surface-active` 或下划线 `--primary`，文字 `--text`。
- Inactive 状态：文字 `--text-secondary`，hover 背景 `--surface-hover`。

### 5.5 状态栏

- 高度 `28px`，背景 `--bg-elevated`，顶部 `1px` `--border` 分隔。
- 左侧：连接状态指示灯 + 当前项目/工作区。
- 中部：当前模型、模式、Token 用量（紧凑显示）。
- 右侧：待审批数、变更数、工具调用数（仅在有非零值时高亮）。
- 避免使用 `|` 竖线分隔，改用 `8px` 间距或 dot 分隔。

### 5.6 消息气泡

- 用户消息：右对齐，背景 `--surface-active`，圆角 `12px`（右上 `4px`）。
- Agent 消息：左对齐，无背景，使用左侧头像/品牌标识区分。
- 工具调用卡片：背景 `--surface`，圆角 `8px`，左侧 `2px` 状态色指示条。
- 代码块：使用 `rehype-highlight` + 暗色主题，圆角 `8px`，带复制按钮。

### 5.7 对话框 / Modal

- 背景遮罩 `rgba(0,0,0,0.6)`。
- 面板背景 `--surface`，圆角 `12px`，最大宽度限制（如 `560px`）。
- 标题使用 `xl` 字号 `font-semibold`。
- 关闭按钮在右上角，图标 `X`。

---

## 6. 动画与加载态

### 6.1 动画

- 微交互优先使用 CSS `transition`，时长 `150ms ~ 250ms`。
- 列表加载/页面切换使用 `transform` 和 `opacity`，避免触发重排。
- 缓动函数：`cubic-bezier(0.4, 0, 0.2, 1)`（标准）或 `cubic-bezier(0, 0, 0.2, 1)`（decelerate）。
- 检测 `prefers-reduced-motion`，在系统偏好减少动画时关闭非必要动效。

### 6.2 加载态

- 页面级加载使用骨架屏或品牌 spinner，不使用空白闪烁。
- 按钮加载态禁用按钮并展示 spinner，禁止重复提交。
- 列表异步加载使用局部 spinner 或 skeleton item。
- Agent 流式输出使用打字光标/脉冲指示器，明确告知正在生成。

---

## 7. 交互与快捷键

### 7.1 全局快捷键

- `Cmd/Ctrl + N`：新建会话。
- `Cmd/Ctrl + Shift + N`：新建标签页/线程。
- `Cmd/Ctrl + ;`：打开 Side Chat（分支对话）。
- `Cmd/Ctrl + B`：切换侧边栏。
- `Cmd/Ctrl + J`：切换右侧面板。
- `Cmd/Ctrl + K`：打开命令面板。
- `Cmd/Ctrl + /`：显示快捷键帮助。
- `Esc`：关闭对话框/面板/菜单。

### 7.2 右键与上下文菜单

- 会话项右键：重命名、复制、归档、删除。
- 文件树项右键：打开、对比、接受/回滚变更。
- 消息右键：复制、引用、删除。

---

## 8. 主题与模式

- 默认暗色主题，通过 `dark` class 或系统偏好自动切换。
- 亮色主题为可选模式，必须同步调整语义色 token。
- 所有颜色引用必须使用 CSS 变量，禁止在组件中硬编码十六进制色值。

---

## 9. 风格锁定机制

### 9.1 锁定条件

以下条件全部满足时，本文件进入锁定状态：

1. 色彩 token（背景、表面、文字、品牌、语义色）已确定并通过评审。
2. 字体方案（字体族、字号梯度、行高）已确定。
3. 间距系统、圆角、阴影等基础 token 已定义。
4. 至少完成核心页面（会话列表、聊天画布、Composer、状态栏）的设计稿/实现评审。

### 9.2 锁定后规则

- 本文件中标注的数值、选型、规则为硬性约束，PR 中不得违反。
- 如需调整，须提交变更提案，经设计 + 前端共同评审后修改。
- 新增页面/组件必须对照本文件逐项检查，不符合项不得合入。

---

## 10. 检查清单

每个 RGoat 桌面端 PR 合入前，对照以下项自查：

- [ ] 布局遵循 4px 栅格，核心布局符合"左侧边栏 + 中央画布 + 右侧面板"形态。
- [ ] 无 Emoji 字符出现，图标均来自 `lucide-react`。
- [ ] 颜色引用使用 CSS 变量，未在组件中硬编码色值。
- [ ] 动画时长在 150~250ms，已处理 `prefers-reduced-motion`。
- [ ] 异步加载场景使用骨架屏或 loading 状态。
- [ ] 按钮/输入框/卡片圆角符合规范（4/8/12/16px）。
- [ ] 状态栏信息精简，不使用 `|` 竖线分隔，高亮仅在非零/异常时出现。
- [ ] Composer 固定在底部，宽度受限，支持自适应高度。
- [ ] 当前会话指示使用 subtle 背景 + 左侧指示条，而非大面积实色。
- [ ] 中文内容行高不低于 1.6，关键文案已做溢出处理。
