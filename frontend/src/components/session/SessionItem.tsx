import { MessageSquare, Trash2 } from 'lucide-react'
import type { Session } from '@/types'
import { useSessionStore } from '@/stores/sessionStore'

interface SessionItemProps {
  session: Session
  isActive: boolean
  isRunning: boolean
}

export default function SessionItem({ session, isActive, isRunning }: SessionItemProps) {
  const setActiveSession = useSessionStore((s) => s.setActiveSession)
  const deleteSession = useSessionStore((s) => s.deleteSession)

  return (
    <div
      onClick={() => setActiveSession(session.id)}
      className={`group flex items-center gap-2 px-3 py-2 cursor-pointer text-sm transition-colors ${
        isActive
          ? 'bg-selection-bg text-selection-fg border-l-2 border-primary'
          : 'text-text-dim hover:bg-surface-light border-l-2 border-transparent'
      }`}
    >
      <MessageSquare size={14} className="flex-shrink-0" />
      <span className="flex-1 truncate">{session.title}</span>
      {isRunning && (
        <span className="w-2 h-2 rounded-full bg-green-500 animate-pulse flex-shrink-0" title="运行中" />
      )}
      <button
        onClick={(e) => {
          e.stopPropagation()
          deleteSession(session.id)
        }}
        className="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-surface-lighter text-text-darker hover:text-error transition-all"
        title="删除会话"
      >
        <Trash2 size={12} />
      </button>
    </div>
  )
}