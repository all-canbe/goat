import { useRef, useEffect } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Bot, AlertCircle, RotateCcw, Square } from 'lucide-react'
import ToolCallCard from './ToolCallCard'
import type { ToolCall } from '@/types'

type BubbleStatus = 'thinking' | 'streaming' | 'error'

interface GoatStreamBubbleProps {
  status: BubbleStatus
  content?: string
  error?: string
  pendingToolCalls?: ToolCall[]
  onRetry?: () => void
  onCancel?: () => void
}

function HoofprintDots() {
  return (
    <div className="flex gap-2 items-center py-2">
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className="hoofprint-dot"
          style={{ animationDelay: `${i * 250}ms` }}
        />
      ))}
      <span className="text-text-dim text-xs ml-2">思考中...</span>
    </div>
  )
}

function ScanLine() {
  return <div className="scan-line" />
}

function StatusIndicator({ status }: { status: BubbleStatus }) {
  if (status === 'streaming') {
    return (
      <span className="inline-flex items-center gap-1 ml-1">
        <span className="hoofprint-mini" />
        <span className="inline-block w-2 h-4 bg-primary-light animate-pulse align-bottom" />
      </span>
    )
  }
  return null
}

export default function GoatStreamBubble({ status, content, error, pendingToolCalls, onRetry, onCancel }: GoatStreamBubbleProps) {
  const containerRef = useRef<HTMLDivElement>(null)

  // 自动滚动
  useEffect(() => {
    if (containerRef.current) {
      const el = containerRef.current.closest('.overflow-y-auto')
      if (el) el.scrollTop = el.scrollHeight
    }
  }, [content])

  if (status === 'error') {
    return (
      <div className="flex gap-3 px-4 py-3">
        <div className="w-6 h-6 rounded bg-error/30 flex items-center justify-center flex-shrink-0 mt-0.5">
          <AlertCircle size={14} className="text-error" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="bg-error/10 border border-error/30 rounded p-3">
            <p className="text-error text-sm font-medium mb-1">出错了</p>
            <p className="text-error/80 text-xs whitespace-pre-wrap break-words mb-3">{error || '未知错误'}</p>
            <div className="flex gap-2">
              {onRetry && (
                <button
                  onClick={onRetry}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs bg-error/20 text-error rounded hover:bg-error/30 transition-colors"
                >
                  <RotateCcw size={12} />
                  重试
                </button>
              )}
              {onCancel && (
                <button
                  onClick={onCancel}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs bg-surface-light text-text-dim rounded hover:text-text transition-colors"
                >
                  <Square size={12} />
                  取消
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div ref={containerRef} className="flex gap-3 px-4 py-3 bg-surface/50 relative overflow-hidden">
      <div className={`w-6 h-6 rounded flex items-center justify-center flex-shrink-0 mt-0.5 ${
        status === 'thinking'
          ? 'bg-primary-dim/50 glow-ring-thinking'
          : 'bg-primary-dim/50 glow-ring-streaming'
      }`}>
        <Bot size={14} className={`${status === 'thinking' ? 'text-primary animate-pulse' : 'text-primary'}`} />
      </div>
      <div className="flex-1 min-w-0">
        {content ? (
          <>
            <div className="prose prose-invert prose-sm max-w-none">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
              <StatusIndicator status={status} />
            </div>
            {pendingToolCalls && pendingToolCalls.length > 0 && (
              <div className="mt-2 border-t border-border/30 pt-2 space-y-1">
                {pendingToolCalls.map((tc, i) => (
                  <ToolCallCard key={`pending-tc-${i}`} toolCall={tc} />
                ))}
              </div>
            )}
          </>
        ) : (
          <HoofprintDots />
        )}
      </div>
      {status === 'thinking' && <ScanLine />}

      <style>{`
        @keyframes hoofprint-wave {
          0%, 60%, 100% { opacity: 0.2; transform: scale(0.8) rotate(45deg); }
          30% { opacity: 1; transform: scale(1.1) rotate(45deg); }
        }
        .hoofprint-dot {
          display: inline-block;
          width: 8px;
          height: 8px;
          border-radius: 2px;
          background: var(--color-primary);
          opacity: 0.2;
          transform: rotate(45deg);
          animation: hoofprint-wave 1.2s ease-in-out infinite;
        }

        @keyframes glow-pulse-thinking {
          0%, 100% { box-shadow: 0 0 4px 0px var(--color-primary-dim); }
          50% { box-shadow: 0 0 14px 4px var(--color-primary-glow); }
        }
        @keyframes glow-pulse-streaming {
          0%, 100% { box-shadow: 0 0 2px 0px var(--color-primary-dim); }
          50% { box-shadow: 0 0 8px 2px var(--color-primary); }
        }
        .glow-ring-thinking {
          animation: glow-pulse-thinking 1.8s ease-in-out infinite;
        }
        .glow-ring-streaming {
          animation: glow-pulse-streaming 2.5s ease-in-out infinite;
        }

        @keyframes scan-line-move {
          0% { transform: translateX(-100%); }
          100% { transform: translateX(100%); }
        }
        .scan-line {
          position: absolute;
          bottom: 0;
          left: 0;
          width: 60%;
          height: 1px;
          background: linear-gradient(90deg, transparent, var(--color-primary-dim), transparent);
          animation: scan-line-move 2.5s ease-in-out infinite;
        }

        .hoofprint-mini {
          display: inline-block;
          width: 6px;
          height: 6px;
          border-radius: 1.5px;
          background: var(--color-primary-light);
          transform: rotate(45deg);
          opacity: 0.7;
          animation: hoofprint-wave 1.2s ease-in-out infinite;
        }
      `}</style>
    </div>
  )
}