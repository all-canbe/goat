---
name: vue-analyzer
description: Vue/UniApp 前端项目分析器插件。自包含的分析方案，由编排器加载执行。
version: 1.0.0
---

# Vue / 前端分析器

<!-- @类型: 分析器插件 -->
<!-- @目的: 对 Vue/React/Angular/UniApp 前端项目进行完整分析 -->

## 适用识别条件

当项目包含以下任一条件时由编排器加载：

- `package.json` 中存在 `vue`、`react`、`angular`、`svelte`、`@dcloudio/uni-app` 依赖
- 存在 `vite.config.*`、`webpack.config.*`、`vue.config.*`、`next.config.*`
- `pages.json`（UniApp 小程序）
- 存在 `src/pages/` 或 `src/views/` 目录

## 分析维度

### 维度 1：框架与核心生态

**执行动作：**
- 读取 `package.json`，提取框架名称和版本
- 识别 UI 组件库（Element/Ant Design/Vant/uView/TDesign）
- 识别 TypeScript 配置（`tsconfig.json` 是否存在）
- 识别 CSS 预处理器（SCSS/Less/Tailwind）

**输出格式：**
```
框架: {名称} v{版本}
UI库: {名称} v{版本}
语言: TypeScript / JavaScript
样式: {方案}
```

### 维度 2：路由与页面

**执行动作：**
- UniApp：读取 `pages.json`，提取页面清单、TabBar、分包配置
- Vue Router：读取 `src/router/` 目录，提取路由定义
- React Router：搜索路由配置文件或组件
- 列出所有页面及其对应路由路径

**输出格式：**
```
路由类型: {pages.json / vue-router / react-router}
页面数量: {N}
TabBar页面: {列表}
分包: {分包配置}
```

### 维度 3：状态管理

**执行动作：**
- 检测 Store 方案：Vuex / Pinia / Redux / Zustand
- 读取 Store 目录结构
- 列出模块划分

**输出格式：**
```
方案: {Vuex/Pinia/Redux/...}
模块: {模块列表}
关键文件: {路径}
```

### 维度 4：组件组织

**执行动作：**
- 扫描 `src/components/` 目录结构
- 识别全局组件注册方式
- 分析组件命名约定

### 维度 5：API 与数据层

**执行动作：**
- 定位 API 请求封装（`src/utils/request.ts` 等）
- 定位 API 模块（`src/api/` 目录）
- 描述请求拦截器、错误处理、Token 管理

### 维度 6：构建与配置

**执行动作：**
- 识别构建工具（Vite/Webpack）
- 读取构建配置文件
- 识别环境变量文件（`.env*`）
- 识别代理配置

### 维度 7：开发约定

**执行动作：**
- 读取 `.trae/rules/` 目录下所有规则文件
- 读取 `.eslintrc*` 和 `.prettierrc*` 配置
- 检查 Git Hooks（`.husky/`、`lint-staged`）

## 模块提取策略

1. 扫描 `src/` 下的顶层目录，每个目录视为一个模块候选
2. 按路由/页面配置补充模块信息
3. 按 `import` 语句分析模块间依赖
4. Store 模块作为独立模块记录

## 输出格式

返回给编排器的结构化数据：

```
## 分析器输出

### 项目基本信息
{框架/语言/样式/构建工具}

### 目录结构
{目录 → 职责映射表}

### 模块地图
| 模块名 | 职责 | 关键文件 | 依赖关系 |
|--------|------|---------|---------|

### 核心依赖
| 依赖 | 版本 | 用途 |
|------|------|------|

### 数据流
{API → Store → 组件的描述}

### 开发约定
{规则文件摘要}

### 入口索引
{入口文件路径和说明}
```

---

## 版本历史

- **v1.0.0** (2026-05-29) - 初始版本
