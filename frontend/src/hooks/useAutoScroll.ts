import { useEffect, useRef } from 'react'
import { useChatStore } from '@/stores/chatStore'

export function useAutoScroll() {
  const containerRef = useRef<HTMLDivElement>(null)
  const activeState = useChatStore((s) => s.getActiveState())
  const messages = activeState?.messages ?? []
  const streamingContent = activeState?.streamingContent ?? ''

  useEffect(() => {
    const el = containerRef.current
    if (el) {
      el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
    }
  }, [messages, streamingContent])

  return containerRef
}