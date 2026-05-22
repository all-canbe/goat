"""
Minimal Textual test to verify Input.Submitted event handling
"""
from typing import ClassVar
from textual.app import App, ComposeResult
from textual.widgets import Input, Static
from textual.message import Message


class TestInputHandler(Static):
    """Widget that handles Input.Submitted"""
    
    submitted_texts: list[str] = []
    
    BINDINGS: ClassVar[list] = []
    
    def compose(self) -> ComposeResult:
        yield Input(placeholder="Type here", id="test_input")
    
    def on_input_submitted(self, event: Input.Submitted) -> None:
        text = event.value.strip()
        if not text:
            return
        TestInputHandler.submitted_texts.append(text)
        print(f"  HANDLER CALLED: text={text!r}")
        event.input.clear()


class TestApp(App):
    def compose(self) -> ComposeResult:
        yield TestInputHandler()


async def test():
    app = TestApp()
    async with app.run_test() as pilot:
        input_widget = app.query_one("#test_input", Input)
        input_widget.focus()
        await pilot.pause()
        
        # Type some text
        await pilot.press("h", "e", "l", "l", "o")
        await pilot.pause()
        
        print(f"Before Enter: submitted_texts={TestInputHandler.submitted_texts}")
        print(f"Input value: {input_widget.value!r}")
        
        # Press Enter
        await pilot.press("enter")
        await pilot.pause()
        
        print(f"After Enter: submitted_texts={TestInputHandler.submitted_texts}")
        print(f"Input value after submit: {input_widget.value!r}")
        
        assert len(TestInputHandler.submitted_texts) == 1, f"Expected 1 submission, got {TestInputHandler.submitted_texts}"
        assert TestInputHandler.submitted_texts[0] == "hello"
        
        print("\nTest PASSED: Input.Submitted was handled correctly")

if __name__ == "__main__":
    import asyncio
    asyncio.run(test())