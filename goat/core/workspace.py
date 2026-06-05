from __future__ import annotations

import os
import sys
from pathlib import Path


def get_goat_home() -> Path:
    path = Path.home() / ".goat"
    path.mkdir(parents=True, exist_ok=True)
    return path


def get_app_root() -> Path:
    try:
        main_mod = sys.modules.get("__main__")
        if main_mod and hasattr(main_mod, "__file__") and main_mod.__file__:
            return Path(main_mod.__file__).resolve().parent
    except Exception:
        pass
    return Path.cwd().resolve()


def get_skills_dir() -> Path:
    app_root = get_app_root()
    skills_dir = app_root / "skills"
    return skills_dir


def resolve_workspace(workspace_arg: str | None = None) -> Path:
    if workspace_arg:
        path = Path(workspace_arg).resolve()
        if path.is_dir():
            return path
        print(f"  ⚠️ 指定的工作空间不存在: {workspace_arg}，使用当前目录")
    return Path.cwd().resolve()


def get_workspace_goat_dir(workspace: Path | None = None) -> Path:
    """返回项目级 .goat 目录（等价于 Claude Code 的 .claude/）。

    新建 `<project_root>/.goat/` 目录，用于存放：
    - skills/        — 项目专属 skill，仅本项目生效
    - rules/         — 项目规则（类似 CLAUDE.md）
    - settings.json  — 项目专属设置（后续扩展）
    """
    root = workspace or Path.cwd().resolve()
    goat_dir = root / ".goat"
    goat_dir.mkdir(parents=True, exist_ok=True)
    return goat_dir


def ensure_workspace_goat_layout(workspace: Path | None = None) -> Path:
    """确保打开的工作空间下 .goat/ 目录布局完整。

    返回值：项目级 .goat 目录路径。
    副作用：若 .goat 不存在则创建；始终确保 skills/ 和 GOAT.md 存在。
    """
    root = workspace or Path.cwd().resolve()
    goat_dir = root / ".goat"
    goat_dir.mkdir(parents=True, exist_ok=True)
    (goat_dir / "skills").mkdir(parents=True, exist_ok=True)
    (goat_dir / "GOAT.md").touch()  # 仅占位，不写内容
    return goat_dir
