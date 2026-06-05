---

name: kz-skill-creator
description: 创建、重构或评测 Skill 的指南。当用户想要创建新 Skill，或需要梳理、评价已有 Skill 及相关可复用指令资产的工作流边界、参考文档、验证机制与结构质量时，应使用此 Skill。
version: 1.40.6
---

# Skill 创建器

<!-- @类型: 标准操作流程(SOP) -->

<!-- @目的: 指导创建、重构高质量、可验证、可发布的 AI Skill，并评测相关可复用指令资产 -->

<!-- @场景: 用户需要创建新 Skill、重构现有 Skill，或评测已有 Skill / 可复用指令资产的结构质量 -->

<!-- @触发条件: 当用户想要创建新 Skill，或需要重构 / 评测已有 Skill 或相关可复用指令资产以提升结构质量与可维护性时 -->

> **一句话**: 我是 Skill 的构建指南，教你创建、重构并评测结构化、可验证的 AI 技能
> **版本**: v1.40.6
> **用途**: 指导创建、重构与评测高质量的 AI Skill 与相关可复用指令资产
> **适用范围**: 所有 Skill 开发、重构，以及相关结构化指令资产的结构评测场景

## 触发示例

用户说以下话时会触发此 Skill：

- "帮我创建一个处理 PDF 的 Skill"
- "我要做一个自动生成周报的工具"
- "把这个 Python 脚本封装成技能"
- "如何创建一个 Skill 来检查代码风格"
- "帮我重构这个 Skill 的 workflow 边界"
- "整理这个 Skill 的 references 和验证机制"
- "用这个口径评价下这个 Skill"
- "帮我审一下这个 Skill，并保存成文档"
- "看看这个 Skill 的结构有没有明显问题"
- "帮我审一下这个 workflow prompt / agent instruction"

***

## 目录

- [1. 快速路由](#1-快速路由)
- [2. Skill 概述](#2-skill-概述)
- [3. 核心原则](#3-核心原则)
- [4. Skill 结构与渐进式披露](#4-skill-结构与渐进式披露)
- [5. Skill 创建与评测流程](#5-skill-创建与评测流程)
- [6. 语义化标记系统规范](#6-语义化标记系统规范)
- [附录](#附录)

***

## @工作流: 理解并创建Skill

<!-- @类型: 主工作流 -->
<!-- @目的: 指导创建、重构高质量的 AI Skill，并评测相关结构化指令资产 -->
<!-- @场景: 用户需要创建、重构 Skill，或评测 Skill / 指令资产 -->
<!-- @前置条件: 已明确想解决的任务、流程或工具集成 -->
<!-- @后置验证: 生成可验证并可打包发布的 Skill -->
<!-- @ID: wf-skill-creator-main -->

***

## 1. 快速路由 {#1-快速路由}

<!-- @范围: 运行期识别 -->

### @步骤40: 用决策矩阵命中当前任务

<!-- @类型: 决策步骤 -->
<!-- @优先级: 必须 -->
<!-- @验证点: 已把当前请求稳定路由到创建、重构、评测或模板生成路径 -->
<!-- @验证方式: 能根据用户原话先命中矩阵，再决定后续读取哪份 workflow / reference -->
<!-- @ID: step-route-by-decision-matrix -->

| 场景 | 命中信号 | 跳转到 |
|---|---|---|
| 评测已有 Skill / 指令资产 | `评价这个 Skill`、`结构有没有问题`、`帮我审一下 Skill`、`帮我审一下 workflow prompt / agent instruction`、`输出评估文档` | `评测已有 Skill / 指令资产` + [skill-evaluation-workflow.md](references/authoring/skill-evaluation-workflow.md) |
| 重构已有 Skill | `重构 Skill`、`整理 workflow 边界`、`清理 references`、`补验证机制` | `重构现有 Skill` + [skill-refactoring-workflow.md](references/authoring/skill-refactoring-workflow.md) |
| 创建 / 更新目标 Skill | `创建 Skill`、`做一个 Skill`、`封装成技能`、`更新这个 Skill` | 主工作流 `理解并创建Skill` + `创建流程（含前置确认）` |
| 生成场景输入模板 | `生成输入模板`、`补 references/examples`、`沉淀场景示例` | `理解并创建Skill` + [scenario-input-template.md](references/templates/scenario-input-template.md) |

当用户明确要“评价 / 审查 / 输出评估文档”时，默认先进入评测路径；先判断目标是 Skill 目录还是独立指令资产，再做证据收集与校验，不要直接改目标对象。

***

### @步骤41: 强规则摘要

<!-- @类型: 规则步骤 -->
<!-- @优先级: 必须 -->
<!-- @验证点: 命中矩阵后已应用本 Skill 的固定创建/重构/评测约束 -->
<!-- @验证方式: 能说清哪些是运行期默认动作，哪些是设计期补充工作 -->
<!-- @ID: step-apply-skill-creator-hard-rules -->

- @动作: 若输入命中“评价 / 审一下 / 结构有没有问题 / 输出评估文档”，默认先读 [skill-evaluation-workflow.md](references/authoring/skill-evaluation-workflow.md)，先判断目标是 Skill 目录还是独立指令资产；对 Skill 先跑 `validate`，对独立指令资产先做证据审计与适用校验盘点
- @动作: 轻量 Skill 可以不用机械套“决策矩阵 + 强规则摘要”，但中复杂 Skill 若存在多个稳定用户意图，优先采用这套结构
- @动作: `workflow 映射表` 解决设计期边界；“决策矩阵 + 强规则摘要”解决运行期识别，不要拿其中一层替代另一层
- @动作: 主 `SKILL.md` 只保留路由、硬规则、关键动作和验证入口；长解释、长模板与实现细节继续下沉到 `references/`
- @动作: 修改完成后统一走 `python scripts/skill_cli.py validate <path_to_skill_folder>`；若目标 Skill 另有 `scripts/validate_skill.py`，再补跑工程硬校验

***

## 2. Skill 概述 {#2-skill-概述}

<!-- @范围: 概念介绍 -->

### @步骤1: 先确认目标 Skill 解决的问题

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @说明: 开始前先确认目标 Skill 要稳定解决什么问题，以及是否值得单独做成 Skill -->
<!-- @验证点: 能用一句话说明目标 Skill 的作用、触发场景与边界 -->
<!-- @验证方式: 输出一句话定义，并判断是否需要单独做成 Skill -->
<!-- @ID: step-understand-skill-definition -->

Skill 是为某类稳定任务提供“可触发、可执行、可验证”方法的工作手册。
@提示: 若需要完整的基础概念、能力分类和常见误区，读取 [skill-foundations.md](references/authoring/skill-foundations.md) 与 [writing-a-good-skill.md](references/authoring/writing-a-good-skill.md)；若需要整体业务图，再读 [skill-creator-workflow-guide.md](references/workflows/skill-creator-workflow-guide.md)。

***

## 3. 核心原则 {#3-核心原则}

<!-- @范围: 设计原则 -->

### @步骤2: 只把真正影响执行的内容留在正文

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @警告: 主 SKILL.md 不是百科，而是路由层 -->
<!-- @验证点: 主正文仅保留触发边界、workflow、关键动作和校验入口 -->
<!-- @验证方式: 检查解释性内容是否已下沉到 references -->
<!-- @ID: step-keep-main-body-thin -->

上下文窗口是公共资源。优先把背景解释、长示例、评估报告写法和变体规则下沉，让主文档只负责识别当前任务、路由到正确 workflow，并给出验证入口。
- @动作: 若主文档已经出现连续的大段说明文字，优先压缩成 1-2 句摘要，再链接参考文档
- @动作: 若某份内容不会影响“当前任务先走哪条 workflow”，优先放到 `references/`，不要继续堆在主正文
@提示: 简洁原则、自由度分层和下沉判断口径，参见 [skill-foundations.md](references/authoring/skill-foundations.md)。

***

## 4. Skill 结构与渐进式披露 {#4-skill-结构与渐进式披露}

<!-- @范围: 文件组织与内容加载 -->

### @步骤3: 用最小结构起步

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @文件: skill-name/SKILL.md -->
<!-- @验证点: 目录结构与 frontmatter 满足最低要求 -->
<!-- @验证方式: 检查是否具备 `SKILL.md`、资源目录和版本三处信息 -->
<!-- @ID: step-plan-skill-structure -->

主文档至少要有 `name`、`description`、`version`、主 workflow、关键步骤和版本历史；其余目录按需创建，不预置无关文件。
@提示: 最小目录结构、frontmatter 口径、资源职责和“什么不该建”，统一参见 [skill-foundations.md](references/authoring/skill-foundations.md)。

### @步骤4: 把主文档维持为路由层

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @说明: 主文档负责触发边界、workflow 和关键动作，展开细节按需读取 -->
<!-- @验证点: 长解释、长模板和次级流程已被下沉 -->
<!-- @验证方式: 检查正文是否主要由路由与决策内容构成 -->
<!-- @ID: step-apply-progressive-disclosure -->

Skill 使用三级加载：`name + description` 始终可见，`SKILL.md` 在 Skill 触发后加载，`scripts/`、`references/`、`assets/` 按需读取。
- @动作: 若目标 Skill 存在 3 个及以上稳定用户意图、多个 workflow 触发词容易混淆，或旧决策树已变长难维护，优先在主 `SKILL.md` 设计“决策矩阵 + 强规则摘要”
- @动作: 决策矩阵只负责“用户说什么 -> 命中哪条 workflow”；强规则摘要只保留固定约束、禁止事项与默认入口
- @动作: 若目标 Skill 只有 1-2 个稳定意图且几乎没有混淆空间，可继续使用简短 bullets，不必机械表格化
- @动作: 若主文档采用决策矩阵，且存在 `references/examples/input-template-*.md`，同步维护 `references/examples/index.md` 的“决策矩阵命中速查”
@提示: 渐进式披露的具体模式与示例，参见 [progressive-disclosure-patterns.md](references/authoring/progressive-disclosure-patterns.md)。

***

## 5. Skill 创建与评测流程 {#5-skill-创建与评测流程}

<!-- @范围: 实施步骤 -->

## @工作流: 创建流程（含前置确认）

<!-- @类型: 子工作流 -->
<!-- @说明: 按顺序遵循这些步骤，仅在有明确理由时跳过 -->
<!-- @后置验证: Skill 结构通过验证并可被打包为 .skill -->
<!-- @ID: wf-skill-creation-flow -->

默认顺序：统一确认 -> 理解需求 -> 规划边界 -> 初始化 -> 编辑 -> 验证 / 打包 -> 迭代。

## @工作流: 重构现有 Skill

<!-- @类型: 子工作流 -->
<!-- @说明: 当目标是整理已有 Skill 的结构、边界、references 或验证机制时使用 -->
<!-- @后置验证: 重构后的 Skill 结构更清晰，且校验与打包仍通过 -->
<!-- @ID: wf-refactor-existing-skill -->

命中“重构 Skill / 整理 workflow 边界 / 清理 references / 补验证机制”时，默认先盘点问题类型，再决定哪些内容留在主 `SKILL.md`、哪些下沉到 `references/`，最后统一跑 `validate` / `package`。细节参见 [skill-refactoring-workflow.md](references/authoring/skill-refactoring-workflow.md)。

## @工作流: 评测已有 Skill / 指令资产

<!-- @类型: 子工作流 -->
<!-- @说明: 当目标是评价一个已有 Skill 或相关可复用指令资产的结构质量、可维护性与重构优先级，并输出评估报告时使用 -->
<!-- @后置验证: 已产出带证据的评估报告，明确主要优点、主要问题、验证约束与后续重构方向 -->
<!-- @ID: wf-evaluate-existing-skill -->

命中“评价 / 审查 / 输出评估文档”时，默认先盘点证据；若目标是 Skill 目录则运行 `validate`，若目标是独立指令资产则记录适用的校验入口与验证缺口，再归纳优先级并生成评估文档；Skill 目录默认落到 `skill-evaluation.md`，单文件指令资产默认落到同目录的 `<target-file-stem>.evaluation.md`。除非用户明确要求，不直接改目标对象。细节参见 [skill-evaluation-workflow.md](references/authoring/skill-evaluation-workflow.md)。

### @步骤0: 统一确认进入条件与关键问题

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @说明: 在开始前先确认是否需要进入当前流程，并一次性确认所有关键问题 -->
<!-- @验证点: 已确认是否进入流程，且关键待确认问题已集中澄清 -->
<!-- @验证方式: 检查是否输出统一确认清单，并记录确认结论 -->
<!-- @ID: step-confirm-scope-and-open-questions -->

- @动作: 一次性确认任务类型、交付物类型、是否需要 workflow 设计，以及哪些要求是硬规则
- @动作: 若用户明确不需要进入当前流程或不需要做 workflow 设计，记录结论后转入轻量执行路径
- @动作: 将确认结论写入 `design.md` 或同等设计文档
- @动作: 若当前任务是评测目标对象，额外记录报告输出路径、命名约束、目标类型与是否隐藏本技能名称

### @步骤1: 用具体示例理解目标对象，并判断复杂度

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @说明: 收集用户需求、触发语句、核心痛点，并判断目标对象复杂度 -->
<!-- @验证点: 至少收集到 2 个可复述的触发示例，明确核心痛点，并完成复杂度分级 -->
<!-- @验证方式: 列出“用户说什么会触发 Skill”与对应期望结果，并记录复杂度判断及其依据 -->
<!-- @ID: step-collect-concrete-examples -->

- @动作: 至少整理 2 条真实触发语句和 1 份痛点总结
- @动作: 把输入 / 输出 / 交互模式写入 `design.md` 或同等设计文档
- @动作: 为目标对象标注“轻量 / 中等 / 复杂”，并写明依据；需要系统化访谈时运行 `python scripts/skill_cli.py analyze --output design.md`
- @动作: 若判断为轻量 Skill，记录为什么可跳过完整 workflow 映射；若判断为复杂 Skill，记录后续必须补 workflow 映射表
- @动作: 若当前任务是评测目标对象，额外记录主文档类型、关键文件清单和预期评估文档落点

### @步骤2: 规划可重用内容与工作流边界

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @依赖: step-collect-concrete-examples -->
<!-- @说明: 基于需求分析决定哪些内容应成为脚本、参考资料或资产，并为中复杂 Skill 明确 workflow 边界 -->
<!-- @验证点: 产出调用方式设计、资源清单，以及复杂 Skill 所需的 workflow 映射表 -->
<!-- @验证方式: 检查是否明确 scripts / references / assets 的职责，并在复杂 Skill 中完成 workflow 映射表 -->
<!-- @ID: step-plan-reusable-content -->

- @动作: 若目标 Skill 为中等或复杂 Skill，先读取 [business-to-workflow-mapping.md](references/authoring/business-to-workflow-mapping.md)
- @动作: 若当前任务是重构已有 Skill，先读取 [skill-refactoring-workflow.md](references/authoring/skill-refactoring-workflow.md)
- @动作: 输出资源清单，明确 `scripts/`、`references/`、`assets/` 的职责；模板目录约束参见 [skill-foundations.md](references/authoring/skill-foundations.md) 与 [scenario-input-template.md](references/templates/scenario-input-template.md)
- @动作: 若目标 Skill 为复杂 Skill，必须先在 `design.md` 或同等设计文档中产出 workflow 映射表，再继续；字段要求见 [business-to-workflow-mapping.md](references/authoring/business-to-workflow-mapping.md)
- @动作: 若目标 Skill 为中等或复杂 Skill，且存在多个稳定用户意图或 workflow 容易混淆，除设计期 `workflow 映射表` 外，再为主 `SKILL.md` 规划运行期“决策矩阵 + 强规则摘要”
- @动作: 若目标 Skill 已维护 `references/examples/input-template-*.md`，且主文档采用“决策矩阵 + 强规则摘要”，同步维护 `references/examples/index.md`；“决策矩阵命中速查”写法见 [business-to-workflow-mapping.md](references/authoring/business-to-workflow-mapping.md)
- @动作: 若当前任务是重构已有 Skill，参考 [business-to-workflow-mapping.md](references/authoring/business-to-workflow-mapping.md) 中的重构触发器清单，判断是否需要重整 workflow
- @动作: 若需要从目标 Skill 的 `SKILL.md` 自动生成场景示例草稿，运行 `python scripts/skill_cli.py generate-templates <path_to_skill_folder>`；默认输出到 `<skill>/references/examples/`，并同时更新 `index.md`
- @动作: 若自动生成覆盖了已存在的示例文档，递增补丁版本并在版本历史首条记录“重新生成/更新”的原因，版本历史最多保留 5 条
- @动作: 对高风险技术点先做最小 PoC，再决定是否纳入 Skill
- @动作: 若当前任务是评测目标对象，输出最小证据清单；Skill 目录至少覆盖 `SKILL.md`、关键 companion files 与 `validate` 入口，单文件指令资产至少覆盖主文档、关键 companion files 与可用校验入口

### @步骤3: 初始化Skill

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @依赖: step-plan-reusable-content -->
<!-- @说明: 使用脚手架创建基础目录结构 -->
<!-- @验证点: 输出目录存在且包含 SKILL.md -->
<!-- @验证方式: 检查输出目录是否包含 SKILL.md、scripts、references、assets -->
<!-- @ID: step-init-skill -->

- @动作: 使用 `python scripts/skill_cli.py init <skill-name> --path <output-directory>` 初始化 Skill 骨架
- @动作: 生成目录后立即修正 frontmatter 与目录职责
- @动作: 清理 TODO / 占位信息，并按需补齐真实资源

### @步骤4: 编辑Skill

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @依赖: step-init-skill -->
<!-- @验证点: SKILL.md 结构清晰且关键资源可被引用 -->
<!-- @验证方式: 检查 description 覆盖触发条件、正文包含工作流与导航 -->
<!-- @ID: step-edit-skill -->

- @动作: 编辑前必须读取 [skill-markup-guide.md](references/authoring/skill-markup-guide.md) 与 [versioning-and-validation.md](references/authoring/versioning-and-validation.md)
- @动作: workflow 设计时再读 [business-to-workflow-mapping.md](references/authoring/business-to-workflow-mapping.md)；重构时再读 [skill-refactoring-workflow.md](references/authoring/skill-refactoring-workflow.md)；评测时再读 [skill-evaluation-workflow.md](references/authoring/skill-evaluation-workflow.md)；输入模板场景再读 [scenario-input-template.md](references/templates/scenario-input-template.md)
- @动作: 若目标 Skill 为复杂 Skill，确认 workflow 映射表已存在且字段完整，再开始编辑目标 Skill 的 `SKILL.md`
- @动作: 若目标 Skill 采用“决策矩阵 + 强规则摘要”，在正文中同时补齐矩阵表头与“强规则摘要”步骤；不要只写其中一半
- @动作: 先实现 `scripts/`、`references/`、`assets/`，再回写 `SKILL.md` 导航
- @动作: 按需新增 `references/`、`assets/` 内的真实资源，不预置无关占位文件
- @动作: **编辑 SKILL.md 和 references/ 文档时，必须使用语义化标记**：`## @工作流:`、`### @步骤N:`、HTML 注释元数据（`@类型`、`@优先级`、`@验证点`、`@验证方式`）、`- @动作:`；完整规范参见 [skill-markup-guide.md](references/authoring/skill-markup-guide.md)
- @动作: 修改完成后同步更新 `version`（frontmatter + 头部版本信息 + 文末首条版本历史），并确保版本历史不超过 5 条
- @动作: 若当前任务是评测目标对象，默认先读 workflow 参考；除非用户明确要求，不直接修改目标对象
- @动作: 若用户要求保存评估结果，Skill 目录默认写入 `<target-skill>/skill-evaluation.md`；单文件指令资产默认写入同目录的 `<target-file-stem>.evaluation.md`

### @步骤5: 验证与打包Skill

<!-- @类型: 操作步骤 -->
<!-- @优先级: 必须 -->
<!-- @依赖: step-edit-skill -->
<!-- @验证点: Skill 结构已验证；若需要发布，则打包产物已生成 -->
<!-- @验证方式: 先运行验证脚本；若任务涉及发布，再运行打包脚本并确认 .skill 文件生成 -->
<!-- @ID: step-package-skill -->

- @动作: 修改 Skill 后同步更新 `version`、头部版本信息和文末版本历史
- @动作: 统一从 `skill_cli.py` 进入：先跑 `python scripts/skill_cli.py validate <path_to_skill_folder>`，需要发布时再跑 `package`，需要效果评测时再跑 `eval`
- @动作: 若目标 Skill 存在 `scripts/validate_skill.py`，发布前进入目标 Skill 目录后运行 `python scripts/validate_skill.py --skill .`
- @动作: 发布前确认版本三处一致、语义化标记与 Markdown 链接通过校验、复杂 Skill 已补齐 workflow 映射表
- @动作: 若主 `SKILL.md` 采用“决策矩阵 + 强规则摘要”，确认两者同时存在；若存在 `references/examples/input-template-*.md`，确认 `references/examples/index.md` 已补“决策矩阵命中速查”
- @动作: 若当前任务是评测目标对象，把适用校验输出中的硬错误写入评估文档，并核对评估文档路径、引用链接与目标类型是否一致
@提示: 完整规则与排查顺序，参见 [versioning-and-validation.md](references/authoring/versioning-and-validation.md)。

### @步骤6: 迭代

<!-- @类型: 操作步骤 -->
<!-- @优先级: 可选 -->
<!-- @依赖: step-package-skill -->
<!-- @验证点: 改进点被转化为可执行的文档或脚本变更 -->
<!-- @验证方式: 更新 SKILL.md / 资源后重新运行关键脚本或打包验证 -->
<!-- @ID: step-iterate-skill -->

- @动作: 在真实任务中记录失败点、模糊点和低效点
- @动作: 将反馈转成 `SKILL.md`、`references/`、`scripts/` 或 `assets/` 变更
- @动作: 同步更新版本号和版本历史，并重新运行关键验证 / 打包

***

## 6. 语义化标记系统规范 {#6-语义化标记系统规范}

<!-- @范围: 文档结构化标准 -->

<!-- @类型: 强制规范 -->

<!-- @目的: 确保所有 Skill 使用统一结构化标记，提升可执行性与可验证性 -->

所有 Skill 必须严格遵循语义化标记系统规范，把自然语言说明转成更稳定、可验证、可巡检的结构。

最低写作要求：

1. 用 `## @工作流:` 定义主工作流或关键子工作流
2. 用 `### @步骤N:` 定义可执行步骤
3. 在标题下紧贴 HTML 注释元数据，至少覆盖 `@类型`、`@优先级`、`@验证点`、`@验证方式`
4. 可执行事项统一用 `- @动作:` 表达
5. 脆弱步骤补充 `@失败信号`、`@排查`、`@回滚`

@警告: 这是强制要求。完整字段定义与示例参见 [skill-markup-guide.md](references/authoring/skill-markup-guide.md)。

***

## 附录

### A. 快速参考

- 本 Skill 的功能边界、业务流程与子工作流关系：参见 [skill-creator-workflow-guide.md](references/workflows/skill-creator-workflow-guide.md)
- 基础概念、目录结构、资源职责、渐进加载：参见 [skill-foundations.md](references/authoring/skill-foundations.md)
- workflow 拆分与重构：参见 [business-to-workflow-mapping.md](references/authoring/business-to-workflow-mapping.md)
- Skill 重构步骤与迁移检查：参见 [skill-refactoring-workflow.md](references/authoring/skill-refactoring-workflow.md)
- Skill / 指令资产结构评估与评估文档写法：参见 [skill-evaluation-workflow.md](references/authoring/skill-evaluation-workflow.md)
- Skill 结构评估报告模板：参见 [skill-evaluation-template.md](references/templates/skill-evaluation-template.md)
- Skill 结构评估内部 checklist：参见 [skill-evaluation-checklist.md](references/templates/skill-evaluation-checklist.md)
- 版本与发布校验：参见 [versioning-and-validation.md](references/authoring/versioning-and-validation.md)
- 语义化标记规范：参见 [skill-markup-guide.md](references/authoring/skill-markup-guide.md)

### B. 评测与复盘工具

- Skill 结构评估流程与标准报告结构参见 [skill-evaluation-workflow.md](references/authoring/skill-evaluation-workflow.md)
- 正式 `skill-evaluation.md` 模板参见 [skill-evaluation-template.md](references/templates/skill-evaluation-template.md)
- 8 维评分前复核 checklist 参见 [skill-evaluation-checklist.md](references/templates/skill-evaluation-checklist.md)
- 评测入口、回放与聚合方式参见 [eval-loop.md](references/evaluation/eval-loop.md)

***

## 版本历史

- **v1.40.6** (2026-05-17) - 收口评测执行文案：将第 5 节残留的 `Skill-only` 评测条件统一为“目标对象”口径，并为单文件指令资产补充默认评估文档落点
- **v1.40.5** (2026-05-17) - 对齐评测边界与入口：将主文档的评测范围统一到“Skill / 可复用指令资产”，补充独立指令资产的默认校验口径，并在附录显式加入 `skill-evaluation-checklist.md` 入口
- **v1.40.4** (2026-05-17) - 补齐独立评估模板入口：在附录快速参考与评测工具区加入 `skill-evaluation-template.md` 直达链接，便于直接落正式评估文档
- **v1.40.3** (2026-05-17) - 联动整理 supporting docs：补齐“评测已有 Skill”示例模板，统一 workflow guide / evaluation workflow 与主文档的决策矩阵、README 与验证入口口径
- **v1.40.2** (2026-05-17) - 执行 P2.6 瘦身：将 workflow 映射字段要求与 examples/index 联动细则进一步下沉到 references，主文档仅保留入口规则与同步动作
