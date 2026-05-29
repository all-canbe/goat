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