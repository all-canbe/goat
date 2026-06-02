import { useState, useRef, useEffect, KeyboardEvent } from 'react'
import { Send, Square, File, X } from 'lucide-react'
import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'

interface FileAttachment {
  path: string
  name: string
}

export default function InputPanel() {
  const [text, setText] = useState('')
  const [attachments, setAttachments] = useState<FileAttachment[]>([])
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const triggeredRef = useRef(false)
  const textRef = useRef('')
  const activeState = useChatStore((s) => s.getActiveState())
  const isStreaming = activeState?.isStreaming ?? false
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const pendingQuestion = activeState?.pendingQuestion ?? null

  const handleSubmit = async () => {
    const sessionId = activeSessionId || 'default'
    if (pendingQuestion) {
      const trimmed = text.trim()
      if (!trimmed) return

      const wsClient = (window as any).__wsClient
      if (wsClient) {
        wsClient.send('ask_user.answer', {
          questionId: pendingQuestion.questionId,
          answer: trimmed,
        })
      }
      useChatStore.getState().setPendingQuestion(sessionId, null)
      setText('')
      textareaRef.current?.focus()
      return
    }

    const trimmed = text.trim()
    if (!trimmed && attachments.length === 0) return
    if (isStreaming) return

    const sStore = useSessionStore.getState()
    if (!sStore.activeSessionId) {
      await sStore.createSession()
    }
    const realSessionId = useSessionStore.getState().activeSessionId ?? sessionId

    let finalText = trimmed
    if (attachments.length > 0) {
      const filesPart = attachments.map(a => `📄 \`${a.path}\``).join('\n')
      finalText = trimmed ? `${trimmed}\n${filesPart}` : filesPart
    }

    const store = useChatStore.getState()
    store.clearStream(realSessionId)
    store.addMessage(realSessionId, {
      id: crypto.randomUUID(),
      role: 'user',
      content: finalText,
      timestamp: Date.now(),
    })
    store.appendStreamToken(realSessionId, '')

    const wsClient = (window as any).__wsClient
    if (wsClient) {
      wsClient.send('chat.send', { text: finalText, sessionId: realSessionId })
    } else {
      store.addMessage(realSessionId, {
        id: crypto.randomUUID(),
        role: 'error',
        content: 'WebSocket 连接未就绪，请刷新页面重试',
        timestamp: Date.now(),
      })
      store.clearStream(realSessionId)
    }

    setText('')
    setAttachments([])
    textareaRef.current?.focus()
  }

  useEffect(() => {
    const win = window as any
    win.__clearInput = () => {
      setText('')
      textRef.current = ''
      triggeredRef.current = false
    }
    win.__commandPaletteClose = () => {
      if (!textRef.current.startsWith('/')) {
        triggeredRef.current = false
      }
    }
    win.__addFileToInput = (path: string, name: string) => {
      setAttachments(prev => [...prev, { path, name }])
    }
    return () => {
      delete win.__clearInput
      delete win.__commandPaletteClose
      delete win.__addFileToInput
    }
  }, [])

  const handleCancel = () => {
    const sessionId = activeSessionId || 'default'
    const wsClient = (window as any).__wsClient
    if (wsClient) wsClient.send('chat.cancel', { sessionId })
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (isStreaming) {
        handleCancel()
      } else {
        handleSubmit()
      }
    }
  }

  return (
    <div className="border-t border-border bg-surface p-3">
      {attachments.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-2">
          {attachments.map((att) => (
            <span
              key={att.path}
              className="inline-flex items-center gap-1 px-2 py-1 bg-emerald-500/10 text-emerald-400 rounded text-xs"
            >
              <File size={12} />
              <span className="max-w-[120px] truncate">{att.name}</span>
              <button
                onClick={() => setAttachments((prev) => prev.filter((a) => a.path !== att.path))}
                className="hover:text-emerald-200 transition-colors"
              >
                <X size={12} />
              </button>
            </span>
          ))}
        </div>
      )}
      <div className="flex gap-2 items-stretch">
        <textarea
          ref={textareaRef}
          value={text}
          onChange={(e) => {
            const val = e.target.value
            setText(val)
            textRef.current = val
            if (val.startsWith('/') && !triggeredRef.current) {
              triggeredRef.current = true
              ;(window as any).__commandPaletteOpen?.(val)
            }
            if (!val.startsWith('/')) {
              triggeredRef.current = false
            }
          }}
          onKeyDown={handleKeyDown}
          placeholder={pendingQuestion ? "回答 Agent 的提问..." : "输入消息... (Shift+Enter 换行, Enter 发送)"}
          rows={1}
          className="flex-1 bg-surface-light border border-border rounded px-3 py-2 text-sm text-text placeholder-text-darker resize-none focus:outline-none focus:border-border-focus transition-colors scrollbar-thin"
          style={{ maxHeight: '120px' }}
        />
        <button
          onClick={isStreaming ? handleCancel : handleSubmit}
          className={`p-2 rounded transition-colors ${
            isStreaming
              ? 'bg-error/20 text-error hover:bg-error/30'
              : 'bg-primary text-white hover:bg-primary-light'
          }`}
          title={isStreaming ? '取消生成' : '发送'}
        >
          {isStreaming ? <Square size={16} /> : <Send size={16} />}
        </button>
      </div>
    </div>
  )
}