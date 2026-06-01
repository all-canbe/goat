import { useChatStore } from '@/stores/chatStore'
import { useAutoScroll } from '@/hooks/useAutoScroll'
import SearchBar from '@/components/search/SearchBar'
import UserMessage from './UserMessage'
import AssistantMessage from './AssistantMessage'
import SystemMessage from './SystemMessage'
import ErrorMessage from './ErrorMessage'
import StreamingBubble from './StreamingBubble'
import ToolCallCard from './ToolCallCard'

export default function MessageList() {
  const activeState = useChatStore((s) => s.getActiveState())
  const messages = activeState?.messages ?? []
  const streamingContent = activeState?.streamingContent ?? ''
  const isStreaming = activeState?.isStreaming ?? false
  const searchResults = useChatStore((s) => s.searchResults)
  const activeSearchIndex = useChatStore((s) => s.activeSearchIndex)
  const isSearchActive = useChatStore((s) => s.isSearchActive)
  const containerRef = useAutoScroll()

  return (
    <div
      ref={containerRef}
      className="flex-1 overflow-y-auto scrollbar-thin"
    >
      <SearchBar />
      {messages.length === 0 && !isStreaming && (
        <div className="flex items-center justify-center h-full text-text-darker text-sm">
          <p>输入消息开始对话...</p>
        </div>
      )}
      {messages.map((msg, idx) => {
        const isActiveMatch = isSearchActive && activeSearchIndex >= 0 && idx === searchResults[activeSearchIndex]
        return (
          <div key={msg.id} className={`${isActiveMatch ? 'bg-primary-dim' : ''} ${!isSearchActive ? 'animate-fade-in' : ''}`}>
            {msg.role === 'user' ? (
              <UserMessage content={msg.content} />
            ) : msg.role === 'assistant' ? (
              <>
                {msg.toolCalls?.map((tc, i) => (
                  <ToolCallCard key={i} toolCall={tc} />
                ))}
                <AssistantMessage content={msg.content} />
              </>
            ) : msg.role === 'system' ? (
              <SystemMessage content={msg.content} />
            ) : (
              <ErrorMessage content={msg.content} />
            )}
          </div>
        )
      })}
      {isStreaming && (
        <StreamingBubble content={streamingContent} />
      )}
    </div>
  )
}