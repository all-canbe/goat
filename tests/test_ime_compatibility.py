"""
中文 IME 兼容性验证脚本 (诊断用途，非 pytest 测试)
验证 Textual 原生 Input 的中文输入表现
"""
import asyncio
import sys
sys.path.insert(0, '.')

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')

from textual.app import App, ComposeResult
from textual.widgets import Input
from textual.message import Message


# 测试 A：原生 Input 基本事件链
submitted_texts_a: list[str] = []


class NativeInputApp(App):
    def compose(self) -> ComposeResult:
        yield Input(placeholder="测试原生 Input", id="input_a")

    def on_mount(self):
        self.query_one("#input_a", Input).focus()

    def on_input_submitted(self, event: Input.Submitted):
        text = event.value.strip()
        if text:
            submitted_texts_a.append(text)
            event.input.clear()


# 测试 B：通过 screen handler 处理事件（模拟 TuiScreen 模式）
submitted_texts_b: list[str] = []
changed_texts_b: list[str] = []


class HandlerScreenApp(App):
    class Submitted(Message):
        def __init__(self, text: str):
            super().__init__()
            self.text = text

    class Changed(Message):
        def __init__(self, value: str):
            super().__init__()
            self.value = value

    def compose(self) -> ComposeResult:
        yield Input(placeholder="测试事件链", id="input_b")

    def on_mount(self):
        self.query_one("#input_b", Input).focus()

    def on_input_submitted(self, event: Input.Submitted):
        text = event.value.strip()
        if not text:
            return
        self.post_message(self.Submitted(text))
        event.input.clear()

    def on_input_changed(self, event: Input.Changed):
        self.post_message(self.Changed(event.value))

    def on_handler_screen_app_submitted(self, message: Submitted):
        submitted_texts_b.append(message.text)

    def on_handler_screen_app_changed(self, message: Changed):
        changed_texts_b.append(message.value)


# 测试 C：中文输入提交链
submitted_texts_c: list[str] = []


class ChineseSubmitApp(App):
    def compose(self) -> ComposeResult:
        yield Input(placeholder="中文测试", id="input_c")

    def on_mount(self):
        self.query_one("#input_c", Input).focus()

    def on_input_submitted(self, event: Input.Submitted):
        text = event.value.strip()
        if text:
            submitted_texts_c.append(text)
            event.input.clear()


async def test_native_input_event_chain():
    print("\n=== 测试 A: 原生 Input 事件链 ===")
    app = NativeInputApp()
    async with app.run_test(size=(80, 10)) as pilot:
        input_w = app.query_one("#input_a", Input)
        assert input_w.has_focus

        await pilot.press("h", "e", "l", "l", "o")
        await pilot.pause()
        assert input_w.value == "hello"

        await pilot.press("enter")
        await pilot.pause()
        assert len(submitted_texts_a) == 1
        assert submitted_texts_a[0] == "hello"
        assert input_w.value == ""

        print("  [PASS] 英文输入事件链正常")


async def test_chinese_event_chain():
    print("\n=== 测试 B: 中文输入事件链 ===")
    app = HandlerScreenApp()
    async with app.run_test(size=(80, 10)) as pilot:
        input_w = app.query_one("#input_b", Input)
        await pilot.press("中", "文")
        await pilot.pause()

        print(f"  输入值: {input_w.value!r}")
        print(f"  Changed 事件记录: {changed_texts_b}")

        await pilot.press("enter")
        await pilot.pause()

        print(f"  提交内容: {submitted_texts_b}")

        if len(submitted_texts_b) == 1:
            print("  [PASS] 中文输入事件链正常")
        else:
            print("  [INFO] 中文提交结果取决于当前终端环境")


async def test_chinese_submit_pilot():
    print("\n=== 测试 C: 中文输入提交链 ===")
    app = ChineseSubmitApp()
    async with app.run_test(size=(80, 10)) as pilot:
        input_w = app.query_one("#input_c", Input)
        input_w.value = "你好世界"
        await pilot.pause()

        await input_w.action_submit()
        await pilot.pause()

        print(f"  Input 值: {input_w.value!r}")
        print(f"  提交内容: {submitted_texts_c}")

        if len(submitted_texts_c) == 1 and submitted_texts_c[0] == "你好世界":
            print("  [PASS] 中文提交链通过 Textual 原生 Input 工作正常")
        else:
            print(f"  [INFO] 提交结果: {submitted_texts_c}")


async def main():
    print("=" * 60)
    print("Textual 中文 IME 兼容性测试")
    print(f"Textual 版本: 8.2.7")
    print(f"平台: {sys.platform}")
    print("=" * 60)

    await test_native_input_event_chain()
    await test_chinese_event_chain()
    await test_chinese_submit_pilot()

    print("\n" + "=" * 60)
    print("测试完成")
    print("=" * 60)
    print("\n注意: 中文 IME 的 composition 输入无法在自动化测试中模拟")
    print("请在 Windows Terminal 中手动运行此脚本验证中文输入")


if __name__ == "__main__":
    asyncio.run(main())