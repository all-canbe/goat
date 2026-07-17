# RGoat 桌面端设计规范 (Style.md · 修订稿 v1.1)

> 本文件在视觉风格确定后锁定，作为后续所有 RGoat 桌面端前端开发的硬性约束。
> 未经团队评审，不得修改已锁定的条目。
> 参考对象：OpenCode Desktop、Claude Code Desktop、OpenAI Codex Desktop App、TRAE Work 设计库。

---

## 0. 文档定位与参考关系（新增）

### 0.1 参考对象的角色

| 对象 | 角色 | 说明 |
|---|---|---|
| OpenCode / Claude Code / Codex | **审美参考** | 提供"AI 编码桌面端"品类的设计共识（三栏、暗色、等宽、克制强调色），不作为 token 来源。 |
| TRAE Work 设计库 | **参考对象之一**（降级） | 原规范将其作为"硬约束源"，但 TRAE Work 为**亮色优先**库（其规则明令不加暗色覆盖），与 RGoat 暗色优先方向冲突。故降级为参考对象，仅借鉴其命名结构、圆角/间距取值与组件思路，不复用其亮色 token。 |

### 0.2 决策结论

- RGoat 维持**暗色优先**，自建暗色 token 体系（见第 2 节），不绑定 TRAE Work 的亮色 token。
- 若未来需要与 TRAE Work 对齐，按附录 A 的映射表派生暗色 token，并需经设计评审。
- 本规范所有 CSS 变量为 RGoat 自有命名，与 TRAE Work 命名风格相近以便未来映射，但不依赖其文件。

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
- **栅格系统**：以 `4px` 为基准间距单位，使用 `--space-*` 变量（见 2.1），取值 `4/8/12/16/20/24/32/48/64`。
- **密度适中**：单屏信息量控制在 3 秒内可扫视完成；密集信息区使用卡片/分组/留白区分，不依赖重边框。
- **标题栏整合**：窗口标题栏与应用标题栏合并，放置标签页/搜索/全局设置入口，避免多层 header 堆叠。标签页体系见 5.8。
- **底部 Composer**：输入框固定在底部中央，采用悬浮/卡片式 composer，承载模式选择、模型选择、附件、发送按钮。
- **对齐与层级**：同类元素左对齐、顶部标题左对齐；通过字号/字重/颜色明度建立层级，不依赖过多背景色块。

### 1.3 圆角与阴影

- 圆角：`4px`（小标签/图标按钮）、`8px`（按钮/输入框/卡片）、`12px`（悬浮面板/对话框）、`16px`（Composer 容器）。
- 阴影：使用柔和单层投影，分三档（token 见 2.1）：
  - `--shadow-sm`：Composer、吸顶卡片等需与底层画布区分的固定元素。
  - `--shadow-md`：dropdown、popover、toast。
  - `--shadow-lg`：dialog、modal。
- **不使用毛玻璃效果**。理由：跨平台一致性（Windows 原生不支持 backdrop-filter 的设备体验割裂）与性能开销，而非"与暗色冲突"（暗色下毛玻璃技术上可行）。

---

## 2. 色彩系统

### 2.1 设计 Token（CSS 变量）

```css
:root,
.dark {
  /* 背景 */
  --bg: #0c0c0c;              /* 主背景 */
  --bg-elevated: #111111;     /* 提升层背景（状态栏等） */
  --surface: #151515;         /* 侧边栏/面板/输入框背景 */
  --surface-hover: #1f1f1f;   /* hover 状态 */
  --surface-active: #27272a;  /* active/selected 状态 */

  /* 文字 */
  --text: #fafafa;            /* 主文字 */
  --text-secondary: #a1a1aa;  /* 次要文字 */
  --text-tertiary: #71717a;   /* 禁用/占位文字 */
  --text-on-brand: #ffffff;   /* 品牌色背景上的文字 */

  /* 品牌文字（用于暗色背景上的文字/链接/选中态文字，保证对比度） */
  --text-brand: #818cf8;      /* 链接、选中态文字、品牌强调文字 */
  --text-brand-hover: #a5b4fc;

  /* 边框与分隔 */
  --border: #27272a;          /* 边框 */
  --border-strong: #3f3f46;   /* 强调边框 */
  --divider: rgba(255,255,255,0.08); /* 细分割线 */

  /* 品牌/强调 */
  --primary: #6366f1;         /* 主色调：indigo，用于填充按钮背景、选中指示条 */
  --primary-hover: #818cf8;
  --primary-active: #4f46e5;
  --primary-subtle: rgba(99, 102, 241, 0.12); /* 选中背景 */

  /* 次级强调（可选，待品牌确认）——用于 Agent 运行态/流式指示，与 primary 区分 */
  --accent-signal: #22d3ee;
  --accent-signal-subtle: rgba(34, 211, 238, 0.12);

  /* 语义色 */
  --success: #22c55e;
  --success-subtle: rgba(34, 197, 94, 0.12);
  --warning: #f59e0b;
  --warning-subtle: rgba(245, 158, 11, 0.12);
  --error: #ef4444;
  --error-subtle: rgba(239, 68, 68, 0.12);
  --info: #38bdf8;

  /* 间距（4px 栅格，命名与 TRAE Work --spacer-* 可映射） */
  --space-0: 0px;
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --space-6: 24px;
  --space-8: 32px;
  --space-12: 48px;
  --space-16: 64px;

  /* 焦点环 */
  --ring: rgba(129, 140, 248, 0.55);   /* 由 primary-hover 派生，暗色下保证可见 */

  /* 阴影 */
  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.30);
  --shadow-md: 0 4px 16px rgba(0, 0, 0, 0.28);
  --shadow-lg: 0 8px 32px rgba(0, 0, 0, 0.36);
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
  --text-on-brand: #ffffff;
  --text-brand: #4f46e5;
  --text-brand-hover: #4338ca;
  --border: #e4e4e7;
  --border-strong: #d4d4d8;
  --divider: rgba(0,0,0,0.06);
  --primary: #4f46e5;
  --primary-hover: #4338ca;
  --primary-active: #3730a3;
  --primary-subtle: rgba(79, 70, 229, 0.10);
  --accent-signal: #0891b2;
  --accent-signal-subtle: rgba(8, 145, 178, 0.10);
  --success: #16a34a;
  --success-subtle: rgba(22, 163, 74, 0.10);
  --warning: #d97706;
  --warning-subtle: rgba(217, 119, 6, 0.10);
  --error: #dc2626;
  --error-subtle: rgba(220, 38, 38, 0.10);
  --info: #0284c7;

  --ring: rgba(79, 70, 229, 0.45);
  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.06);
  --shadow-md: 0 4px 16px rgba(0, 0, 0, 0.10);
  --shadow-lg: 0 8px 32px rgba(0, 0, 0, 0.14);
}
```

### 2.2 使用规则

- 背景层级严格遵循：`--bg` > `--bg-elevated` > `--surface`，禁止在 `--bg` 上直接使用接近 `--surface` 的自定义色。
- **品牌色用法区分**（解决暗色对比度问题）：
  - `--primary`（#6366f1）：仅用于**填充按钮背景**（配 `--text-on-brand` 白字）、选中项左侧 2px 指示条、发送按钮。不直接用作暗色背景上的文字色（对比度约 4.5，处于 WCAG AA 边界）。
  - `--text-brand`（#818cf8）：用于暗色背景上的**链接、选中态文字、品牌强调文字**（对比度 ≥ 7，满足 AAA）。
- 间距必须使用 `--space-*` 变量，禁止在组件中硬编码 `px` 间距值。
- 状态色优先使用 `*-subtle` 作为背景 + 对应实色作为文字/图标，不使用高饱和背景配白色文字。
- 避免高饱和荧光色作为主色大面积使用。
- 所有颜色引用必须使用 CSS 变量，禁止在组件中硬编码十六进制色值。

---

## 3. 字体与排版

### 3.1 字体栈

```css
body {
  font-family: "Inter", "SF Pro Text", "PingFang SC", system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  font-feature-settings: "cv02", "cv03", "cv04", "cv11";
}

/* 等宽：代码块、token 计数、路径、模型名 */
code, .font-mono {
  font-family: "JetBrains Mono", "SF Mono", "Fira Code", ui-monospace, monospace;
}
```

### 3.2 字号梯度

> 解决原规范"行高 1.6 下限"与多档字号行高（1.27/1.38）的自相矛盾。
> 拆分为"界面行高"（用于菜单/按钮/标签等界面元素，不承载连续阅读）与"正文行高(CJK)"（承载连续中文阅读，≥1.6）两列。

| 层级 | 尺寸 | 界面行高 | 正文行高(CJK) | 字重 | 用途 |
|------|------|----------|---------------|------|------|
| `2xs` | 11px | 16px | — | 500 | 状态栏、标签、时间戳 |
| `xs` | 12px | 16px | — | 400/500 | 辅助说明、Badge、会话副标题 |
| `sm` | 13px | 18px | 21px | 400/500 | 菜单项/按钮(界面)、紧凑正文(正文) |
| `base` | 14px | 20px | 24px | 400 | 输入框(界面)、主正文(正文) |
| `lg` | 16px | 24px | 26px | 500 | 小标题、会话标题 |
| `xl` | 18px | 28px | — | 600 | 对话框标题、空状态标题 |
| `2xl` | 20px | 30px | — | 600 | 品牌/大标题 |

### 3.3 排版规则

- 正文使用 `font-normal`（400），标题使用 `font-medium`（500）或 `font-semibold`（600），不使用超过 600 的字重。
- 文字颜色层级：`--text`（主内容）> `--text-secondary`（描述/时间）> `--text-tertiary`（占位/禁用）。
- **行高适用规则**（修订）：
  - 凡承载**连续中文阅读**的正文（`sm`/`base`/`lg`）使用"正文行高(CJK)"列，均 ≥1.6。
  - 菜单项、按钮、输入框、标签、Badge、状态栏等**界面元素**使用"界面行高"列，不受 1.6 下限约束（因其不承载连续阅读）。
  - `2xs`/`xs`/`xl`/`2xl` 仅用于界面/标题，不承载连续正文，仅使用界面行高。
- 路径、模型名、token 数使用等宽字体。
- 长文本需做溢出处理（`text-overflow: ellipsis` 或换行控制），关键文案不得截断语义。

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

- **Primary 按钮**：圆角 `8px`，背景 `--primary`，hover `--primary-hover`，active `--primary-active`，文字 `--text-on-brand`。仅用于发送、确认、保存等核心正向操作。
- **Secondary 按钮**（修订，修正原 hover 方向反转问题）：圆角 `8px`，默认背景 `--surface-hover`（#1f1f1f），hover 背景 `--surface-active`（#27272a，更亮），active/pressed 背景 `--border-strong`（#3f3f46），文字 `--text`。
- **Ghost 按钮**：无背景，hover 背景 `--surface-hover`，用于图标按钮、工具栏。
- **Danger 按钮**：使用 `--error` 背景，文字白色，仅用于删除、取消执行等破坏性操作。
- 按钮内图标与文字间距 `--space-2`（8px）。
- 图标按钮最小点击区域 `28px × 28px`。
- 所有按钮必须支持 `:focus-visible`（见第 9 节）。

### 5.2 输入框 / Composer

- 输入框使用 `--surface` 背景、`--border` 边框，圆角 `12px`。
- focus 状态使用 `--primary` 边框（1px）；`:focus-visible` 额外加 `--ring` 焦点环（见第 9 节），不使用外发光。
- Composer 固定在底部，水平居中，最大宽度 `840px`，与边缘保持 `--space-4`（16px）间距。
- Composer 容器使用 `--shadow-sm` 与底层画布区分。
- Composer 承载：模式选择、附件、文本域、模型选择、发送按钮。
- 文本域高度自适应，最小 `48px`，最大 `200px`。
- Placeholder 使用 `--text-tertiary`。

### 5.3 侧边栏

- 固定宽度 `260px`，折叠时宽度 `0`（隐藏）。
- 顶部为品牌区 + 全局操作（新建会话、设置）。
- 会话列表按时间分组：今天 / 昨天 / 最近 7 天 / 更早。
- 当前会话使用 `--primary-subtle` 背景 + `--primary` 左侧 `2px` 指示条，不使用完整实色背景。
- 列表项 hover 使用 `--surface-hover`。
- 右键菜单支持重命名、删除、归档。

### 5.4 标签 / Tabs

- 使用 pill 风格或下划线风格，禁止把 tab 按钮填充实色作为 active 状态。
- Active 状态：背景 `--surface-active` 或下划线 `--primary`，文字 `--text`。
- Inactive 状态：文字 `--text-secondary`，hover 背景 `--surface-hover`。

### 5.5 状态栏

- 高度 `28px`，背景 `--bg-elevated`，顶部 `1px` `--border` 分隔。
- 左侧：连接状态指示灯 + 当前项目/工作区。
- 中部：当前模型、模式、Token 用量（紧凑显示）。
- 右侧：待审批数、变更数、工具调用数（仅在有非零值时高亮）。
- 避免使用 `|` 竖线分隔，改用 `--space-2`（8px）间距或 dot 分隔。

### 5.6 消息气泡

- 用户消息：右对齐，背景 `--surface-active`，圆角 `12px`（右上 `4px`）。
- Agent 消息：左对齐，无背景，使用左侧头像/品牌标识区分。
- **Agent 长回复视觉锚点**（新增）：Agent 主体文本不使用背景色；段落之间使用 `--divider` 细线或 `--space-4`（16px）留白分段，避免长回复在暗色下失去视觉锚点。连续多轮 Agent 消息之间以 `--divider` 顶部分隔线区分。
- 工具调用卡片：背景 `--surface`，圆角 `8px`，左侧 `2px` 状态色指示条。
- 代码块：使用 `rehype-highlight` + 暗色主题，圆角 `8px`，带复制按钮。

### 5.7 对话框 / Modal

- 背景遮罩 `rgba(0,0,0,0.6)`。
- 面板背景 `--surface`，圆角 `12px`，`--shadow-lg`，最大宽度限制（如 `560px`）。
- 标题使用 `xl` 字号 `font-semibold`。
- 关闭按钮在右上角，图标 `X`。

### 5.8 标题栏标签页体系（新增，对齐 1.2 与 7.1）

- 标题栏整合窗口控制与应用标签页，支持多会话/多线程以标签页形式并存。
- 标签页样式沿用 5.4 下划线风格：active 项底部 `2px` `--primary` 下划线 + 文字 `--text`；inactive 文字 `--text-secondary`，hover 背景 `--surface-hover`。
- 标签页最小宽度 `120px`，最大宽度 `240px`，超出显示省略号。
- 标签页关闭按钮（`X`，14px）仅在 hover 当前标签页时出现。
- 新建标签页入口（`+`）固定在标签栏末尾，对应快捷键 `Cmd/Ctrl + Shift + N`。

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
- Agent 流式输出使用打字光标/脉冲指示器（可选用 `--accent-signal` 作为流式态指示色），明确告知正在生成。

---

## 7. 交互与快捷键

### 7.1 全局快捷键

- `Cmd/Ctrl + N`：新建会话。
- `Cmd/Ctrl + Shift + N`：新建标签页/线程（见 5.8）。
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
- 亮色主题为可选模式，必须同步调整语义色 token（见 2.1 `.light`）。
- 所有颜色引用必须使用 CSS 变量，禁止在组件中硬编码十六进制色值。
- 间距、圆角、阴影、焦点环同样必须使用变量，禁止硬编码。

---

## 9. 焦点与可访问性（新增）

### 9.1 焦点环

- 所有可交互元素（按钮、链接、输入框、tab、菜单项）必须支持 `:focus-visible`。
- 焦点环规范：`2px` `--ring` 实线环，向外偏移 `2px`（与元素之间留 `2px` 透明间隔，避免与元素边框粘连）。
- 实现示例：

```css
:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
}
```

- `:focus`（非 `:focus-visible`）用于鼠标点击时不显示环，保持整洁；键盘导航时显示。
- 禁止用 `outline: none` 移除焦点而不提供替代环。

### 9.2 对比度

- 正文文字（`--text` on `--bg`）对比度 ≥ 7（AAA）。
- 次要文字（`--text-secondary`）对比度 ≥ 4.5（AA）。
- 暗色背景上的品牌色文字必须使用 `--text-brand`（#818cf8，对比度 ≥ 7），不得使用 `--primary`（#6366f1，对比度约 4.5，边界）。
- 占位文字（`--text-tertiary`）仅用于非关键提示，对比度 ≥ 3。

### 9.3 最小点击区域

- 图标按钮最小 `28px × 28px`（见 5.1）。
- 列表项、菜单项最小高度 `32px`。

---

## 10. 风格锁定机制

### 10.1 锁定条件

以下条件全部满足时，本文件进入锁定状态：

1. 色彩 token（背景、表面、文字、品牌、语义色、间距、阴影、焦点环）已确定并通过评审。
2. 字体方案（字体族、字号梯度、界面/正文双行高）已确定。
3. 间距系统、圆角、阴影等基础 token 已定义为变量。
4. 至少完成核心页面（会话列表、聊天画布、Composer、状态栏）的设计稿/实现评审。

### 10.2 锁定后规则

- 本文件中标注的数值、选型、规则为硬性约束，PR 中不得违反。
- 如需调整，须提交变更提案，经设计 + 前端共同评审后修改。
- 新增页面/组件必须对照本文件逐项检查，不符合项不得合入。

---

## 11. 检查清单

每个 RGoat 桌面端 PR 合入前，对照以下项自查：

- [ ] 布局遵循 4px 栅格，间距使用 `--space-*` 变量，核心布局符合"左侧边栏 + 中央画布 + 右侧面板"形态。
- [ ] 无 Emoji 字符出现，图标均来自 `lucide-react`。
- [ ] 颜色引用使用 CSS 变量，未在组件中硬编码色值。
- [ ] 暗色背景上的品牌色文字使用 `--text-brand`，未误用 `--primary`。
- [ ] 间距、圆角、阴影、焦点环均使用变量，未硬编码 `px` 间距。
- [ ] 动画时长在 150~250ms，已处理 `prefers-reduced-motion`。
- [ ] 异步加载场景使用骨架屏或 loading 状态。
- [ ] 按钮/输入框/卡片圆角符合规范（4/8/12/16px）。
- [ ] 状态栏信息精简，不使用 `|` 竖线分隔，高亮仅在非零/异常时出现。
- [ ] Composer 固定在底部，宽度受限，支持自适应高度，使用 `--shadow-sm`。
- [ ] 当前会话指示使用 subtle 背景 + 左侧指示条，而非大面积实色。
- [ ] 正文中文行高使用"正文行高(CJK)"列（≥1.6），界面元素使用"界面行高"列。
- [ ] 关键文案已做溢出处理。
- [ ] 所有可交互元素支持 `:focus-visible` 焦点环（2px `--ring`，offset 2px）。
- [ ] Secondary 按钮 hover 方向正确（hover 比 default 更亮）。
- [ ] 标签页体系符合 5.8，新建标签页入口与 `Cmd/Ctrl + Shift + N` 对应。

---

## 附录 A：与 TRAE Work token 映射（可选，未来对齐用）

> 仅当 RGoat 决定与 TRAE Work 设计库对齐时使用。当前 RGoat 自建 token，不依赖此映射。

| RGoat token | TRAE Work 对应 | 说明 |
|---|---|---|
| `--bg` | `--bg-base-default`（亮）/ 需派生暗色 | TRAE Work 仅提供亮色，暗色需自行派生 |
| `--surface` | `--bg-base-secondary` | |
| `--surface-hover` | `--bg-overlay-l1` | TRAE Work 用 rgba overlay，暗色下建议改固定色阶 |
| `--primary` | `--bg-brand` (#4B3FE3) | 色值不同，需统一决策 |
| `--text-brand` | `--text-brand` (#4B3FE3 亮) | 暗色需派生为 #818cf8 |
| `--space-*` | `--spacer-*` | 命名一致，可直接映射 |
| `--radius-*` | `--radius-*` | 取值子集一致 |
| `--shadow-*` | TRAE Work 未定义暗色阴影 | 需补充 |
| `--ring` | TRAE Work 未定义 | RGoat 新增，TRAE Work 需补充 |

---

## 修订记录

### v1.1（本次修订）

1. **TRAE Work 关系降级**：从"硬约束源"降为"参考对象之一"，新增第 0 节说明决策与理由，新增附录 A 的 token 映射。
2. **行高矛盾修复**：第 3.2 节拆分"界面行高"与"正文行高(CJK)"两列，正文行高均 ≥1.6，界面元素豁免；第 3.3 节明确适用规则。
3. **间距 token 补全**：第 2.1 节新增 `--space-*` 变量，第 2.2/8 节要求间距必须用变量，检查清单新增对应项。
4. **焦点环规范新增**：新增第 9 节，定义 `--ring` 与 `:focus-visible`（2px 环 + 2px offset），检查清单新增对应项。
5. **毛玻璃措辞修正**：第 1.3 节理由改为"跨平台一致性 + 性能"，而非"与暗色冲突"。
6. **暗色品牌色对比度修复**：第 2.1 节新增 `--text-brand`（#818cf8），第 2.2 节区分 `--primary`（按钮背景）与 `--text-brand`（文字）用途，第 9.2 节给出对比度要求。
7. **Secondary 按钮 hover 方向修复**：第 5.1 节修正原"default=--surface-active, hover=--surface-hover"的反转问题，改为 default=--surface-hover, hover=--surface-active。
8. **Composer 阴影定义**：第 5.2 节明确 Composer 使用 `--shadow-sm`，第 1.3 节新增三档阴影 token。
9. **Agent 长回复视觉锚点**：第 5.6 节新增段落间分隔规则。
10. **标题栏标签页体系**：新增第 5.8 节，对齐 1.2 与 7.1 的标签页/快捷键。
11. **次级强调色（可选）**：第 2.1 节新增 `--accent-signal`，用于 Agent 运行态/流式指示，增强品牌辨识度，待品牌确认。
