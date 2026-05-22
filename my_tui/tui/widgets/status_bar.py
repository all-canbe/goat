from __future__ import annotations

from textual.widgets import Static
from textual.reactive import reactive

from ..theme import RED_BLACK_THEME


class StatusBar(Static):
    mode_name = reactive("AGENT")
    is_streaming = reactive(False)
    context_pct = reactive(0.0)

    def render(self) -> str:
        mode_color = {
            "AGENT": "#cc0000",
            "PLAN": "#ff6600",
            "YOLO": "#ff0000",
        }.get(self.mode_name, "#cc0000")

        left = (
            f"[#888888] Tab:[/#888888] "
            f"[{mode_color}]{self.mode_name}[/{mode_color}]"
        )
        center = " ▶ 流式输出中" if self.is_streaming else " ⏸ 空闲"
        right = (
            f"[#888888]上下文:[/#888888] "
            f"[#e0e0e0]{self.context_pct:.0f}%[/#e0e0e0]"
        )

        # Pad to terminal width
        return f"{left}  │{center}  │  {right}"