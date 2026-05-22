"""Test LLM flow: conversation_manager + LLM call"""
import sys, asyncio, json
sys.path.insert(0, '.')

from my_tui.provider.provider import create_llm, ProviderConfig, ProviderType
from my_tui.conversation.conversation_manager import ConversationManager
from my_tui.conversation.context_compression import CompactionConfig
from langchain_core.messages import SystemMessage, HumanMessage


async def test_llm():
    settings = json.loads(open('setting.json', encoding='utf-8').read())
    config = ProviderConfig(
        provider_type=ProviderType.OPENAI_COMPATIBLE,
        base_url=settings['base_url'],
        api_key=settings['api_key'],
        model=settings['model'],
    )

    llm = create_llm(config)
    print(f'LLM created: {type(llm).__name__}, model={config.model}')

    conversations = ConversationManager(
        db_path='test_conversations.db',
        max_tokens=128000,
        compression_config=CompactionConfig(
            context_window=128000, compaction_threshold_ratio=0.7,
            micro_compact_tool_count=10, micro_compact_min_tokens=5000, hot_tail_size=3,
        ),
    )
    await conversations.create_session(model=config.model, title='Test')

    user_msg = HumanMessage(content='Hello, respond with exactly: Hi there!')
    await conversations.add_message(user_msg)
    await conversations.compress_context()

    msgs = [
        SystemMessage(content='You are a helpful assistant. Keep responses short.'),
        *conversations.get_messages(),
    ]

    print(f'Messages: {len(msgs)}')
    for i, m in enumerate(msgs):
        print(f'  [{i}] type={type(m).__name__}, content={str(m.content)[:80]!r}')

    print('\nCalling LLM astream...')
    collected = []
    try:
        async for chunk in llm.astream(msgs):
            if chunk.content:
                collected.append(chunk.content)
                print(f'  chunk: {str(chunk.content)[:60]!r}')
        full = ''.join(collected)
        print(f'\nFull response: {full!r}')
    except Exception as e:
        print(f'LLM error: {type(e).__name__}: {e}')
        import traceback
        traceback.print_exc()


asyncio.run(test_llm())