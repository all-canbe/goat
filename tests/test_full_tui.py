"""
Test FULL flow including _process_input_loop with mock LLM
"""
import asyncio, sys
sys.path.insert(0, '.')
from pathlib import Path
from textual.app import App
from my_tui.tui.state import TUIState
from my_tui.tui.bridge import TUIBridge
from my_tui.core.event_bus import EventBus, EventType
from my_tui.tui.app import TuiScreen
from my_tui.conversation.conversation_manager import ConversationManager
from my_tui.conversation.context_compression import CompactionConfig
from my_tui.conversation.prompt_engine import engine as prompt_engine
from my_tui.agent.skill_system import SkillRegistry, Skill
from my_tui.tools.tools import BUILTIN_TOOLS
from my_tui.provider.provider import ProviderConfig, ProviderType, PROVIDER_DISPLAY_NAMES
from langchain_core.messages import SystemMessage, AIMessage
from langchain_core.language_models.chat_models import BaseChatModel


class MockLLM(BaseChatModel):
    """Mock LLM that returns a simple response"""
    
    def _generate(self, messages, stop=None, run_manager=None, **kwargs):
        from langchain_core.messages import AIMessageChunk
        from langchain_core.outputs import ChatResult, ChatGeneration
        return ChatResult(generations=[ChatGeneration(message=AIMessage(content="Mock response"))])
    
    async def _astream(self, messages, stop=None, run_manager=None, **kwargs):
        from langchain_core.messages import AIMessageChunk
        yield AIMessageChunk(content="Mock ")
        yield AIMessageChunk(content="response")
    
    def _llm_type(self):
        return "mock"
    
    @property
    def _identifying_params(self):
        return {}


async def _process_input_loop(user_input_queue, llm, event_bus, state, conversations, skill_registry, provider_config):
    """Simplified version for testing"""
    while True:
        try:
            text = await asyncio.wait_for(user_input_queue.get(), timeout=0.5)
        except asyncio.TimeoutError:
            continue
        except asyncio.CancelledError:
            break

        try:
            await _do_chat(text, llm, event_bus, conversations, skill_registry, provider_config)
        except asyncio.CancelledError:
            break
        except Exception as e:
            print(f"ERROR in process: {type(e).__name__}: {e}")
            import traceback
            traceback.print_exc()
            event_bus.publish_nowait("system", EventType.ERROR, f"处理失败: {type(e).__name__}: {e}", agent_name="system")
            event_bus.publish_nowait("system", EventType.COMPLETED, "completed", agent_name="system")


async def _do_chat(message, llm, event_bus, conversations, skill_registry, provider_config):
    skills = [f"{s.name} — {s.description}" for s in skill_registry.list_all()]
    system_prompt = prompt_engine.render_main_system(
        "general", skills=skills, cwd=str(Path.cwd()),
        model=provider_config.model,
        provider=PROVIDER_DISPLAY_NAMES.get(provider_config.provider_type, provider_config.provider_type.value),
    )
    
    from langchain_core.messages import HumanMessage
    user_msg = HumanMessage(content=message)
    await conversations.add_message(user_msg)
    await conversations.compress_context()
    
    messages = [SystemMessage(content=system_prompt), *conversations.get_messages()]
    print(f"  _do_chat: {len(messages)} messages to LLM")
    
    collected_chunks = []
    try:
        async for chunk in llm._astream(messages):
            if chunk.content:
                collected_chunks.append(chunk.content)
                event_bus.publish_nowait("llm", EventType.LLM_STREAM, chunk.content, agent_name="assistant")
                print(f"  LLM_STREAM: {chunk.content!r}")
    except Exception as e:
        print(f"  LLM error: {type(e).__name__}: {e}")
        event_bus.publish_nowait("system", EventType.ERROR, f"LLM 调用失败: {type(e).__name__}: {e}", agent_name="system")
        event_bus.publish_nowait("system", EventType.COMPLETED, "completed", agent_name="system")
        return
    
    full_content = "".join(collected_chunks)
    event_bus.publish_nowait("llm", EventType.LLM_RESPONSE, full_content or "(无内容)", agent_name="assistant")
    
    if full_content:
        await conversations.add_message(AIMessage(content=full_content))
    event_bus.publish_nowait("system", EventType.COMPLETED, "completed", agent_name="system")


class TestFullApp(App):
    def __init__(self):
        super().__init__()
        self.event_bus = EventBus()
        self.state = TUIState()
        self.state.connected = True
        self.state.provider_name = "Test Provider"
        self.state.model_name = "test-model"
        self.bridge = TUIBridge(self.event_bus, self.state)
        self.bridge.start()
        self.user_input_queue: asyncio.Queue[str] = asyncio.Queue()

    def on_mount(self):
        screen = TuiScreen(self.event_bus, self.state, self.bridge, user_input_queue=self.user_input_queue)
        self.push_screen(screen)


async def test():
    # Setup
    provider_config = ProviderConfig(
        provider_type=ProviderType.OPENAI_COMPATIBLE,
        base_url="http://test",
        api_key="test",
        model="test-model",
    )
    
    conversations = ConversationManager(
        db_path="test_full_flow.db",
        max_tokens=128000,
        compression_config=CompactionConfig(context_window=128000, compaction_threshold_ratio=0.7,
                                            micro_compact_tool_count=10, micro_compact_min_tokens=5000, hot_tail_size=3))
    await conversations.create_session(model="test-model", title="TUI Test")
    
    skill_registry = SkillRegistry()
    bt = BUILTIN_TOOLS
    tool_objs = [bt[t] for t in ["list_files", "read_file"] if t in bt]
    skill_registry.register(Skill(name="code_explorer", description="代码探索", tools=tool_objs, metadata={"role": "explore"}))
    
    llm = MockLLM()
    event_bus = EventBus()  # Separate from TUI's event_bus
    
    # Create TUI app with SEPARATE event_bus (like real flow)
    app = TestFullApp()
    
    # Create processing task
    processing_task = asyncio.create_task(
        _process_input_loop(
            user_input_queue=app.user_input_queue,
            llm=llm,
            event_bus=app.event_bus,  # Use TUI's event bus
            state=app.state,
            conversations=conversations,
            skill_registry=skill_registry,
            provider_config=provider_config,
        )
    )
    
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause()
        screen = app.screen
        chat_input = screen.query_one("#chat_input")
        
        print(f"Initial: queue_size={app.user_input_queue.qsize()}, msg_count={app.state.message_count}")
        
        # Type and submit via pilot
        await pilot.press("h", "e", "l", "l", "o")
        await pilot.pause()
        await pilot.press("enter")
        
        # Wait for processing
        await asyncio.sleep(1.0)
        await pilot.pause()
        
        print(f"\nAfter submit:")
        print(f"  queue_size={app.user_input_queue.qsize()}")
        print(f"  msg_count={app.state.message_count}")
        print(f"  messages: {len(app.state.messages)}")
        print(f"  version: {app.state.version}")
        print(f"  is_streaming: {app.state.is_streaming}")
        print(f"  streaming_content: {app.state.streaming_content!r}")
        
        for i, m in enumerate(app.state.messages):
            print(f"  msg[{i}]: role={m.role}, content={m.content[:60]!r}")
        
        if len(app.state.messages) >= 2:
            print("\nTest PASSED: Both user message and assistant response received!")
        else:
            print(f"\nTest FAILED: Expected at least 2 messages, got {len(app.state.messages)}")
    
    processing_task.cancel()
    try:
        await processing_task
    except asyncio.CancelledError:
        pass
    
    import os
    for f in ["test_full_flow.db"]:
        if os.path.exists(f):
            try:
                os.remove(f)
            except:
                pass


if __name__ == "__main__":
    asyncio.run(test())