import { useState, useEffect, useRef } from 'react'
import { Search } from 'lucide-react'
import { useSkillStore } from '@/stores/skillStore'
import { useSessionStore } from '@/stores/sessionStore'

export default function SkillInstallDialog() {
  const { phase, cancelFlow } = useSkillStore()
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const [query, setQuery] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (phase === 'input') {
      setQuery('')
      setTimeout(() => inputRef.current?.focus(), 0)
    }
  }, [phase])

  if (phase !== 'input') return null

  const handleSubmit = () => {
    const q = query.trim()
    if (!q) return

    const prompt = `请使用 find-skills 工具搜索与「${q}」相关的可用 AI 辅助编程 skill（技能/工具插件）。\n请列出每个 skill 的：名称、描述、安装 URL。`

    const ws = (window as any).__wsClient as import('@/lib/ws-client').WSClient | null
    if (ws && activeSessionId) {
      ws.send('chat.send', {
        sessionId: activeSessionId,
        text: prompt,
      })
    }
    cancelFlow()
  }

  return (
    <div className="border-t border-border bg-surface px-4 py-3">
      <div className="flex items-center gap-2">
        <Search size={14} className="text-text-dimmer flex-shrink-0" />
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') handleSubmit()
            if (e.key === 'Escape') cancelFlow()
          }}
          placeholder="输入要查找的 skill（按 Enter 发送，Esc 取消）"
          className="flex-1 bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text placeholder-text-darker outline-none focus:border-border-focus transition-colors"
        />
        <button
          onClick={handleSubmit}
          disabled={!query.trim()}
          className="px-3 py-1.5 rounded text-xs bg-primary text-white hover:bg-primary-light transition-colors disabled:opacity-50"
        >
          发送
        </button>
        <button
          onClick={cancelFlow}
          className="px-2 py-1.5 rounded text-xs text-text-dimmer hover:text-text transition-colors"
        >
          取消
        </button>
      </div>
    </div>
  )
}