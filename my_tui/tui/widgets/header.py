from __future__ import annotations

from textual.widgets import Static
from textual.app import ComposeResult
from textual.reactive import reactive
from textual.binding import Binding

from my_tui.security.approval import PermissionMode
from ..theme import RED_BLACK_THEME


class HeaderWidget(Static):
    mode = reactive(PermissionMode.DEFAULT)
    model_name = reactive("")
    provider_name = reactive("")
    token_input = reactive(0)
    token_output = reactive(0)
    cost = reactive(0.0)
    connected = reactive(False)
    is_streaming = reactive(False)
    message_count = reactive(0)

    MODE_LABELS = {
        PermissionMode.DEFAULT: (" AGENT ", "#cc0000"),
        PermissionMode.PLAN: (" PLAN  ", "#ff6600"),
        PermissionMode.YOLO: (" YOLO  ", "#ff0000"),
    }

    def render(self) -> str:
        mode_label, mode_color = self.MODE_LABELS.get(
            self.mode, (" AGENT ", "#cc0000")
        )
        conn_icon = "●" if self.connected else "○"
        conn_color = "#00cc66" if self.connected else "#555555"
        stream_indicator = " ▶" if self.is_streaming else ""
        cost_str = f"${self.cost:.4f}" if self.cost > 0 else ""

        return (
            f"[bold #ffffff]🐐 Goat TUI[/bold #ffffff]  "
            f"[{mode_color} on #1a1a1a]{mode_label}[/{mode_color} on #1a1a1a]"
            f"{stream_indicator}"
            f"  │  "
            f"[{conn_color}]{conn_icon}[/{conn_color}] "
            f"[#888888]{self.provider_name}[/#888888] "
            f"[#cc0000]{self.model_name}[/#cc0000]"
            f"  │  "
            f"[#888888]MSG:[/#888888] [#e0e0e0]{self.message_count}[/#e0e0e0]"
            f"  │  "
            f"[#888888]IN:[/#888888] [#e0e0e0]{self.token_input}[/#e0e0e0] "
            f"[#888888]OUT:[/#888888] [#e0e0e0]{self.token_output}[/#e0e0e0]"
            f"{'  │  ' + cost_str if cost_str else ''}"
        )