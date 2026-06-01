import { useState, useRef, useEffect } from 'react'
import { Wrench, ChevronDown, ChevronRight, Loader2, CheckCircle2, XCircle } from 'lucide-react'
import type { ToolCall } from '@/types'

interface ToolCallCardProps {
  toolCall: ToolCall
}

const RESULT_PREVIEW_HEIGHT = 320

export default function ToolCallCard({ toolCall }: ToolCallCardProps) {
  const [argsOpen, setArgsOpen] = useState(false)
  const [resultOpen, setResultOpen] = useState(false)
  const [resultFull, setResultFull] = useState(false)
  const [resultOverflows, setResultOverflows] = useState(false)
  const resultRef = useRef<HTMLPreElement>(null)

  useEffect(() => {
    if (resultOpen && resultRef.current) {
      setResultOverflows(resultRef.current.scrollHeight > RESULT_PREVIEW_HEIGHT + 10)
    }
  }, [resultOpen, toolCall.result])

  const statusIcons = {
    running: <Loader2 size={14} className="text-warning animate-spin" />,
    complete: <CheckCircle2 size={14} className="text-success" />,
    error: <XCircle size={14} className="text-error" />,
  }

  return (
    <div className="mx-4 my-1 border border-border rounded bg-surface-light overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 text-sm">
        {statusIcons[toolCall.status]}
        <Wrench size={14} className="text-text-dim" />
        <span className="text-primary-light font-medium">{toolCall.name}</span>
        <span className="text-xs text-text-darker capitalize">{toolCall.status}</span>
      </div>

      {Object.keys(toolCall.args).length > 0 && (
        <>
          <div
            onClick={() => setArgsOpen(!argsOpen)}
            className="flex items-center gap-1 px-3 py-1 text-xs text-text-dim cursor-pointer hover:text-text border-t border-border/50"
          >
            {argsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            参数
          </div>
          {argsOpen && (
            <pre className="px-6 py-2 text-xs text-text-dim bg-surface overflow-x-auto">
              {JSON.stringify(toolCall.args, null, 2)}
            </pre>
          )}
        </>
      )}

      {toolCall.result && (
        <>
          <div
            onClick={() => {
              setResultOpen(!resultOpen)
              if (resultOpen) setResultFull(false)
            }}
            className="flex items-center gap-1 px-3 py-1 text-xs text-text-dim cursor-pointer hover:text-text border-t border-border/50"
          >
            {resultOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            结果
          </div>
          {resultOpen && (
            <>
              <pre
                ref={resultRef}
                className={`px-6 py-2 text-xs text-text-dim bg-surface overflow-x-auto ${resultFull ? '' : 'overflow-y-auto'}`}
                style={resultFull ? {} : { maxHeight: RESULT_PREVIEW_HEIGHT }}
              >
                {toolCall.result}
              </pre>
              {resultOverflows && (
                <div
                  onClick={() => setResultFull(!resultFull)}
                  className="flex items-center justify-center gap-1 px-3 py-1.5 text-xs text-primary cursor-pointer hover:text-primary-light border-t border-border/50 bg-surface/80"
                >
                  {resultFull ? (
                    <>收起</>
                  ) : (
                    <>显示全部 ({Math.ceil(toolCall.result.length / 80)} 行)</>
                  )}
                </div>
              )}
            </>
          )}
        </>
      )}
    </div>
  )
}