# Skills 系统

## 目录结构

```
skills/<skill-name>/
├── SKILL.md              — 入口 (YAML frontmatter + body)
├── agents/               — 子角色 prompt
│   ├── analyzer.md
│   └── grader.md
├── references/           — 参考文档
│   ├── schemas.md
│   └── workflows.md
└── scripts/              — 可执行脚本
    ├── run_eval.py
    └── utils.py
```

## 加载机制

- `discover_skill_directories(skills_root)` 扫描 `skills/*/SKILL.md`
- `load_skill_from_directory(skill_dir)` 解析单个技能目录
- SKILL.md 的 YAML frontmatter 定义: `name` / `description` / `tools` / `agents`
- `SkillRegistry.load_skills_from_directory()` 批量加载
- 无 skills/ 目录时回退为 5 个内置硬编码技能

## Skill 类字段

| 字段 | 类型 | 来源 |
|------|------|------|
| name | str | frontmatter / 构造函数 |
| description | str | frontmatter / 构造函数 |
| tools | list[BaseTool] | 构造函数注入 |
| path | Path \| None | 目录路径 |
| agents | dict[str, str] | agents/*.md |
| references | dict[str, str] | references/*.md |
| scripts | list[Path] | scripts/ 目录 |
| metadata | dict | 扩展属性 |

## 设计原则

- 每个技能是一个可独立分发的目录
- SKILL.md body 在触发时载入，元数据始终在上下文中
- agents/references/scripts 按需读取，不预加载
- 技能描述 (description) 是触发匹配的主要机制
- 不要在同一技能中混合多个不相关的领域