from __future__ import annotations

from textual.widgets import Static, RichLog
from textual.app import ComposeResult
from textual.reactive import reactive
from textual.containers import Container
from textual import events

from rich.markdown import Markdown
from rich.panel import Panel
from rich.text import Text
from rich.console import RenderableType

from ..state import MessageData, MessageRole, ApprovalRequest


_COLLAPSIBLE_ROLES = {MessageRole.TOOL_CALL, MessageRole.TOOL_RESULT}

# ── 消息类型视觉配置 ──────────────────────────────────────────────
_ROLE_STYLE = {
    MessageRole.USER: {
        "icon": "User",
        "title": "你",
        "border": "dim gray50",
        "title_color": "dim gray50",
        "content_color": "white",
    },
    MessageRole.ASSISTANT: {
        "icon": "",
        "title": "AI",
        "border": "bright_red",
        "title_color": "bright_red",
    },
    MessageRole.TOOL_CALL: {
        "icon": "\U0001f527",   # 🔧
        "title": "工具调用",
        "border": "dark_orange",
        "title_color": "dark_orange",
        "summary_color": "gray70",
    },
    MessageRole.TOOL_RESULT: {
        "icon": "\U0001f4cb",   # 📋
        "title": "工具结果",
        "border": "gray42",
        "title_color": "gray58",
        "summary_color": "gray50",
    },
    MessageRole.ERROR: {
        "icon": "\u274c",       # ❌
        "title": "错误",
        "border": "red",
        "title_color": "red",
    },
    MessageRole.SYSTEM: {
        "icon": "\u2139\ufe0f",  # ℹ️
        "title": "系统",
        "border": "gray37",
        "title_color": "gray50",
    },
}


class ChatPanel(Static):
    messages: list[MessageData] = reactive([], always_update=True)
    is_streaming = reactive(False)
    streaming_content = reactive("", always_update=True)
    pending_approval: ApprovalRequest | None = reactive(None, always_update=True)

    _known_count = 0
    _collapsed: dict[int, bool] = {}

    # ── compose ────────────────────────────────────────────────────

    def compose(self) -> ComposeResult:
        yield Container(
            RichLog(id="chat_log", highlight=True, markup=True, wrap=True),
            id="chat_container",
        )

    # ── 键盘交互：Enter 展开/折叠 ──────────────────────────────────

    def _on_key(self, event: events.Key) -> None:
        """捕获 Enter 键，折叠/展开最近的可折叠消息。"""
        if event.key != "enter":
            return
        # 从后向前查找最近的可折叠消息
        for i in range(len(self.messages) - 1, -1, -1):
            if self.messages[i].role in _COLLAPSIBLE_ROLES:
                self._collapsed[i] = not self._collapsed.get(i, True)
                self._full_refresh()
                event.stop()
                return

    def on_key(self, event: events.Key) -> None:
        """将按键事件路由到 _on_key。"""
        self._on_key(event)

    # ── 消息渲染核心 ──────────────────────────────────────────────

    def _render_message(self, msg: MessageData, idx: int = -1) -> RenderableType:
        """将消息渲染为 Rich renderable。"""
        match msg.role:
            case MessageRole.USER:
                return self._render_user(msg)
            case MessageRole.ASSISTANT:
                return self._render_assistant(msg)
            case MessageRole.TOOL_CALL:
                return self._render_tool_call(msg, idx)
            case MessageRole.TOOL_RESULT:
                return self._render_tool_result(msg, idx)
            case MessageRole.ERROR:
                return self._render_error(msg)
            case MessageRole.SYSTEM:
                return self._render_system(msg)
        return ""

    # ── 各消息类型渲染 ────────────────────────────────────────────

    def _render_user(self, msg: MessageData) -> RenderableType:
        style = _ROLE_STYLE[MessageRole.USER]
        content = Text(msg.content, style=style["content_color"])
        return Panel(
            content,
            title=f"[{style['title_color']}]{style['title']}[/]",
            title_align="left",
            border_style=style["border"],
            padding=(0, 1),
        )

    def _render_assistant(self, msg: MessageData) -> RenderableType:
        style = _ROLE_STYLE[MessageRole.ASSISTANT]
        name = f" {msg.agent_name}" if msg.agent_name else ""
        return Panel(
            Markdown(msg.content, code_theme="monokai"),
            title=f"[{style['title_color']}]{style['title']}{name}[/]",
            title_align="left",
            border_style=style["border"],
            padding=(0, 1),
        )

    def _render_tool_call(self, msg: MessageData, idx: int) -> RenderableType:
        style = _ROLE_STYLE[MessageRole.TOOL_CALL]
        collapsed = self._collapsed.get(idx, True)
        name = f" {msg.agent_name}" if msg.agent_name else ""

        if collapsed:
            summary = self._make_summary(msg.content, 120)
            body = Text.assemble(
                (summary, style["summary_color"]),
                ("\n[dim]  [Enter 展开][/dim]", "dim"),
            )
        else:
            body = Markdown(msg.content, code_theme="monokai")

        return Panel(
            body,
            title=f"[{style['title_color']}]{style['icon']} {style['title']}{name}[/]",
            title_align="left",
            border_style=style["border"],
            padding=(0, 1),
        )

    def _render_tool_result(self, msg: MessageData, idx: int) -> RenderableType:
        style = _ROLE_STYLE[MessageRole.TOOL_RESULT]
        collapsed = self._collapsed.get(idx, True)

        if collapsed:
            summary = self._make_summary(msg.content, 200)
            body = Text.assemble(
                (summary, style["summary_color"]),
                ("\n[dim]  [Enter 展开][/dim]", "dim"),
            )
        else:
            body = Markdown(msg.content, code_theme="monokai")

        return Panel(
            body,
            title=f"[{style['title_color']}]{style['icon']} {style['title']}[/]",
            title_align="left",
            border_style=style["border"],
            padding=(0, 1),
        )

    def _render_error(self, msg: MessageData) -> RenderableType:
        style = _ROLE_STYLE[MessageRole.ERROR]
        return Panel(
            Text(msg.content, style="red"),
            title=f"[{style['title_color']}]{style['icon']} {style['title']}[/]",
            title_align="left",
            border_style=style["border"],
            padding=(0, 1),
        )

    def _render_system(self, msg: MessageData) -> RenderableType:
        style = _ROLE_STYLE[MessageRole.SYSTEM]
        return Panel(
            Text(msg.content, style="gray50"),
            title=f"[{style['title_color']}]{style['icon']} {style['title']}[/]",
            title_align="left",
            border_style=style["border"],
            padding=(0, 1),
        )

    # ── 辅助方法 ───────────────────────────────────────────────────

    @staticmethod
    def _make_summary(content: str, max_len: int) -> str:
        """截取内容摘要（取首行或前 N 字符）。"""
        first_line = content.split("\n", 1)[0].strip()
        if len(first_line) > max_len:
            return first_line[:max_len] + "…"
        return first_line

    # ── 审批渲染（保持原逻辑） ─────────────────────────────────────

    def _render_approval(self, req: ApprovalRequest) -> str:
        if not req:
            return ""
        return (
            f"\n"
            f"[#cc0000]╔══ 需要批准 ══[/#cc0000]\n"
            f"[#cc0000]║[/#cc0000] [#ff6600]工具:[/#ff6600] [#e0e0e0]{req.tool_name}[/#e0e0e0]\n"
            f"[#cc0000]║[/#cc0000] [#ff6600]参数:[/#ff6600] [#888888]{req.args[:200]}[/#888888]\n"
            f"[#cc0000]╚══[/#cc0000]\n"
            f"[#888888]  (y)批准  (n)拒绝  (v)查看详情[/#888888]\n"
        )

    # ── 刷新逻辑 ───────────────────────────────────────────────────

    def _full_refresh(self):
        chat_log = self.query_one("#chat_log", RichLog)
        chat_log.clear()
        for i, msg in enumerate(self.messages):
            rendered = self._render_message(msg, idx=i)
            if rendered:
                chat_log.write(rendered)
        if self.is_streaming and self.streaming_content:
            chat_log.write(
                Panel(
                    Text(self.streaming_content),
                    title="AI 流式输出",
                    title_align="left",
                    border_style="bright_red",
                    padding=(0, 1),
                )
            )
        if self.pending_approval:
            chat_log.write(self._render_approval(self.pending_approval))

    def watch_messages(self, msgs: list[MessageData]):
        if not self.is_mounted:
            self._known_count = len(msgs)
            return
        chat_log = self.query_one("#chat_log", RichLog)
        for i in range(self._known_count, len(msgs)):
            # 新消息如果是可折叠类型，默认折叠
            if msgs[i].role in _COLLAPSIBLE_ROLES and i not in self._collapsed:
                self._collapsed[i] = True
            rendered = self._render_message(msgs[i], idx=i)
            if rendered:
                chat_log.write(rendered)
        self._known_count = len(msgs)
        if self.is_streaming or self.pending_approval:
            self._full_refresh()

    def watch_streaming_content(self, content: str):
        if self.is_mounted:
            self._full_refresh()

    def watch_pending_approval(self, req: ApprovalRequest | None):
        if self.is_mounted:
            self._full_refresh()

    def watch_is_streaming(self, streaming: bool):
        if self.is_mounted:
            self._full_refresh()

    def on_mount(self):
        # 初始化已有消息的折叠状态
        for i, msg in enumerate(self.messages):
            if msg.role in _COLLAPSIBLE_ROLES:
                self._collapsed[i] = True
        self._known_count = len(self.messages)
        self.watch_messages(self.messages)