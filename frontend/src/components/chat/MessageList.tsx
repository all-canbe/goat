import { useMemo, useEffect } from 'react'
import { Bot } from 'lucide-react'
import { useChatStore } from '@/stores/chatStore'
import { useAutoScroll } from '@/hooks/useAutoScroll'
import SearchBar from '@/components/search/SearchBar'
import UserMessage from './UserMessage'
import { AssistantMessageContent } from './AssistantMessage'
import SystemMessage from './SystemMessage'
import ErrorMessage from './ErrorMessage'
import GoatStreamBubble from './GoatStreamBubble'
import ToolCallCard from './ToolCallCard'
import type { Message, ToolCall } from '@/types'

interface AssistantGroup {
  key: string
  content: string
  toolCalls: ToolCall[]
  /** Indices of original messages in the `messages` array, used for search highlighting */
  messageIndices: number[]
  hasVisibleContent: boolean
}

type DisplayItem =
  | { kind: 'user' | 'system' | 'error'; key: string; message: Message; index: number }
  | { kind: 'assistant-group'; key: string; group: AssistantGroup }

export default function MessageList() {
  const activeState = useChatStore((s) => s.getActiveState())
  const messages = activeState?.messages ?? []
  const streamingContent = activeState?.streamingContent ?? ''
  const isStreaming = activeState?.isStreaming ?? false
  const pendingToolCalls = activeState?.pendingToolCalls ?? []
  const searchResults = useChatStore((s) => s.searchResults)
  const activeSearchIndex = useChatStore((s) => s.activeSearchIndex)
  const isSearchActive = useChatStore((s) => s.isSearchActive)
  const containerRef = useAutoScroll()

  // Group consecutive assistant messages into a single display unit
  const displayItems = useMemo(() => {
    const out: DisplayItem[] = []
    for (let idx = 0; idx < messages.length; idx++) {
      const msg = messages[idx]
      if (msg.role === 'tool') continue

      if (msg.role === 'assistant') {
        const last = out[out.length - 1]
        if (last && last.kind === 'assistant-group') {
          // Merge into the existing assistant group
          if (msg.content) {
            last.group.content = last.group.content
              ? last.group.content + '\n\n' + msg.content
              : msg.content
          }
          if (msg.toolCalls?.length) {
            last.group.toolCalls.push(...msg.toolCalls)
          }
          last.group.messageIndices.push(idx)
          if (msg.content || (msg.toolCalls && msg.toolCalls.length > 0)) {
            last.group.hasVisibleContent = true
          }
          continue
        }
        out.push({
          kind: 'assistant-group',
          key: msg.id,
          group: {
            key: msg.id,
            content: msg.content || '',
            toolCalls: msg.toolCalls ? [...msg.toolCalls] : [],
            messageIndices: [idx],
            hasVisibleContent: !!(msg.content || (msg.toolCalls && msg.toolCalls.length > 0)),
          },
        })
        continue
      }
      // user / system / error
      out.push({ kind: msg.role as 'user' | 'system' | 'error', key: msg.id, message: msg, index: idx })
    }
    // Drop empty assistant groups (no content and no tool calls)
    return out.filter((it) => it.kind !== 'assistant-group' || it.group.hasVisibleContent)
  }, [messages])

  // 兜底:最后一条 assistant 消息是最近 1.5 秒内提交的,说明后端已完成回复,
  // 即使 isStreaming 状态未及时更新,也不再显示等待气泡(避免"残留动画"假象)
  const lastMessage = messages[messages.length - 1]
  const recentAssistantCommit =
    lastMessage?.role === 'assistant' &&
    Date.now() - lastMessage.timestamp < 1500
  const shouldShowBubble =
    (isStreaming || activeState?.streamError) && !recentAssistantCommit

  // 兜底:若 isStreaming 仍为 true 但消息列表末尾已提交了 assistant 消息,
  // 主动调用 clearStream 修复状态错位(防止后端时序问题导致气泡残留)
  useEffect(() => {
    if (recentAssistantCommit && isStreaming && !activeState?.streamError) {
      const sid = useChatStore.getState().activeSessionId || 'default'
      useChatStore.getState().clearStream(sid)
    }
  }, [recentAssistantCommit, isStreaming, activeState?.streamError])

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
      {displayItems.map((item) => {
        const matchIdx = isSearchActive && activeSearchIndex >= 0 ? searchResults[activeSearchIndex] : -1
        const isActiveMatch = item.kind === 'assistant-group'
          ? item.group.messageIndices.includes(matchIdx)
          : item.index === matchIdx
        return (
          <div key={item.key} className={`${isActiveMatch ? 'bg-primary-dim' : ''} ${!isSearchActive ? 'animate-fade-in' : ''}`}>
            {item.kind === 'user' ? (
              <UserMessage content={item.message.content} attachments={item.message.attachments} />
            ) : item.kind === 'assistant-group' ? (
              <div className="flex gap-3 px-4 py-3 bg-surface/50">
                <div className="w-6 h-6 rounded bg-primary-dim/50 flex items-center justify-center flex-shrink-0 mt-0.5">
                  <Bot size={14} className="text-primary" />
                </div>
                <div className="flex-1 min-w-0 flex flex-col gap-2">
                  {item.group.toolCalls.map((tc, i) => (
                    <ToolCallCard key={`${item.key}-tc-${i}`} toolCall={tc} />
                  ))}
                  {item.group.content && <AssistantMessageContent content={item.group.content} />}
                </div>
              </div>
            ) : item.kind === 'system' ? (
              <SystemMessage content={item.message.content} />
            ) : (
              <ErrorMessage content={item.message.content} />
            )}
          </div>
        )
      })}
      {shouldShowBubble && (
        <GoatStreamBubble
          status={activeState?.streamError ? 'error' : streamingContent ? 'streaming' : 'thinking'}
          content={streamingContent}
          error={activeState?.streamError || undefined}
          pendingToolCalls={pendingToolCalls}
          onRetry={() => (window as any).__retryLast?.()}
          onCancel={() => {
            const sid = useChatStore.getState().activeSessionId || 'default'
            useChatStore.getState().clearStreamError(sid)
            useChatStore.getState().clearStream(sid)
            const wsClient = (window as any).__wsClient
            if (wsClient) wsClient.send('chat.cancel', { sessionId: sid })
          }}
        />
      )}
    </div>
  )
}