# Design: 全局 vs 项目 Skill 分层方案（Claude Code 模式）

> 日期：2026-06-03
> 状态：设计提案（待实现）

---

## 1. 设计目标

- 借鉴 Claude Code 的「全局 + 项目」双层配置机制
- 全局 skill：用户安装一次，所有项目共享
- 项目 skill：只在本项目生效，覆盖同名的全局 skill
- 合并而非互斥加载

## 2. 目录结构

```
~/.goat/                              ← 全局
  ├── skills/                         ← 全局 skill（用户安装到此）
  │   ├── find-skills/SKILL.md
  │   └── code-explorer/SKILL.md
  ├── rules/                          ← 全局 rules（类似 CLAUDE.md 全局版）
  │   └── preferences.md
  ├── conversations.db
  └── tasks.db

<project_root>/
  ├── .goat/                          ← 新建！项目级 .goat
  │   ├── skills/                     ← 项目 skill（仅本项目生效）
  │   │   └── kz-skill-creator/
  │   │       └── SKILL.md
  │   ├── rules/                      ← 项目 rules（类似 CLAUDE.md）
  │   │   └── README.md
  │   └── settings.json               ← 项目专属设置
  ├── skills/                         ← 保持现状（包内内置 skill）
  └── other files...
```

## 3. 关键变更

### 3.1 新增 `get_workspace_goat_dir()` — [workspace.py](file:///e:/python/tuidemo/my-tui-main/goat/core/workspace.py)

```python
def get_workspace_goat_dir(workspace: Path | None = None) -> Path:
    """返回项目级 .goat 目录（等价于 Claude Code 的 .claude/）。"""
    root = workspace or Path.cwd().resolve()
    goat_dir = root / ".goat"
    goat_dir.mkdir(parents=True, exist_ok=True)
    return goat_dir
```

### 3.2 加载策略变更为**合并模式** — [chat_handler.py](file:///e:/python/tuidemo/my-tui-main/goat/api/chat_handler.py)

```python
# 合并所有候选目录的 skill，而非首次命中停止
_dirs_to_load = []
for _d in _skill_candidates:
    if _d.exists() and _d not in _dirs_to_load:
        _dirs_to_load.append(_d)

# 插入项目 .goat/skills（高优先级）
_workspace_skills = get_workspace_goat_dir() / "skills"
_workspace_skills.mkdir(parents=True, exist_ok=True)
_dir_to_load = [_workspace_skills] + _dirs_to_load  # 项目优先

for _d in _dirs_to_load:
    _loaded = self.skill_registry.load_skills_from_directory(_d)
    if _loaded:
        logger.info(...)
```

### 3.3 `.goatignore` 机制（可选增强）

支持在项目 `.goat/` 中放一个 `.goatignore`，列出不被加载到 agent 上下文中的全局 skill 名称：

```text
# .goatignore — 排除不需要的全局 skill
unused-skill-A
unused-skill-B
```

### 3.4 前端支持

不在整体改动范围内。CommandPalette 的「列出已安装技能」会**自动**显示合并后的列表（因为 GET /api/skills 返回的是合并后的 registry）。

### 3.5 与 CLAUDE.md 的等价性

Claude Code 的 CLAUDE.md 即是项目级 rules / 指令。本项目可通过以下方式对标：

```python
def get_project_rules(workspace: Path | None = None) -> list[str]:
    """读取项目 .goat/rules/*.md + 全局 ~/.goat/rules/*.md。"""
    rules = []
    for base in [get_workspace_goat_dir(workspace), GOAT_HOME]:
        rules_dir = base / "rules"
        if rules_dir.exists():
            for f in sorted(rules_dir.glob("*.md")):
                rules.append(f.read_text(encoding="utf-8"))
    return rules
```

Agent 在每个 session 启动时读取 rules 并拼接到 system prompt 中，等价于 Claude Code 的 CLAUDE.md。

## 4. 迁移路径

1. **Phase 1（P0 当前状态）**：已实现 `~/.goat/skills` 全局目录 + 包内内置回退
2. **Phase 2（本方案）**：新建 `<project>/.goat/` + 合并加载策略
3. **Phase 3（可选）**：rules 机制（等价 CLAUDE.md）+ `.goatignore` 排除

## 5. 不破坏向后兼容

- 现有 `~/.goat/skills` 保持不动
- 现有 `goat/core/workspace.py` 的 `get_skills_dir()` 签名不变，新增函数而非修改已有
- 项目 `.goat/` 即使不存在也正常工作（`mkdir(parents=True, exist_ok=True)`）
- 包内 `skills/` 仍作为最后的回退层