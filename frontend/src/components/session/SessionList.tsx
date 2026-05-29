import { useSessionStore } from '@/stores/sessionStore'
import { useChatStore } from '@/stores/chatStore'
import SessionItem from './SessionItem'
import { useEffect } from 'react'

export default function SessionList() {
  const sessions = useSessionStore((s) => s.sessions)
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const loadSessions = useSessionStore((s) => s.loadSessions)
  const isLoading = useSessionStore((s) => s.isLoading)
  const chatSessions = useChatStore((s) => s.sessions)

  useEffect(() => {
    loadSessions()
  }, [])

  if (isLoading) {
    return (
      <div className="p-4 text-text-darker text-sm text-center">
        加载中...
      </div>
    )
  }

  if (sessions.length === 0) {
    return (
      <div className="p-4 text-text-darker text-sm text-center">
        暂无会话，点击 + 新建
      </div>
    )
  }

  return (
    <div className="py-1">
      {sessions.map((session) => {
        const cs = chatSessions[session.id]
        const isRunning = cs?.isStreaming ?? session.isRunning ?? false
        return (
          <SessionItem
            key={session.id}
            session={session}
            isActive={session.id === activeSessionId}
            isRunning={isRunning}
          />
        )
      })}
    </div>
  )
}