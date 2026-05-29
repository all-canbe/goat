import { useState, useRef, KeyboardEvent } from 'react'
import ReactMarkdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import { Send, MessageCircle } from 'lucide-react'
import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'

export default function AskUserDialog() {
  const activeState = useChatStore((s) => s.getActiveState())
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const pendingQuestion = activeState?.pendingQuestion ?? null
  const [answer, setAnswer] = useState('')
  const submittedRef = useRef(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

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

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSubmit()
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg w-[520px] max-w-[90vw] max-h-[80vh] shadow-2xl flex flex-col">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border flex-shrink-0">
          <MessageCircle size={16} className="text-primary" />
          <span className="text-text text-sm font-medium">Agent 提问</span>
        </div>

        <div className="px-4 py-3 overflow-y-auto flex-1">
          <div className="prose prose-invert prose-sm max-w-none">
            <ReactMarkdown
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

        <div className="px-4 py-3 border-t border-border flex-shrink-0">
          <div className="flex gap-2 items-stretch">
            <textarea
              ref={textareaRef}
              value={answer}
              onChange={(e) => setAnswer(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入你的回答..."
              rows={2}
              className="flex-1 bg-surface-light border border-border rounded px-3 py-2 text-sm text-text placeholder-text-darker resize-none focus:outline-none focus:border-border-focus transition-colors scrollbar-thin"
            />
            <button
              onClick={handleSubmit}
              disabled={!answer.trim()}
              className="p-2 rounded bg-primary text-white hover:bg-primary-light transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              title="提交回答"
            >
              <Send size={16} />
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}