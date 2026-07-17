# RGoat 桌面端 UI/UX 优化实施方案（基于现有功能修订版）

> 依据：[RGoat-Style.md](file:///e:/WorkBuddyOutput/2026-06-25-10-54-30/goat_rust/rgoat-desktop/RGoat-Style.md)（v1.1）
> 目标：在**不破坏现有功能**的前提下，将当前桌面端逐步改造为符合 OpenCode / Claude Code / Codex 设计共识的"Agent Command Center"工作台，并明确技术栈取舍。

---

## 一、现状总览：功能边界与实现

当前桌面端已实现的核心功能（基于对源码的梳理）：

| 模块 | 已实现的文件 | 现有能力 |
|---|---|---|
| **布局外壳** | `AppLayout.tsx` | 两栏布局：顶部独立 Header + 左侧 Sidebar + 中央 ChatArea + 底部 StatusBar。Sidebar 可折叠并持久化。 |
| **会话导航** | `Sidebar.tsx`、`sessionStore.ts` | 会话列表、搜索、新建、重命名、删除；Sidebar 内分 Sessions/Files/Changes/Tasks 四栏。 |
| **聊天画布** | `ChatArea.tsx`、`MessageList.tsx`、`MessageBubble.tsx`、`StreamingThought.tsx` | 用户/Agent/工具/系统/错误消息、流式输出、空状态示例提问、上下文压缩通知。 |
| **输入 composer** | `InputPanel.tsx` | Provider 下拉选择、Mode 切换（Agent/Plan/Flow/YOLO）、Slash 命令下拉、发送/取消、自适应高度文本域。 |
| **工具与变更** | `ToolCallCard.tsx`、`FileTree.tsx`、`ChangesPanel.tsx`、`ChangeItem.tsx`、`changesStore.ts` | 工具调用卡片（含风险徽章）、文件树、会话内变更列表。 |
| **审批与计划** | `ApprovalDialog.tsx`、`ApprovalScopeSelector.tsx`、`PlanPreviewDialog.tsx` | 工具执行审批弹窗、Plan Mode 计划预览。 |
| **全局交互** | `CommandPalette.tsx`、`useGlobalShortcuts.ts`、`SettingsDialog.tsx`、`themeStore.ts` | 命令面板、全局快捷键、设置页（Provider/Mode/主题/关于）、暗/亮/跟随系统主题。 |
| **状态反馈** | `StatusBar.tsx`、`Toaster.tsx` | 连接状态、模型、模式、token 用量、审批/变更/工具统计；Toast 通知。 |
| **后端桥接** | `tauri-bridge.ts` | 通过 Tauri IPC 调用 Rust 后端命令。 |

当前技术栈：

| 层级 | 当前选择 | 版本 |
|---|---|---|
| 桌面框架 | Tauri | v2（Rust 后端） |
| 前端框架 | React | 18.3.1 |
| 构建工具 | Vite | 5.3.1 |
| 样式方案 | Tailwind CSS | 3.4.4 |
| 状态管理 | Zustand | 4.5.2 |
| 图标库 | lucide-react | 0.396.0 |
| Markdown 渲染 | react-markdown + remark-gfm + rehype-highlight | 9.0.1 / 4.0.0 / 7.0.0 |

---

## 二、技术栈主流性评估与建议

### 2.1 桌面框架：Tauri v2

**结论：当前选择符合 2026 年主流趋势，建议保留。**

- Tauri 是 2026 年增长最快的桌面跨平台框架之一，与 Electron 相比具有显著更小的包体积（典型 Tauri 应用 3–10 MB，Electron 120–200 MB）和更低的空闲内存占用。
- Tauri v2 已稳定，支持 Windows/macOS/Linux，并新增 iOS/Android 支持，为后续可能的移动端扩展保留空间。
- Electron 仍是生态最成熟的选项（VS Code、Slack、Discord 等），但包体积大、内存占用高。RGoat 作为 AI 编码助手，用户通常对启动速度和资源占用敏感，Tauri 的轻量特性更匹配。
- 由于项目本身已是 Rust 后端（`rgoat-core`），Tauri 的 Rust 技术栈与现有团队能力一致，迁移 Electron 反而会增加技术栈异构性。

**建议**：保持 Tauri v2，无需迁移。后续可关注 Tauri 插件生态（如自动更新、日志、Updater）是否满足长期需求。

### 2.2 前端框架：React 18.3.1

**结论：当前选择可用，但 React 19 已进入稳定版，建议作为中期升级项评估。**

- React 19 于 2024 年底发布，2026 年已成熟。主要改进包括：Actions / `useActionState` / `useOptimistic` / `use()` hook、自动批处理增强、React Compiler（自动 memoization）、ref as prop 等。
- 对于 RGoat 桌面端这类客户端 SPA，React 19 的 Actions、`useOptimistic` 和 React Compiler 对表单/流式交互有实际收益，但**不是必须立即升级**。
- React 18 仍受支持，但长期看迁移到 React 19 是合理方向，可获得更长的安全支持周期和更好的 AI 代码生成兼容性。

**建议**：
- **短期（本优化周期）**：保持 React 18，优先完成 UI 改造。
- **中期**：在 UI 改造稳定后，单独规划 React 18 → 19 升级。升级前需验证 Tauri 前端构建、`@tauri-apps/api` 兼容性以及所有 Zustand store 的渲染行为。

### 2.3 样式方案：Tailwind CSS 3.4.4

**结论：Tailwind 仍是 2026 年最主流的样式方案，但 v4 已发布，升级有收益但非 urgent。**

- Tailwind CSS 在 2026 年仍是 React 项目的事实标准。当前 RGoat 使用 Tailwind 3 + CSS 变量定义主题 token，与 Style.md 的 token 化设计方向一致。
- Tailwind v4（2025 年 1 月正式发布）改用 Rust -based Oxide 引擎，构建速度提升约 100 倍，并支持 CSS-first 配置（`@theme` 指令）。
- 但 Tailwind v4 的配置方式与 v3 有 breaking change（`tailwind.config.ts` 被 `@theme` 替代），升级会牵涉到所有 token 定义和构建脚本。

**建议**：
- **短期**：保持 Tailwind 3，按 Style.md 扩展 `tailwind.config.ts` 的 colors/spacing/borderRadius/boxShadow 映射。
- **中期**：待 UI 改造完成后，评估 Tailwind 3 → 4 的迁移成本。如果项目构建速度不是瓶颈，可暂缓。

### 2.4 组件库：是否需要引入 shadcn/ui？

**结论：当前不建议引入完整 shadcn/ui，但可借鉴其模式。**

- shadcn/ui 是 2026 年 React 领域最流行的"copy-paste"组件库模式，基于 Radix UI / Base UI 无头原语 + Tailwind CSS，强调代码所有权和无运行时开销。
- RGoat 已有自定义组件体系，且 Style.md 定义了自有设计 token。全面引入 shadcn/ui 需要重构大量组件，与"最小改动"原则冲突。
- 但对于未来新增的复杂交互组件（如下拉菜单、右键菜单、对话框、标签页），可参考 shadcn/ui 的实现或直接复制其基于 Radix 的原语，以确保可访问性。

**建议**：不整体引入 shadcn/ui，但在实现 ContextMenu、Tabs、CommandPalette 升级时，可局部参考或引入 Radix UI 原语。

### 2.5 状态管理：Zustand 4.5.2

**结论：当前选择完全合理，建议保留。**

- Zustand 在 2026 年仍是 React 状态管理的主流选择之一，以轻量、无 Provider、选择性订阅著称，非常适合 RGoat 当前规模（6 个 store）。
- Redux Toolkit 适合大型团队和复杂状态机，但对 RGoat 来说过重；Jotai 适合原子化/派生状态复杂的场景，但当前 store 边界清晰，无需切换。
- 当前 `sessionStore`、`chatStore`、`configStore`、`changesStore`、`themeStore`、`toastStore`、`skillStore` 分工明确，Zustand 足以支撑。

**建议**：保留 Zustand。后续若出现跨 store 派生状态增多，可考虑 Zustand 的 `subscribeWithSelector` 或少量 Jotai 原子辅助，但当前无需调整。

### 2.6 图标库：lucide-react

**结论：符合规范，保留。**

- Style.md 已要求统一使用 `lucide-react`，当前代码基本符合，只需统一尺寸（14/16/18/24px）和 `currentColor` 继承。

### 2.7 技术栈总建议

| 层级 | 当前 | 建议 |
|---|---|---|
| 桌面框架 | Tauri v2 | **保留** |
| 前端框架 | React 18.3.1 | **保留，中期升级 React 19** |
| 构建工具 | Vite 5.3.1 | **保留，可择机升级 Vite 6** |
| 样式方案 | Tailwind CSS 3.4.4 | **保留，中期评估 Tailwind 4** |
| 状态管理 | Zustand 4.5.2 | **保留** |
| 组件库 | 自定义 | **不引入 shadcn/ui 整体，局部参考 Radix 原语** |
| 图标库 | lucide-react | **保留** |

---

## 三、现有功能到 RGoat-Style.md 的映射与差距

### 3.1 色彩体系

- **现状**：`index.css` 仅定义 `--bg`、`--surface`、`--surface-light`、`--primary`、`--text`、`--text-muted`、`--success`、`--warning`、`--error`、`--border`，共 10 个变量。`--primary` 为 #e94560（高饱和粉红）。
- **规范要求**：完整的暗色 token 体系，包括 `--bg-elevated`、`--surface-hover`、`--surface-active`、`--text-secondary`、`--text-tertiary`、`--text-brand`、`--text-on-brand`、`--border-strong`、`--divider`、`--primary-hover`、`--primary-active`、`--primary-subtle`、`--space-*`、`--ring`、`--shadow-*`、`--accent-signal` 等。
- **影响文件**：`frontend/src/index.css`、`frontend/tailwind.config.ts`、所有组件文件中的颜色类名。

### 3.2 布局结构

- **现状**：两栏布局（Sidebar + ChatArea），顶部独立 Header 高 48px，Sidebar 内包含 Logo、折叠按钮、四栏 Tabs。
- **规范要求**：三栏基准布局（左侧边栏 260px、中央画布、右侧面板 320px），标题栏与窗口标题栏整合，包含标签页/搜索/全局设置入口。
- **功能映射**：
  - Sidebar 的 `Sessions` 保留在左侧边栏。
  - Sidebar 的 `Files` / `Changes` 应迁移到**右侧面板**，作为工具面板。
  - Sidebar 的 `Tasks` 当前为占位，可继续在右侧面板中以标签形式保留或隐藏。
  - 顶部 Header 应改造为 `TitleBar`，整合当前 Header 的设置入口、Sidebar 折叠、Logo、会话标签页。
  - 右侧面板需新增 `RightPanel` 组件，承载 Files / Changes / Diff / Preview / Terminal 标签。

### 3.3 Composer（原 InputPanel）

- **现状**：底部全宽 footer，高 56px+，Provider 与 Mode 选择器为实色填充块。
- **规范要求**：底部居中悬浮卡片（max-width 840px），模式选择为 subtle pill，整体使用 `--shadow-sm`。
- **功能映射**：
  - 保留现有 Provider 选择、Mode 切换、Slash 命令、发送/取消逻辑。
  - 仅调整视觉容器：从全宽 footer 改为居中悬浮卡片。
  - Mode 选择器从实色填充 tab 改为 pill 风格。

### 3.4 状态栏

- **现状**：使用 `|` 竖线分隔，信息全量平铺，包含 Ready/Streaming、Provider/Model、Mode、session count、Approvals、Changes、Tools、Tokens、WiFi 图标。
- **规范要求**：高度 28px，用间距/dot 分隔，仅异常/非零时高亮，左侧状态指示灯+项目路径，中部模型+模式，右侧 token/变更/审批/工具。
- **功能映射**：保留所有统计字段，仅调整视觉呈现。`session count` 可移除（标题栏标签页已暗示会话数量）或折叠到工具提示中。

### 3.5 消息气泡

- **现状**：用户消息 `bg-primary text-white`（粉红背景）；Agent 消息 `bg-surface border`；代码块硬编码 `bg-[#0d1117]`；内联代码 `text-warning`；流式光标 `bg-primary`。
- **规范要求**：用户消息 `--surface-active` 右对齐；Agent 消息无背景，靠左侧头像/品牌标识区分；代码块 `--bg-elevated`；流式光标 `--accent-signal`。
- **功能映射**：保留消息类型分支，仅替换颜色/圆角/间距类名。需在 `MessageBubble` 中新增 Agent 头像（用 `--primary-subtle` 背景的 "R" 标识）。

### 3.6 工具调用卡片

- **现状**：已较规范，但风险徽章使用 `bg-error/15` 等硬编码透明色，`hover:bg-white/5` 硬编码。
- **规范要求**：使用 `--*-subtle` 语义 token，左侧 2px 状态色指示条，hover 使用 `--surface-hover`。
- **功能映射**：替换颜色类名，新增左侧状态指示条。

### 3.7 命令面板

- **现状**：视觉基本合规，但 active 项使用 `bg-primary/10 text-primary`，未使用 `--primary-subtle` / `--text-brand`。
- **规范要求**：背景 `--surface`，阴影 `--shadow-lg`，圆角 12px，active 项 `--primary-subtle` + `--text-brand`。
- **功能映射**：仅调整颜色和阴影 token。

### 3.8 设置对话框

- **现状**：`SettingsDialog.tsx` 视觉已较接近规范（模态框、圆角、边框），但标题栏使用 `bg-primary/5`，选中项使用 `bg-primary text-white`。
- **规范要求**：对话框标题栏不使用主色背景；选中项使用 `--primary-subtle` + `--text-brand` 或下划线。
- **功能映射**：调整标题栏背景为 `--surface` 或 `--bg-elevated`，调整 Mode/主题选择器的 active 样式为 subtle。

### 3.9 焦点环与可访问性

- **现状**：多处 `outline-none`，未统一 `:focus-visible`。
- **规范要求**：全局 `:focus-visible` 使用 2px `--ring` + 2px offset；所有可交互元素支持键盘焦点。
- **功能映射**：在 `index.css` 中全局定义，然后检查并移除误用的 `outline-none`。

### 3.10 主题切换

- **现状**：`themeStore.ts` 已支持 dark/light/system，通过切换 `html` 的 `.dark`/`.light` class 生效。
- **规范要求**：所有颜色通过 CSS 变量，亮暗主题均有完整 token。
- **功能映射**：扩展 `index.css` 的 `.light` 变量即可，主题切换逻辑无需改动。

---

## 四、分阶段实施方案（基于现有功能）

### 阶段划分原则

- **阶段 1：Foundation（基础 token）**：先完成 CSS 变量、Tailwind 配置、字体、焦点环。所有后续阶段依赖此阶段。
- **阶段 2：Layout（布局骨架）**：改造 `AppLayout`，引入 `TitleBar`、`RightPanel`，将 Files/Changes 从 Sidebar 迁出，形成三栏结构。
- **阶段 3：Chrome（外壳组件）**：Sidebar、StatusBar、Composer（原 InputPanel）适配新规范。
- **阶段 4：Content（内容组件）**：消息气泡、工具卡片、流式指示、空状态、代码块。
- **阶段 5：Interaction（交互与可访问性）**：命令面板、设置对话框、快捷键补齐、焦点环清理、减少动效。
- **阶段 6：Integration & Polish（集成与打磨）**：端到端验证、亮色主题验证、性能检查、变更总结。

---

## 五、阶段 1：Foundation（基础 Token）

### 5.1 任务清单

1. **重写 `frontend/src/index.css`**
   - 替换为 RGoat-Style.md 第 2.1 节完整 token（含 `--space-*`、`--ring`、`--shadow-*`）。
   - 新增 `.light` 主题完整映射。
   - 新增全局 `:focus-visible` 样式。
   - 引入 Inter 与 JetBrains Mono 字体栈（优先使用系统字体 + CDN fallback，避免离线场景依赖外部资源）。
   - 更新自定义滚动条颜色引用为新 token。
2. **扩展 `frontend/tailwind.config.ts`**
   - 将 `--space-*` 映射到 Tailwind spacing scale。
   - 将新颜色 token 映射到 Tailwind colors。
   - 定义 `borderRadius`（sm 4px / DEFAULT 8px / md 8px / lg 12px / xl 16px）。
   - 定义 `boxShadow`（sm/md/lg 引用 `--shadow-*`）。
3. **全局搜索替换旧 token**
   - `text-textMuted` → `text-text-secondary` 或 `text-text-tertiary`（按语义）。
   - `bg-surfaceLight` → `bg-surface-hover`。
   - `text-primary` 在暗色背景文字场景 → `text-brand`。
   - `bg-primary/10` → `bg-primary-subtle`。
   - `bg-warning/10` → `bg-warning-subtle`。

### 5.2 验收标准

- [ ] `index.css` 包含 RGoat-Style.md 第 2.1 节全部 token。
- [ ] Tailwind 配置已导出全部颜色、spacing、圆角、阴影 token。
- [ ] 全局 `:focus-visible` 生效，按钮/输入框鼠标点击无焦点环。
- [ ] 旧颜色类名（`textMuted`、`surfaceLight` 等）已全局替换，构建无错误。

---

## 六、阶段 2：Layout（布局骨架）

### 6.1 目标结构

```
┌─────────────────────────────────────────────────────────────┐
│  TitleBar（窗口标题栏 + 标签页 + 全局操作）                    │ h-10
├──────────┬──────────────────────────────┬───────────────────┤
│          │                              │                   │
│ Sidebar  │      Chat Canvas             │  Right Panel      │
│ 260px    │      （消息列表 + composer）  │  320px（可折叠）   │
│          │                              │                   │
├──────────┴──────────────────────────────┴───────────────────┤
│  StatusBar（精简状态）                                         │ h-7
└─────────────────────────────────────────────────────────────┘
```

### 6.2 任务清单

1. **改造 `AppLayout.tsx`**
   - 移除现有独立 `header`。
   - 新增 `TitleBar` 组件，将 Logo、设置入口、Sidebar 折叠、右侧面板切换、搜索入口整合到标题栏。
   - 新增 `RightPanel` 区域，默认展示 Files/Changes/Diff/Preview/Terminal 标签。
   - 将 `Sidebar` 内的 `Files` / `Changes` / `Tasks` Tabs 移除或简化为仅 `Sessions`。
   - 状态栏保持底部。
2. **新建 `frontend/src/components/layout/TitleBar.tsx`**
   - 左侧：Sidebar 折叠按钮 + RGoat 品牌。
   - 中部：会话标签页列表（复用 `activeSessionId`，当前会话作为唯一标签；为后续多标签预留数据结构）。
   - 右侧：搜索按钮、右侧面板切换按钮、设置按钮。
   - 标签页样式：active 底部 2px `--primary` 下划线 + `--text`；inactive `--text-secondary`，hover `--surface-hover`。
   - 所有图标按钮提供 `aria-label`。
3. **新建 `frontend/src/components/layout/RightPanel.tsx`**
   - 固定宽度 320px，可折叠/隐藏。
   - 顶部标签栏：Files / Changes / Diff / Preview / Terminal。
   - 复用现有 `FileTree` 和 `ChangesPanel`。
   - Diff/Preview/Terminal 当前为占位，未来接入真实功能。
4. **改造 `Sidebar.tsx`**
   - 移除顶部 Logo 和折叠按钮（移至 TitleBar）。
   - 移除 Tab bar 中的 Files/Changes/Tasks，仅保留 Sessions。
   - "New Session" 按钮改为 Secondary 风格（`bg-surface-hover hover:bg-surface-active`）。
   - 当前会话项改为 `bg-primary-subtle` + 左侧 2px `--primary` 指示条 + `--text-brand` 文字。
   - 列表项 hover 改为 `bg-surface-hover`。
   - 搜索框 focus 使用 `--primary` 边框 + `--ring`。

### 6.3 关键代码示例

```tsx
// frontend/src/components/layout/AppLayout.tsx（改造后核心结构）
export default function AppLayout() {
  const [rightPanelVisible, setRightPanelVisible] = useState(true);

  return (
    <div className="flex flex-col h-screen bg-bg text-text">
      <TitleBar
        onToggleSidebar={toggleSidebar}
        onToggleRightPanel={() => setRightPanelVisible((v) => !v)}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      <div className="flex flex-1 overflow-hidden">
        <Sidebar collapsed={sidebarCollapsed} onToggleCollapse={toggleSidebar} />

        <div className="flex flex-col flex-1 overflow-hidden">
          {currentMode === "Plan" && (
            <div className="px-4 py-1.5 bg-warning-subtle border-b border-warning/30 text-warning text-xs text-center">
              Plan Mode：写工具被禁用，agent 仅可读取/搜索/分析。
            </div>
          )}
          <ChatArea onModeChange={handleModeChange} />
        </div>

        {rightPanelVisible && <RightPanel />}
      </div>

      <StatusBar ... />
      {/* Dialogs */}
    </div>
  );
}
```

```tsx
// frontend/src/components/layout/RightPanel.tsx
import { useState } from "react";
import { FileTree as FileTreeIcon, GitCompare, MonitorPlay, Terminal, X } from "lucide-react";
import FileTree from "../sidebar/FileTree";
import ChangesPanel from "../sidebar/ChangesPanel";

const TABS = [
  { id: "files", label: "Files", icon: FileTreeIcon },
  { id: "changes", label: "Changes", icon: GitCompare },
  { id: "diff", label: "Diff", icon: GitCompare }, // 未来接入 DiffViewer
  { id: "preview", label: "Preview", icon: MonitorPlay },
  { id: "terminal", label: "Terminal", icon: Terminal },
];

export default function RightPanel() {
  const [activeTab, setActiveTab] = useState("files");

  return (
    <aside className="w-80 flex flex-col border-l border-border bg-surface shrink-0">
      <div className="flex items-center justify-between px-3 h-9 border-b border-border">
        <div className="flex items-center gap-1">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              title={tab.label}
              className={`p-1.5 rounded transition-colors ${
                activeTab === tab.id
                  ? "bg-surface-active text-text"
                  : "text-text-secondary hover:bg-surface-hover"
              }`}
              aria-label={tab.label}
            >
              <tab.icon size={14} />
            </button>
          ))}
        </div>
      </div>
      <div className="flex-1 overflow-y-auto">
        {activeTab === "files" && <FileTree max_depth={3} />}
        {activeTab === "changes" && <ChangesPanel />}
        {activeTab === "diff" && <div className="p-4 text-xs text-text-secondary text-center">Diff 未启用</div>}
        {activeTab === "preview" && <div className="p-4 text-xs text-text-secondary text-center">Preview 未启用</div>}
        {activeTab === "terminal" && <div className="p-4 text-xs text-text-secondary text-center">Terminal 未启用</div>}
      </div>
    </aside>
  );
}
```

### 6.4 验收标准

- [ ] 界面呈现三栏布局：左侧边栏、中央画布、右侧面板。
- [ ] 标题栏包含品牌、Sidebar 折叠、右侧面板切换、搜索、设置入口。
- [ ] Sidebar 仅保留 Sessions，无重复 Logo/折叠按钮。
- [ ] 右侧面板可折叠/展开，Files/Changes 正常显示。
- [ ] 无双层 header 或重复 Logo。

---

## 七、阶段 3：Chrome（外壳组件）

### 7.1 StatusBar 改造

- **保持数据字段**：状态指示灯、Provider/Model、Mode、Approvals、Changes、Tools、Tokens、WiFi 图标。
- **移除** `session count`（标题栏已暗示）和 `Mode:` 前缀。
- **移除** 所有 `|` 竖线，改用 `w-1 h-1 rounded-full bg-border-strong` dot 或 `gap-3` 间距。
- **高亮规则**：仅当值非零/异常时高亮（token、changes、approvals、streaming）。
- **背景**：`--bg-elevated`；顶部边框 `--border`。
- **字号**：`text-2xs`（11px）。

```tsx
<footer className="flex items-center gap-3 px-4 h-7 bg-bg-elevated border-t border-border text-2xs text-text-secondary shrink-0">
  <span className={`w-2 h-2 rounded-full ${isStreaming ? "bg-warning animate-pulse" : "bg-success"}`} />
  <span>{isStreaming ? "Streaming" : "Ready"}</span>

  <span className="w-1 h-1 rounded-full bg-border-strong" />
  <span className="font-mono truncate max-w-[200px]">{currentProject || "当前工作区"}</span>

  <span className="w-1 h-1 rounded-full bg-border-strong" />
  <span className="font-mono">{currentModel}</span>

  <span className="w-1 h-1 rounded-full bg-border-strong" />
  <span>{currentMode}</span>

  <span className="flex-1" />

  <span className={`flex items-center gap-1 ${totalTokens > 0 ? "text-text" : ""}`}>
    <Coins size={11} />
    {formatTokens(totalTokens)}
  </span>

  {totalChanges > 0 && (
    <span className="flex items-center gap-1 text-text">
      <FileEdit size={11} />
      {totalChanges}
    </span>
  )}

  {pendingApprovals > 0 && (
    <span className="flex items-center gap-1 text-warning">
      <ShieldAlert size={11} />
      {pendingApprovals}
    </span>
  )}

  {isStreaming ? <Wifi size={12} className="text-warning" /> : <WifiOff size={12} className="text-text-tertiary" />}
</footer>
```

### 7.2 Composer 改造（原 InputPanel）

- **保留功能**：Provider 选择、Mode 切换、Slash 命令、发送/取消、自适应高度。
- **视觉改造**：
  - 外层容器从全宽 footer 改为居中悬浮卡片：`fixed bottom-4 left-1/2 -translate-x-1/2 w-full max-w-[840px] px-4`。
  - 内部背景 `--surface`，圆角 16px（`rounded-xl`），边框 `--border`，阴影 `--shadow-sm`。
  - Provider 选择器改为 subtle dropdown 按钮（ghost + border）。
  - Mode 选择器改为 pill 风格，active 用 `--surface-active` + `--text`，inactive `--text-secondary`。
  - 发送按钮保持 Primary；取消按钮保持 Danger。
  - Slash 命令下拉改为 `--shadow-md`、圆角 12px、active 项 `--primary-subtle` + `--text-brand`。
- **参考 Trae Work 输入框的交互细节（差异化适配）**：
  - 整体采用悬浮卡片布局，底部居中，与主内容区保持视觉层级分离。
  - 左侧放置**附件按钮**（回形针图标），支持添加文件到当前对话；不显示录音/麦克风按钮。
  - 右侧工具区顺序：**模型选择器**（显示当前 Provider/Model，如 `Kimi-K2.7-Code`）→ **快捷模式切换按钮**（替代闪电图标，点击后在常用模式间快速切换，例如 Agent / YOLO，hover/tooltip 提示当前模式）→ **设置/展开按钮**（可选，展开更多输入选项）→ **发送/取消按钮**。
  - 快捷模式切换按钮使用 ghost 样式，active 状态使用 `--surface-active` + `--text-brand`，不采用 Trae 的闪电图标，以避免与“加速/强力模式”语义混淆。
  - placeholder 保持现有提示文案，居左显示，字号 `text-sm`，颜色 `--text-tertiary`。
  - 输入框获得焦点时，卡片边框变为 `--primary`，并显示 `--ring` 焦点环。

```tsx
// 关键结构
<div className="fixed bottom-4 left-1/2 -translate-x-1/2 w-full max-w-[840px] px-4 z-20">
  <div className="flex gap-2 p-2 rounded-xl bg-surface border border-border shadow-sm">
    <ProviderSelector />
    <ModeSelector />
    <div className="flex-1 relative">
      <textarea className="w-full px-3 py-2 rounded-lg bg-bg border border-border text-sm resize-none outline-none focus:border-primary focus-visible:ring-2 focus-visible:ring-ring" />
      <SlashMenu />
    </div>
    <SendButton />
  </div>
</div>
```

### 7.3 Sidebar 改造补充

- 会话列表按时间分组（今天 / 昨天 / 最近 7 天 / 更早）。当前代码直接按搜索过滤展示，需新增分组逻辑。
- 右键菜单：会话项右键支持重命名、删除（当前已有点击按钮，可先保留；后续阶段 5 再升级为右键菜单）。

### 7.4 验收标准

- [ ] StatusBar 无 `|` 分隔，高度 28px，仅非零/异常统计高亮。
- [ ] Composer 为底部居中悬浮卡片，模式选择器为 subtle pill。
- [ ] Sidebar 当前会话为 subtle 背景 + 左侧指示条。

---

## 八、阶段 4：Content（内容组件）

### 8.1 MessageBubble 改造

- **用户消息**：右对齐，`bg-surface-active text-text rounded-xl rounded-tr-sm`，不使用 primary 背景。
- **Agent 消息**：左对齐，无背景，左侧添加品牌头像（`w-7 h-7 rounded-full bg-primary-subtle` + `--text-brand` "R"）。
- **代码块**：`bg-bg-elevated border border-border rounded-lg`，移除硬编码 `#0d1117`。
- **内联代码**：`bg-surface-hover text-text-secondary`（替换 `text-warning`）。
- **错误消息**：`bg-error-subtle border-error/40 text-error`。
- **系统消息**：`text-text-tertiary`。
- **多轮 Agent 消息之间**：以 `--divider` 顶部分隔线区分（在 `MessageList` 中实现）。

### 8.2 StreamingThought 改造

- 流式光标颜色改为 `--accent-signal`。
- 添加 "Agent 正在思考…" 的 subtle 标签（可选）。

```tsx
<div className="flex justify-start mb-4 gap-3">
  <div className="w-7 h-7 rounded-full bg-accent-signal-subtle flex items-center justify-center shrink-0">
    <span className="text-xs font-semibold text-accent-signal">R</span>
  </div>
  <div className="flex-1 max-w-[85%] text-sm text-text-secondary italic">
    {content}
    <span className="inline-block w-1.5 h-4 bg-accent-signal ml-0.5 align-text-bottom animate-pulse" />
  </div>
</div>
```

### 8.3 ToolCallCard 改造

- `hover:bg-white/5` → `hover:bg-surface-hover`。
- 状态色引用改为 `--warning-subtle` / `--success-subtle` / `--error-subtle` + 对应实色文字。
- 风险徽章使用 semantic subtle 背景 + 实色文字。
- 左侧增加 2px 状态色指示条。

### 8.4 MessageList 改造

- 空状态示例卡片 hover/focus 加 `--ring`。
- 上下文压缩通知条：`bg-primary-subtle` + `text-brand`。
- Agent 多轮消息之间增加 divider 分隔。

### 8.5 验收标准

- [ ] 用户消息背景为 `--surface-active`，非 primary。
- [ ] Agent 消息无背景，左侧有品牌头像/标识。
- [ ] 代码块背景使用 `--bg-elevated`，非硬编码。
- [ ] 流式指示使用 `--accent-signal`。
- [ ] ToolCallCard 左侧有状态色指示条，hover 使用 token。

---

## 九、阶段 5：Interaction（交互与可访问性）

### 9.1 全局快捷键补齐

- 当前已实现：`Cmd/Ctrl+K`（命令面板）、`Cmd/Ctrl+N`（新建会话）、`Cmd/Ctrl+B`（切换 Sidebar）、`Cmd/Ctrl+/`（聚焦输入）、`Esc`（关闭对话框）。
- 新增：
  - `Cmd/Ctrl + Shift + N`：新建标签页/线程（当前可先映射到"新建会话"，待多标签实现后再区分）。
  - `Cmd/Ctrl + J`：切换右侧面板。
- 在 `useGlobalShortcuts.ts` 中补充。

### 9.2 命令面板适配

- 背景 `--surface`、圆角 12px、`--shadow-lg`。
- 列表项 active：`bg-primary-subtle text-brand`。
- Footer 快捷键提示字号统一为 `text-2xs`。

### 9.3 设置对话框适配

- 标题栏背景改为 `--surface` 或 `--bg-elevated`，移除 `bg-primary/5`。
- Provider/Mode/主题选择器的 active 状态改为 `--primary-subtle` + `--text-brand` 或下划线，不使用 `bg-primary text-white`。
- "完成" 按钮保持 Primary。

### 9.4 焦点环清理

- 全局定义后，搜索 `outline-none` 并逐项审查：
  - 输入框/文本域的 `outline-none` 可保留，依赖全局 `:focus-visible` 显示环。
  - 按钮/链接的 `outline-none` 若未配合 `focus-visible` 样式，需移除或替换。

### 9.5 减少动效

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

### 9.6 验收标准

- [ ] 新增快捷键在 `useGlobalShortcuts.ts` 中实现。
- [ ] 命令面板、设置对话框符合新视觉规范。
- [ ] 所有可交互元素键盘 Tab 导航有可见焦点环。
- [ ] `prefers-reduced-motion` 下无动画。

---

## 十、阶段 6：Integration & Polish（集成与打磨）

### 10.1 端到端验证

1. **亮色主题验证**：切换 `.light`，检查所有组件无 hardcode 暗色。
2. **响应式验证**：右侧面板折叠、侧边栏折叠、窗口缩小时布局不崩。
3. **快捷键验证**：按清单逐条测试。
4. **可访问性基础验证**：
   - 所有图片/图标按钮有 `aria-label`。
   - 颜色对比度达标（可用浏览器 DevTools 对比度检查）。

### 10.2 性能检查

- 滚动性能：消息列表大时长列表可考虑虚拟滚动（当前消息量不大时可暂缓）。
- 动画性能：仅使用 `transform` / `opacity`。
- 字体加载：Inter/JetBrains Mono 使用 `font-display: swap`。

### 10.3 代码质量

- 删除未使用的旧变量/组件。
- 统一 import 顺序。
- 运行 `npm run build` 与 `cargo check` 通过。

### 10.4 验收标准

- [ ] `npm run build` 无错误。
- [ ] 亮色主题下所有组件正常。
- [ ] 快捷键清单全部通过。
- [ ] 无 `outline-none` 误用遗留。
- [ ] 生成变更总结文档到 `openspec/change/`。

---

## 十一、实施优先级与工期建议

| 阶段 | 优先级 | 预估范围 | 关键产出 |
|---|---|---|---|
| 阶段 1 Foundation | P0 | 1~2 天 | `index.css`、`tailwind.config.ts`、旧 token 替换 |
| 阶段 2 Layout | P0 | 2~3 天 | `AppLayout`、`TitleBar`、`RightPanel`、Sidebar 精简 |
| 阶段 3 Chrome | P0 | 2~3 天 | `StatusBar`、`Composer`（原 `InputPanel`） |
| 阶段 4 Content | P1 | 2~3 天 | `MessageBubble`、`ToolCallCard`、`StreamingThought` |
| 阶段 5 Interaction | P1 | 1~2 天 | 快捷键、命令面板、设置对话框、焦点环 |
| 阶段 6 Polish | P1 | 1~2 天 | 验证、修复、文档 |

> 注：以上为相对估算，实际需根据团队节奏调整。建议每完成一个阶段即做一次内部评审。

---

## 十二、风险与注意事项

1. **颜色硬编码清理**：Tailwind 中可能有大量 `bg-primary/10`、`text-textMuted` 等旧 token 引用，需全局搜索替换。
2. **Tailwind 类名冲突**：`space-*` 与 Tailwind 默认 spacing 命名冲突，配置中通过 extend 覆盖，需验证无异常。
3. **右侧面板内容**：Terminal / Preview / Diff 当前为占位，未来接入真实功能时再做二次设计。
4. **Tauri 窗口标题栏**：Windows 平台默认有原生标题栏，当前 `TitleBar` 为应用内标题栏。如需完全自定义（类似无框窗口），需调整 `tauri.conf.json` 的 `decorations: false` 并自行绘制窗口控制按钮，这是一个较大变更，**本优化周期不建议做**。
5. **字体加载**：若使用 CDN，需考虑离线场景；若打包本地字体，需配置 Vite 静态资源。建议优先使用系统字体栈，仅在必要时加载 CDN 字体。
6. **React/Tailwind 大版本升级**：本计划不包含 React 19 / Tailwind 4 升级，仅作为中期建议。避免与 UI 改造同时进行，降低风险。

---

## 十三、变更总结模板（阶段完成后填写）

阶段完成后，按项目惯例在 `openspec/change/` 下创建文件：

```
openspec/change/YYYY-MM-DD-desktop-ui-phase-N.md
```

内容包含：
- 修改了什么
- 为何这样修改
- 变更的意义
- 影响的文件清单
- 验收结果
