from __future__ import annotations

from textual.widgets import Static
from textual.app import ComposeResult
from textual.reactive import reactive


COMMANDS = [
    ("/help", "显示帮助信息"),
    ("/clear", "清空当前对话"),
    ("/status", "显示系统状态"),
    ("/quit", "退出程序"),
    ("", "── 审批模式 ──"),
    ("/agent", "切换到 AGENT 模式（每次确认）"),
    ("/plan", "切换到 PLAN 模式（只读）"),
    ("/yolo", "切换到 YOLO 模式（自动批准）"),
    ("/mode", "切换 Agent/命令模式 — /mode <agent|plan|yolo>"),
    ("", "── 子 Agent ──"),
    ("/spawn", "手动创建子 Agent — /spawn <角色> <任务>"),
    ("/list", "列出所有子 Agent 及其状态"),
    ("/collect", "收集子 Agent 结果 — /collect [ids]"),
    ("/cancel", "取消指定子 Agent — /cancel <id>"),
    ("/eval", "向子 Agent 发送消息 — /eval <id> <消息>"),
    ("", "── 会话管理 ──"),
    ("/sessions", "列出所有对话历史"),
    ("/session", "切换/列出会话 — /session [id]"),
    ("/new", "创建新会话 — /new [标题]"),
    ("/session_rename", "重命名会话 — /session_rename <id> <名称>"),
    ("/session_delete", "删除会话 — /session_delete <id>"),
    ("/search", "搜索历史消息 — /search <关键词>"),
    ("/export", "导出会话 — /export [id] [format]"),
    ("/resume", "恢复最近或指定会话 — /resume [id]"),
    ("/fork", "基于历史会话分叉新会话 — /fork <id> [turn]"),
    ("", "── 信息 ──"),
    ("/memory", "管理跨会话记忆 — /memory [key] [content]"),
    ("/skills", "列出已注册技能"),
    ("/roles", "列出可用角色类型"),
    ("/cost", "显示 Token 用量统计与估算成本"),
    ("/provider", "切换/显示 LLM Provider — /provider [type]"),
    ("/model", "切换/显示当前模型 — /model [name]"),
    ("/sub_model", "切换/显示子 Agent 默认模型 — /sub_model [model]"),
    ("", "── 后台任务 ──"),
    ("/task", "提交持久化后台任务 — /task <名称> [描述]"),
    ("/task_list", "列出任务 — /task_list [status]"),
    ("/task_cancel", "取消任务 — /task_cancel <id>"),
    ("/task_pause", "暂停任务 — /task_pause <id>"),
    ("/task_resume", "恢复任务 — /task_resume <id>"),
    ("/task_recover", "恢复中断的任务"),
]


class CommandMenu(Static):
    visible = reactive(False)
    filter_text = reactive("")
    selected_index = reactive(0)

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self._filtered: list[tuple[str, str]] = []

    def compose(self) -> ComposeResult:
        yield Static("[bold]命令列表[/bold]", id="cmd_title")
        yield Static(id="cmd_content")

    def watch_filter_text(self, text: str):
        if not self.is_mounted:
            return
        self._rebuild()

    def watch_visible(self, visible: bool):
        self.set_class(visible, "-visible")
        if visible:
            self.selected_index = 0

    def _rebuild(self):
        text = self.filter_text
        if not text.startswith("/"):
            self._filtered = []
            self.query_one("#cmd_content", Static).update("")
            return

        lower = text.lower()
        filtered = [(cmd, desc) for cmd, desc in COMMANDS if cmd.startswith(lower)]
        self._filtered = filtered

        if not filtered:
            self.query_one("#cmd_content", Static).update("")
            self.selected_index = 0
            return

        if self.selected_index >= len(filtered):
            self.selected_index = 0

        self._render_content()

    def _render_content(self):
        if not self._filtered:
            self.query_one("#cmd_content", Static).update("")
            return
        lines = []
        for i, (cmd, desc) in enumerate(self._filtered):
            if i == self.selected_index:
                lines.append(f"[reverse]{cmd}   {desc}[/reverse]")
            else:
                lines.append(f"{cmd}   {desc}")
        self.query_one("#cmd_content", Static).update("\n".join(lines))

    @property
    def filtered_commands(self) -> list[tuple[str, str]]:
        return self._filtered

    def select_next(self):
        if not self._filtered:
            return
        if self.selected_index >= len(self._filtered) - 1:
            self.selected_index = 0
        else:
            self.selected_index += 1
        self._render_content()

    def select_prev(self):
        if not self._filtered:
            return
        if self.selected_index <= 0:
            self.selected_index = len(self._filtered) - 1
        else:
            self.selected_index -= 1
        self._render_content()

    def get_selected_command(self) -> str | None:
        if 0 <= self.selected_index < len(self._filtered):
            return self._filtered[self.selected_index][0]
        return None