from __future__ import annotations

from textual.widgets import Static, ListView, ListItem, Label
from textual.app import ComposeResult
from textual.reactive import reactive
from textual.containers import Vertical
from textual import events

from ..state import SubAgentInfo, TaskInfo


class SidePanel(Static):
    """侧边栏：子 Agent 状态与后台任务实时展示。

    - 通过 reactive 属性 sub_agents / tasks 接收 TUIState 变更
    - Tab 键在 Agent 列表与任务列表之间切换焦点
    """

    sub_agents: dict[str, SubAgentInfo] = reactive({}, always_update=True)
    tasks: dict[str, TaskInfo] = reactive({}, always_update=True)

    STATUS_ICONS = {
        "running": "\U0001f504",       # 🔄
        "completed": "\u2705",         # ✅
        "failed": "\u274c",            # ❌
        "cancelled": "\U0001f6ab",     #  🚫
        "pending": "\u23f3",           # ⏳
    }

    STATUS_COLORS = {
        "running": "#00cc66",      # green
        "completed": "#00cc66",    # green
        "failed": "#ff0000",       # red
        "cancelled": "#888888",    # gray
        "pending": "#ffaa00",      # amber
    }

    def compose(self) -> ComposeResult:
        yield Vertical(
            Static("[bold #ff1a1a]子 Agent[/bold #ff1a1a]", id="agent_title"),
            ListView(id="agent_list", classes="side-list"),
            Static("", classes="spacer"),
            Static("[bold #ff1a1a]后台任务[/bold #ff1a1a]", id="task_title"),
            ListView(id="task_list", classes="side-list"),
            id="side_container",
        )

    # ----------------------------------------------------------------
    # 渲染
    # ----------------------------------------------------------------

    def _render_agent_item(self, info: SubAgentInfo) -> str:
        """单行格式：<图标> <id> <name> <role>  <task摘要>"""
        icon = self.STATUS_ICONS.get(info.status, "?")
        color = self.STATUS_COLORS.get(info.status, "#888888")
        short_id = info.agent_id[:8]
        task_text = info.task[:28] + "\u2026" if len(info.task) > 28 else info.task
        return (
            f"[{color}]{icon}[/{color}] "
            f"[#888888]{short_id}[/#888888] "
            f"[#e0e0e0]{info.name}[/#e0e0e0] "
            f"[#666666]{info.role}[/#666666]\n"
            f"     [#555555]{task_text}[/#555555]"
        )

    def _render_task_item(self, info: TaskInfo) -> str:
        """单行格式：<图标> <id> <name>  <进度百分比>"""
        icon = self.STATUS_ICONS.get(info.status, "?")
        color = self.STATUS_COLORS.get(info.status, "#888888")
        short_id = info.task_id[:8]
        pct = int(info.progress * 100)
        return (
            f"[{color}]{icon}[/{color}] "
            f"[#888888]{short_id}[/#888888] "
            f"[#e0e0e0]{info.name}[/#e0e0e0] "
            f"[#888888]{pct}%[/#888888]"
        )

    # ----------------------------------------------------------------
    # Reactive watchers
    # ----------------------------------------------------------------

    def watch_sub_agents(self, agents: dict[str, SubAgentInfo]):
        if not self.is_mounted:
            return
        agent_list = self.query_one("#agent_list", ListView)
        agent_list.clear()
        for info in agents.values():
            item = ListItem(Label(self._render_agent_item(info)))
            agent_list.append(item)

    def watch_tasks(self, tasks: dict[str, TaskInfo]):
        if not self.is_mounted:
            return
        task_list = self.query_one("#task_list", ListView)
        task_list.clear()
        for info in tasks.values():
            item = ListItem(Label(self._render_task_item(info)))
            task_list.append(item)

    # ----------------------------------------------------------------
    # 键盘交互
    # ----------------------------------------------------------------

    def on_key(self, event: events.Key) -> None:
        """Tab 键在 Agent 列表与任务列表之间切换焦点。"""
        if event.key != "tab":
            return
        event.stop()
        agent_list = self.query_one("#agent_list", ListView)
        task_list = self.query_one("#task_list", ListView)
        focused = self.screen.focused
        if focused is task_list:
            agent_list.focus()
        else:
            task_list.focus()