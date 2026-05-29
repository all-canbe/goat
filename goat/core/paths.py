from __future__ import annotations

from pathlib import Path


DATA_ROOT = Path.home() / ".goat"
DATA_ROOT.mkdir(parents=True, exist_ok=True)

MEMORY_DB = DATA_ROOT / "memory.db"
AUDIT_DB = DATA_ROOT / "audit.db"
CONVERSATIONS_DB = DATA_ROOT / "conversations.db"
TASKS_DB = DATA_ROOT / "tasks.db"
SCREENSHOTS_DIR = DATA_ROOT / "screenshots"
SCREENSHOTS_DIR.mkdir(parents=True, exist_ok=True)
