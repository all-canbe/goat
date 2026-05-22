from __future__ import annotations

from textual.widgets import Input, Static
from textual.app import ComposeResult
from textual.reactive import reactive
from textual.binding import Binding
from textual.message import Message
from textual.containers import Horizontal


class InputPanel(Static):
    BINDINGS = [
        Binding("escape", "cancel", "取消", priority=True),
        Binding("ctrl+c", "interrupt", "中断", priority=True),
    ]

    placeholder = reactive("输入消息... (Shift+Enter 换行)")
    is_streaming = reactive(False)

    class Submitted(Message):
        def __init__(self, text: str):
            super().__init__()
            self.text = text

    class Interrupted(Message):
        pass

    class InputChanged(Message):
        def __init__(self, value: str):
            super().__init__()
            self.value = value

    def compose(self) -> ComposeResult:
        yield Horizontal(
            Static(">", id="prompt_icon"),
            Input(
                placeholder=self.placeholder,
                id="chat_input",
            ),
            id="input_row",
        )

    def on_input_changed(self, event: Input.Changed) -> None:
        self.post_message(self.InputChanged(event.value))

    def on_input_submitted(self, event: Input.Submitted) -> None:
        value = event.value.strip()
        if not value:
            return
        self.post_message(self.Submitted(value))
        event.input.clear()

    def action_cancel(self) -> None:
        input_widget = self.query_one("#chat_input", Input)
        input_widget.clear()

    def action_interrupt(self) -> None:
        self.post_message(self.Interrupted())

    def set_text(self, text: str):
        input_widget = self.query_one("#chat_input", Input)
        input_widget.value = text

    def focus_input(self):
        self.query_one("#chat_input", Input).focus()

    def watch_is_streaming(self, streaming: bool):
        input_widget = self.query_one("#chat_input", Input)
        if streaming:
            input_widget.placeholder = "流式输出中... (Ctrl+C 中断)"
            input_widget.disabled = True
        else:
            input_widget.placeholder = self.placeholder
            input_widget.disabled = False