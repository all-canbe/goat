from __future__ import annotations

from typing import Any


class StepsTracker:
    """跟踪当前任务的多步骤计划，用于在 System Prompt 中向 Agent 提供进度可见性。"""
    def __init__(self):
        self.steps: list[dict] = []
        self.completed: set[int] = set()

    def add_plan(self, steps: list[str]):
        self.steps = [
            {"desc": s, "index": i} for i, s in enumerate(steps)
        ]
        self.completed.clear()

    def mark_done(self, index: int):
        self.completed.add(index)

    def register_tool(self, tool_name: str, params: dict):
        pass

    def get_progress(self) -> str:
        if not self.steps:
            return ""
        lines = ["## 当前计划进度"]
        for s in self.steps:
            status = "✅" if s["index"] in self.completed else "⬜"
            lines.append(f"- {status} 步骤 {s['index'] + 1}: {s['desc']}")
        return "\n".join(lines)
