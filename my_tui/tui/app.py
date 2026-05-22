from __future__ import annotations

import asyncio

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.screen import Screen

from ..core.event_bus import EventBus, EventType
from ..security.approval import PermissionMode

from .theme import RED_BLACK_THEME
from .state import TUIState, MessageData, MessageRole
from .bridge import TUIBridge
from .widgets import HeaderWidget, ChatPanel, InputPanel, StatusBar, SidePanel, CommandMenu


TUI_CSS = RED_BLACK_THEME.to_css() + """
Screen {
    background: $bg;
}

HeaderWidget {
    dock: top;
    height: 1;
    background: $surface;
    color: $text;
    padding: 0 1;
}

#main_container {
    layout: horizontal;
    height: 1fr;
}

#chat_area {
    width: 1fr;
    height: 100%;
    border: solid $border;
}

ChatPanel {
    height: 1fr;
    padding: 0 1;
}

#chat_container {
    height: 100%;
}

#chat_log {
    height: 100%;
    background: $bg;
    color: $text;
    scrollbar-color: $scrollbar;
    scrollbar-color-hover: $scrollbar-hover;
    scrollbar-size: 2 2;
}

#chat_log .highlight {
    color: $primary-light;
}

InputPanel {
    dock: bottom;
    height: 3;
    background: $surface;
    border-top: solid $border;
    padding: 0 1;
}

#cmd_menu {
    dock: bottom;
    height: auto;
    max-height: 22;
    display: none;
    background: $surface;
    border: solid $border;
    margin: 0 1 0 1;
}

#cmd_menu.-visible {
    display: block;
}

#cmd_title {
    padding: 0 1;
    border-bottom: solid $border;
}

#cmd_content {
    height: auto;
    max-height: 20;
    padding: 0 1;
    background: $surface;
    scrollbar-color: $scrollbar;
    scrollbar-color-hover: $scrollbar-hover;
    scrollbar-size: 2 2;
    overflow-y: auto;
}

#input_row {
    layout: horizontal;
    height: 3;
    align: center middle;
}

#prompt_icon {
    width: 2;
    color: $primary;
    text-style: bold;
}

#chat_input {
    width: 1fr;
    height: 3;
    background: $surface-light;
    color: $text;
    border: none;
}

#chat_input:focus {
    background: $surface-lighter;
    color: $text-bright;
}

#chat_input .input-cursor {
    color: $primary-light;
    text-style: bold;
}

StatusBar {
    dock: bottom;
    height: 1;
    background: $surface;
    color: $text-dim;
    padding: 0 1;
    border-top: solid $border;
}

#side_panel {
    width: 30;
    height: 100%;
    border: solid $border;
    padding: 0 1;
    background: $surface;
}

.side-list {
    height: 1fr;
    background: $surface;
    border: none;
    scrollbar-color: $scrollbar;
    scrollbar-color-hover: $scrollbar-hover;
    scrollbar-size: 2 2;
}

.side-list ListItem {
    background: $bg;
    color: $text;
    padding: 0 1;
}

.side-list ListItem:hover {
    background: $selection-bg;
    color: $selection-fg;
}

.spacer {
    height: 1;
}

#agent_title, #task_title {
    padding: 1 0 0 0;
    border-bottom: solid $border;
}
"""


class TuiScreen(Screen):
    BINDINGS = [
        Binding("tab", "cycle_mode", "切换模式", priority=True),
        Binding("ctrl+c", "interrupt", "中断", priority=True),
        Binding("ctrl+d", "quit", "退出", priority=True),
        Binding("escape", "focus_input", "聚焦输入", priority=True),
        Binding("y", "approve", "批准", priority=True),
        Binding("n", "reject", "拒绝", priority=True),
        Binding("up", "cmd_up", "上移", priority=True),
        Binding("down", "cmd_down", "下移", priority=True),
    ]

    def __init__(self, event_bus: EventBus, state: TUIState, bridge: TUIBridge,
                 user_input_queue: asyncio.Queue[str] | None = None, **kwargs):
        super().__init__(**kwargs)
        self._event_bus = event_bus
        self._state = state
        self._bridge = bridge
        self._user_input_queue = user_input_queue

    def compose(self) -> ComposeResult:
        yield HeaderWidget(id="header")
        yield Horizontal(
            Vertical(
                ChatPanel(id="chat_panel"),
                id="chat_area",
            ),
            SidePanel(id="side_panel"),
            id="main_container",
        )
        yield CommandMenu(id="cmd_menu")
        yield InputPanel(id="input_panel")
        yield StatusBar(id="status_bar")

    def on_mount(self):
        header = self.query_one("#header", HeaderWidget)
        header.connected = self._state.connected
        header.provider_name = self._state.provider_name
        header.model_name = self._state.model_name
        header.mode = self._state.mode
        self._last_version = -1
        self._bridge.on_state_changed = self._on_state_changed
        self._sync_state()
        self.query_one("#input_panel", InputPanel).focus_input()

    def _on_state_changed(self):
        self._sync_state()

    def _sync_state(self):
        if self._state.version <= self._last_version:
            return
        self._last_version = self._state.version

        chat = self.query_one("#chat_panel", ChatPanel)
        header = self.query_one("#header", HeaderWidget)
        status_bar = self.query_one("#status_bar", StatusBar)
        side = self.query_one("#side_panel", SidePanel)
        inp = self.query_one("#input_panel", InputPanel)

        s = self._state

        chat.messages = list(s.messages)
        chat.is_streaming = s.is_streaming
        chat.streaming_content = s.streaming_content
        chat.pending_approval = s.pending_approval

        header.mode = s.mode
        header.model_name = s.model_name
        header.provider_name = s.provider_name
        header.token_input = s.token_usage.input_tokens
        header.token_output = s.token_usage.output_tokens
        header.cost = s.token_usage.total_cost
        header.connected = s.connected
        header.is_streaming = s.is_streaming
        header.message_count = s.message_count

        status_bar.mode_name = self._mode_label(s.mode)
        status_bar.is_streaming = s.is_streaming
        status_bar.context_pct = s.context_usage_pct

        side.sub_agents = dict(s.sub_agents)
        side.tasks = dict(s.tasks)

        inp.is_streaming = s.is_streaming

    def _mode_label(self, mode: PermissionMode) -> str:
        return {
            PermissionMode.DEFAULT: "AGENT",
            PermissionMode.PLAN: "PLAN",
            PermissionMode.YOLO: "YOLO",
        }.get(mode, "AGENT")

    def action_cycle_mode(self):
        modes = [PermissionMode.PLAN, PermissionMode.DEFAULT, PermissionMode.YOLO]
        current = self._state.mode
        try:
            idx = modes.index(current)
        except ValueError:
            idx = 0
        next_mode = modes[(idx + 1) % len(modes)]
        self._state.mode = next_mode
        self._event_bus.publish_nowait("tui_app", EventType.MESSAGE, f"模式切换: {self._mode_label(next_mode)}")

    def action_interrupt(self):
        self._state.is_streaming = False
        self._state.is_thinking = False
        msg = MessageData(role=MessageRole.SYSTEM, content="⏹ 已中断")
        self._state.messages.append(msg)
        self._event_bus.publish_nowait("tui_app", EventType.MESSAGE, "[中断] 用户中断了流式输出")

    def action_quit(self):
        self.app.exit()

    def action_focus_input(self):
        cmd_menu = self.query_one("#cmd_menu", CommandMenu)
        if cmd_menu.visible:
            cmd_menu.visible = False
            return
        self.query_one("#input_panel", InputPanel).focus_input()

    def action_cmd_up(self):
        cmd_menu = self.query_one("#cmd_menu", CommandMenu)
        if cmd_menu.visible:
            cmd_menu.select_prev()

    def action_cmd_down(self):
        cmd_menu = self.query_one("#cmd_menu", CommandMenu)
        if cmd_menu.visible:
            cmd_menu.select_next()

    def action_approve(self):
        if self._state.pending_approval:
            self._state.pending_approval.callback_approve()
            self._state.pending_approval = None

    def action_reject(self):
        if self._state.pending_approval:
            self._state.pending_approval.callback_reject()
            self._state.pending_approval = None

    def on_input_panel_input_changed(self, message: InputPanel.InputChanged) -> None:
        value = message.value
        cmd_menu = self.query_one("#cmd_menu", CommandMenu)
        if value.startswith("/"):
            cmd_menu.visible = True
            cmd_menu.filter_text = value
        else:
            cmd_menu.visible = False

    def on_input_panel_submitted(self, message: InputPanel.Submitted):
        text = message.text
        cmd_menu = self.query_one("#cmd_menu", CommandMenu)
        if cmd_menu.visible:
            selected = cmd_menu.get_selected_command()
            if selected:
                self._handle_command(selected)
            else:
                self._handle_command(text)
            cmd_menu.visible = False
            return
        if self._user_input_queue is not None:
            self._user_input_queue.put_nowait(text)
        self._bridge.add_user_message(text)
        self._sync_state()

    def on_click(self):
        self.query_one("#input_panel", InputPanel).focus_input()

    def _handle_command(self, cmd: str):
        base = cmd.split()[0].lower() if cmd.strip() else ""
        rest = cmd[len(base):].strip()

        # ── 审批模式切换（通过 bridge.set_mode） ──
        if base == "/agent":
            self._bridge.set_mode("agent")
            self._event_bus.publish_nowait(
                "tui_app", EventType.MESSAGE, "已切换到 AGENT 模式（每次确认）"
            )
        elif base == "/plan":
            self._bridge.set_mode("plan")
            self._event_bus.publish_nowait(
                "tui_app", EventType.MESSAGE, "已切换到 PLAN 模式（只读调查，写操作/Shell 全部阻止）"
            )
        elif base == "/yolo":
            self._bridge.set_mode("yolo")
            self._event_bus.publish_nowait(
                "tui_app", EventType.MESSAGE, "已切换到 YOLO 模式（自动批准，安全守卫仍生效）"
            )
        elif base == "/mode":
            if rest:
                self._bridge.set_mode(rest)
                self._event_bus.publish_nowait(
                    "tui_app", EventType.MESSAGE, f"模式切换: {rest.upper()}"
                )
            else:
                modes = [PermissionMode.PLAN, PermissionMode.DEFAULT, PermissionMode.YOLO]
                current = self._state.mode
                try:
                    idx = modes.index(current)
                except ValueError:
                    idx = 0
                next_mode = modes[(idx + 1) % len(modes)]
                self._bridge.set_mode(self._mode_label(next_mode).lower())
                self._event_bus.publish_nowait(
                    "tui_app", EventType.MESSAGE, f"模式切换: {self._mode_label(next_mode)}"
                )

        # ── TUI 内置命令 ──
        elif base == "/help":
            help_text = (
                "可用命令:\n"
                "  /help        — 显示此帮助\n"
                "  /clear       — 清空当前对话\n"
                "  /status      — 显示系统状态\n"
                "  /quit        — 退出程序\n"
                "\n"
                "审批模式:\n"
                "  /agent       — 切换到 AGENT 模式（每次确认）\n"
                "  /plan        — 切换到 PLAN 模式（只读）\n"
                "  /yolo        — 切换到 YOLO 模式（自动批准）\n"
                "  /mode <mode> — 切换审批模式（无参数则循环切换）\n"
                "\n"
                "子 Agent:\n"
                "  /spawn <角色> <任务>  — 创建子 Agent\n"
                "  /list                 — 列出所有子 Agent\n"
                "  /collect [ids]        — 收集子 Agent 结果\n"
                "  /cancel <id>          — 取消子 Agent\n"
                "  /eval <id> <消息>     — 向子 Agent 发送消息\n"
                "\n"
                "会话管理:\n"
                "  /sessions             — 列出会话\n"
                "  /session [id]         — 切换会话\n"
                "  /new [标题]           — 创建新会话\n"
                "  /session_rename <id> <名称>  — 重命名\n"
                "  /session_delete <id>  — 删除\n"
                "  /search <关键词>      — 搜索消息\n"
                "  /export [id] [format] — 导出\n"
                "  /resume [id]          — 恢复会话\n"
                "  /fork <id> [turn]     — 分叉会话\n"
                "\n"
                "信息:\n"
                "  /skills, /roles, /cost, /provider, /model, /sub_model\n"
                "\n"
                "后台任务:\n"
                "  /task, /task_list, /task_cancel, /task_pause,\n"
                "  /task_resume, /task_recover\n"
            )
            self._event_bus.publish_nowait("tui_app", EventType.MESSAGE, help_text)
        elif base == "/clear":
            self._state.messages.clear()
            self._state.message_count = 0
            self._state.bump()
            self._event_bus.publish_nowait("tui_app", EventType.MESSAGE, "对话已清空")
        elif base == "/quit":
            self.app.exit()
        elif base == "/status":
            s = self._state
            lines = [
                f"模式: {self._mode_label(s.mode)}",
                f"Provider: {s.provider_name}",
                f"模型: {s.model_name}",
                f"连接: {'已连接' if s.connected else '未连接'}",
                f"消息数: {s.message_count}",
                f"上下文使用: {s.context_usage_pct:.1f}%",
                f"Token 输入: {s.token_usage.input_tokens}",
                f"Token 输出: {s.token_usage.output_tokens}",
                f"总成本: ${s.token_usage.total_cost:.6f}",
            ]
            self._event_bus.publish_nowait(
                "tui_app", EventType.MESSAGE, "\n".join(lines)
            )

        # ── 其他命令：通过 user_input_queue 送后端处理 ──
        else:
            if self._user_input_queue is not None:
                self._user_input_queue.put_nowait(cmd)

    def on_input_panel_interrupted(self):
        self.action_interrupt()


class TuiApp(App):
    CSS = TUI_CSS

    def __init__(self, event_bus: EventBus, state: TUIState, bridge: TUIBridge,
                 user_input_queue: asyncio.Queue[str] | None = None):
        super().__init__()
        self._event_bus = event_bus
        self._state = state
        self._bridge = bridge
        self._user_input_queue = user_input_queue

    def on_mount(self):
        self.push_screen(TuiScreen(self._event_bus, self._state, self._bridge,
                                   user_input_queue=self._user_input_queue))
        self.title = "Goat TUI"
        self.sub_title = "SubAgent TUI"