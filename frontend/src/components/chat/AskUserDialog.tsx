import { useState, useRef, KeyboardEvent, useEffect } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import { Send, MessageCircle, X } from 'lucide-react'
import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'

export default function AskUserDialog() {
  const activeState = useChatStore((s) => s.getActiveState())
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const pendingQuestion = activeState?.pendingQuestion ?? null
  const [answer, setAnswer] = useState('')
  const submittedRef = useRef(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    if (pendingQuestion) {
      setAnswer('')
      submittedRef.current = false
      setTimeout(() => textareaRef.current?.focus(), 0)
    }
  }, [pendingQuestion?.questionId])

  if (!pendingQuestion) return null

  const handleSubmit = () => {
    if (submittedRef.current) return
    const trimmed = answer.trim()
    if (!trimmed) return

    submittedRef.current = true
    const ws = (window as any).__wsClient
    if (ws) {
      ws.send('ask_user.answer', {
        questionId: pendingQuestion.questionId,
        answer: trimmed,
      })
    }
    setAnswer('')
    useChatStore.getState().setPendingQuestion(activeSessionId || 'default', null)
    submittedRef.current = false
  }

  const handleDismiss = () => {
    if (submittedRef.current) return
    submittedRef.current = true
    useChatStore.getState().setPendingQuestion(activeSessionId || 'default', null)
    submittedRef.current = false
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSubmit()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      handleDismiss()
    }
  }

  return (
    <div className="fixed bottom-[72px] left-1/2 -translate-x-1/2 z-40 w-[640px] max-w-[calc(100vw-32px)] animate-fade-in">
      <div className="bg-surface border border-border rounded-lg shadow-2xl flex flex-col overflow-hidden">
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-border bg-surface-light/40 flex-shrink-0">
          <div className="flex items-center gap-2">
            <MessageCircle size={14} className="text-primary" />
            <span className="text-text text-xs font-medium">Agent 提问</span>
          </div>
          <button
            onClick={handleDismiss}
            className="text-text-darker hover:text-text transition-colors p-0.5"
            title="跳过 / 关闭 (Esc)"
          >
            <X size={14} />
          </button>
        </div>

        <div className="px-4 py-3 max-h-[40vh] overflow-y-auto scrollbar-thin">
          <div className="prose prose-sm max-w-none">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
              components={{
                code({ className, children, ...props }: any) {
                  const isInline = !className
                  if (isInline) {
                    return <code className="bg-surface-light px-1 py-0.5 rounded text-sm" {...props}>{children}</code>
                  }
                  return <pre className="bg-surface-light rounded p-2 overflow-x-auto"><code className={className} {...props}>{children}</code></pre>
                },
              }}
            >{pendingQuestion.question}</ReactMarkdown>
          </div>
        </div>

        <div className="px-3 py-2.5 border-t border-border flex-shrink-0 bg-surface">
          <div className="flex gap-2 items-stretch">
            <textarea
              ref={textareaRef}
              value={answer}
              onChange={(e) => setAnswer(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入你的回答... (Enter 发送, Esc 跳过)"
              rows={2}
              className="flex-1 bg-surface-light border border-border rounded px-3 py-2 text-sm text-text placeholder-text-darker resize-none focus:outline-none focus:border-border-focus transition-colors scrollbar-thin"
              style={{ maxHeight: '120px' }}
            />
            <button
              onClick={handleSubmit}
              disabled={!answer.trim()}
              className="p-2 rounded bg-primary text-white hover:bg-primary-light transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              title="提交回答 (Enter)"
            >
              <Send size={16} />
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
