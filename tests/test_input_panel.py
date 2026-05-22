"""
Test InputPanel from the actual codebase
"""
import asyncio
from textual.app import App, ComposeResult
from my_tui.tui.widgets.input_panel import InputPanel


class TestApp(App):
    def compose(self) -> ComposeResult:
        yield InputPanel(id="test_panel")

    def on_mount(self):
        self.query_one("#test_panel", InputPanel).focus_input()


# Track submissions
submitted_texts: list[str] = []


class TestScreenApp(App):
    def compose(self) -> ComposeResult:
        yield InputPanel(id="test_panel")

    def on_mount(self):
        self.query_one("#test_panel", InputPanel).focus_input()

    def on_input_panel_submitted(self, message: InputPanel.Submitted):
        text = message.text
        submitted_texts.append(text)
        print(f"  TuiScreen handler called: text={text!r}")


async def test():
    app = TestScreenApp()
    async with app.run_test() as pilot:
        await pilot.pause()
        
        input_widget = app.query_one("#chat_input")
        print(f"Input focused: {input_widget.has_focus}")
        print(f"Input value: {input_widget.value!r}")
        
        # Type something
        await pilot.press("h", "e", "l", "l", "o")
        await pilot.pause()
        
        print(f"Before Enter: submitted_texts={submitted_texts}")
        print(f"Input value: {input_widget.value!r}")
        
        # Press Enter
        await pilot.press("enter")
        await pilot.pause()
        
        print(f"After Enter: submitted_texts={submitted_texts}")
        print(f"Input value after: {input_widget.value!r}")
        
        if len(submitted_texts) == 1 and submitted_texts[0] == "hello":
            print("\nTest PASSED: InputPanel submit chain works correctly!")
        else:
            print(f"\nTest FAILED: Expected ['hello'], got {submitted_texts}")
            # Debug: check if the handler was called at the InputPanel level
            print("Checking InputPanel.on_input_submitted...")
            panel = app.query_one("#test_panel", InputPanel)
            handler = getattr(panel, 'on_input_submitted', None)
            print(f"  handler exists: {handler is not None}")
            if handler:
                from textual.widgets import Input
                print(f"  Input.Submitted.handler_name: {Input.Submitted.handler_name}")
                print(f"  InputPanel handler name: on_input_submitted")


if __name__ == "__main__":
    asyncio.run(test())