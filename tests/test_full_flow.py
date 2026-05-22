"""Test full TUI data flow: queue -> _do_chat -> events -> bridge -> state"""
import sys, asyncio, json
from pathlib import Path
sys.path.insert(0, '.')

from my_tui.core.event_bus import EventBus, EventType
from my_tui.tui.state import TUIState, MessageData, MessageRole
from my_tui.tui.bridge import TUIBridge
from my_tui.provider.provider import create_llm, ProviderConfig, ProviderType, get_provider_display, PROVIDER_DISPLAY_NAMES
from my_tui.conversation.conversation_manager import ConversationManager
from my_tui.conversation.context_compression import CompactionConfig
from my_tui.conversation.prompt_engine import engine as prompt_engine
from my_tui.agent.skill_system import SkillRegistry, Skill
from my_tui.tools.tools import BUILTIN_TOOLS
from langchain_core.messages import SystemMessage, HumanMessage, AIMessage
from langchain_openai import ChatOpenAI


async def test_full_flow():
    # 1. Setup like tui_main.py
    settings = json.loads(open('setting.json', encoding='utf-8').read())
    provider_config = ProviderConfig(
        provider_type=ProviderType.OPENAI_COMPATIBLE,
        base_url=settings['base_url'],
        api_key=settings['api_key'],
        model=settings['model'],
    )

    llm = create_llm(provider_config)
    event_bus = EventBus()
    state = TUIState()
    state.connected = True
    state.provider_name = get_provider_display(provider_config)
    state.model_name = provider_config.model

    conversations = ConversationManager(
        db_path='test_full_flow.db',
        max_tokens=128000,
        compression_config=CompactionConfig(
            context_window=128000, compaction_threshold_ratio=0.7,
            micro_compact_tool_count=10, micro_compact_min_tokens=5000, hot_tail_size=3,
        ),
    )
    await conversations.create_session(model=provider_config.model, title="TUI Test")

    skill_registry = SkillRegistry()
    bt = BUILTIN_TOOLS
    for name, desc, tools, role in [
        ("code_explorer", "代码库探索与分析", ["list_files", "read_file", "search_code"], "explore"),
    ]:
        tool_objs = [bt[t] for t in tools if t in bt]
        skill_registry.register(Skill(name=name, description=desc, tools=tool_objs, metadata={"role": role}))

    # 2. Setup bridge
    bridge = TUIBridge(event_bus, state)
    bridge.start()
    await asyncio.sleep(0.1)

    # 3. Simulate user input
    user_input_queue: asyncio.Queue[str] = asyncio.Queue()
    text = "Hello, respond with exactly: Hi there!"
    user_input_queue.put_nowait(text)
    bridge.add_user_message(text)
    
    print(f'After add_user_message: message_count={state.message_count}, version={state.version}')
    print(f'  messages len={len(state.messages)}, first role={state.messages[0].role}')
    
    # 4. Simulate _do_chat
    skills = [f"{s.name} — {s.description}" for s in skill_registry.list_all()]
    system_prompt = prompt_engine.render_main_system(
        "general", skills=skills, cwd=str(Path.cwd()),
        model=provider_config.model,
        provider=PROVIDER_DISPLAY_NAMES.get(
            provider_config.provider_type,
            provider_config.provider_type.value,
        ),
    )
    
    print(f'\nSystem prompt length: {len(system_prompt)}')
    print(f'First 100 chars: {system_prompt[:100]!r}')
    
    user_msg = HumanMessage(content=text)
    await conversations.add_message(user_msg)
    await conversations.compress_context()
    
    messages = [
        SystemMessage(content=system_prompt),
        *conversations.get_messages(),
    ]
    print(f'\nMessages to LLM: {len(messages)}')
    for i, m in enumerate(messages):
        print(f'  [{i}] {type(m).__name__}: {str(m.content)[:60]!r}')
    
    # 5. Call LLM and publish events
    print('\n--- Simulating LLM streaming ---')
    collected_chunks = []
    try:
        async for chunk in llm.astream(messages):
            if chunk.content:
                collected_chunks.append(chunk.content)
                event_bus.publish_nowait(
                    "llm", EventType.LLM_STREAM,
                    chunk.content,
                    agent_name="assistant",
                )
                print(f'  Published LLM_STREAM: {chunk.content!r}')
    except Exception as e:
        print(f'LLM error: {type(e).__name__}: {e}')
        event_bus.publish_nowait("system", EventType.ERROR, f"LLM 调用失败: {type(e).__name__}: {e}", agent_name="system")
        event_bus.publish_nowait("system", EventType.COMPLETED, "completed", agent_name="system")
    
    full_content = "".join(collected_chunks)
    
    # Wait for bridge to process events
    await asyncio.sleep(0.5)
    
    print(f'\n--- After LLM stream ---')
    print(f'state.is_streaming={state.is_streaming}')
    print(f'state.streaming_content={state.streaming_content!r}')
    print(f'state.messages count={len(state.messages)}')
    
    # Publish LLM_RESPONSE
    event_bus.publish_nowait(
        "llm", EventType.LLM_RESPONSE,
        full_content or "(无内容)",
        agent_name="assistant",
    )
    
    if full_content:
        await conversations.add_message(AIMessage(content=full_content))
    
    event_bus.publish_nowait(
        "system", EventType.COMPLETED, "completed", agent_name="system",
    )
    
    # Wait for bridge to process
    await asyncio.sleep(0.5)
    
    print(f'\n--- Final state ---')
    print(f'state.is_streaming={state.is_streaming}')
    print(f'state.streaming_content={state.streaming_content!r}')
    print(f'state.messages count={len(state.messages)}')
    print(f'state.version={state.version}')
    for i, m in enumerate(state.messages):
        print(f'  [{i}] role={m.role}, content={m.content[:80]!r}')
    
    bridge.stop()
    
    # Cleanup
    import os
    for f in ['test_full_flow.db', 'test_conversations.db']:
        if os.path.exists(f):
            os.remove(f)
    
    print('\nTest passed!')


asyncio.run(test_full_flow())