"""
Test with actual TuiScreen from the codebase - let real handler run
"""
import asyncio
from textual.app import App
from tui_legacy.tui.state import TUIState
from tui_legacy.tui.bridge import TUIBridge
from goat.core.event_bus import EventBus
from tui_legacy.tui.app import TuiScreen


class TestApp(App):
    def __init__(self):
        super().__init__()
        self.event_bus = EventBus()
        self.state = TUIState()
        self.bridge = TUIBridge(self.event_bus, self.state)
        self.user_input_queue: asyncio.Queue[str] = asyncio.Queue()

    def on_mount(self):
        screen = TuiScreen(
            self.event_bus, self.state, self.bridge,
            user_input_queue=self.user_input_queue,
        )
        self.push_screen(screen)


async def test():
    app = TestApp()
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause()
        
        screen = app.screen
        
        chat_input = screen.query_one("#chat_input")
        print(f"Input focused: {chat_input.has_focus}")
        
        # Type something
        await pilot.press("h", "e", "l", "l", "o")
        await pilot.pause()
        
        print(f"\nBefore Enter:")
        print(f"  Input value: {chat_input.value!r}")
        print(f"  Queue size: {app.user_input_queue.qsize()}")
        print(f"  State msg_count: {app.state.message_count}")
        
        # Press Enter
        await pilot.press("enter")
        await pilot.pause()
        
        print(f"\nAfter Enter:")
        print(f"  Input value: {chat_input.value!r}")
        print(f"  Queue size: {app.user_input_queue.qsize()}")
        print(f"  State message_count: {app.state.message_count}")
        print(f"  State messages: {len(app.state.messages)}")
        if app.state.messages:
            m = app.state.messages[0]
            print(f"  First msg: role={m.role}, content={m.content[:50]!r}")
        
        if app.user_input_queue.qsize() > 0:
            queued = app.user_input_queue.get_nowait()
            print(f"\nQueue content: {queued!r}")
            
            if queued == "hello" and app.state.message_count == 1:
                print("\nTest PASSED: Handler called & queue populated correctly!")
            else:
                print(f"\nTest PARTIAL: queue={queued!r}, msg_count={app.state.message_count}")
        else:
            print(f"\nTest FAILED: Queue is empty - handler was not called!")
            print(f"  state.version={app.state.version}")
            print(f"  state.message_count={app.state.message_count}")
            
            # Manual test: directly invoke handler
            print("\nTrying manual test:")
            screen._user_input_queue.put_nowait("manual_test")
            screen._bridge.add_user_message("manual_test")
            print(f"  After manual: queue_size={app.user_input_queue.qsize()}")
            print(f"  After manual: msg_count={app.state.message_count}")
            
            # Check if Input.Submitted bubbling works
            panel = screen.query_one("#input_panel")
            from textual.widgets import Input
            print(f"\n  Input.Submitted.handler_name: {Input.Submitted.handler_name}")
            handler = getattr(type(panel), 'on_input_submitted', None)
            print(f"  InputPanel class has on_input_submitted: {handler is not None}")
            # Check TuiScreen
            screen_cls = type(screen)
            screen_handler = getattr(screen_cls, 'on_input_panel_submitted', None)
            print(f"  TuiScreen class has on_input_panel_submitted: {screen_handler is not None}")


if __name__ == "__main__":
    asyncio.run(test())