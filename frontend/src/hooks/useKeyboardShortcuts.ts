import { useEffect } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'

export function useKeyboardShortcuts() {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === 't') {
        e.preventDefault()
        useSessionStore.getState().createSession()
      }
      if (e.ctrlKey && e.key === 'c') {
        const chatStore = useChatStore.getState()
        const sessionStore = useSessionStore.getState()
        const sid = sessionStore.activeSessionId || 'default'
        const activeState = chatStore.sessions[sid]
        if (activeState?.isStreaming) {
          e.preventDefault()
          const ws = (window as any).__wsClient
          ws?.send('chat.cancel', {})
          chatStore.clearStream(sid)
        }
      }
      if (e.key === 'y' || e.key === 'Y') {
        const chatStore = useChatStore.getState()
        const sessionStore = useSessionStore.getState()
        const sid = sessionStore.activeSessionId || 'default'
        const activeState = chatStore.sessions[sid]
        if (activeState?.pendingApproval) {
          const ws = (window as any).__wsClient
          ws?.send('tool.approve', { toolCallId: activeState.pendingApproval.toolCallId })
          chatStore.setPendingApproval(sid, null)
        }
      }
      if (e.key === 'n' || e.key === 'N') {
        const chatStore = useChatStore.getState()
        const sessionStore = useSessionStore.getState()
        const sid = sessionStore.activeSessionId || 'default'
        const activeState = chatStore.sessions[sid]
        if (activeState?.pendingApproval) {
          const ws = (window as any).__wsClient
          ws?.send('tool.reject', { toolCallId: activeState.pendingApproval.toolCallId })
          chatStore.setPendingApproval(sid, null)
        }
      }
      if (e.key === 'F3') {
        const state = useChatStore.getState()
        if (state.isSearchActive) {
          e.preventDefault()
          state.navigateSearch(e.shiftKey ? 'prev' : 'next')
        }
      }
    }
    document.addEventListener('keydown', handler)
    return () => document.removeEventListener('keydown', handler)
  }, [])
}