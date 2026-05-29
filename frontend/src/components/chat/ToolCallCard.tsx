import { useState } from 'react'
import { Wrench, ChevronDown, ChevronRight, Loader2, CheckCircle2, XCircle } from 'lucide-react'
import type { ToolCall } from '@/types'

interface ToolCallCardProps {
  toolCall: ToolCall
}

export default function ToolCallCard({ toolCall }: ToolCallCardProps) {
  const [argsOpen, setArgsOpen] = useState(false)
  const [resultOpen, setResultOpen] = useState(false)

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

      {toolCall.result && (
        <>
          <div
            onClick={() => setResultOpen(!resultOpen)}
            className="flex items-center gap-1 px-3 py-1 text-xs text-text-dim cursor-pointer hover:text-text border-t border-border/50"
          >
            {resultOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            结果
          </div>
          {resultOpen && (
            <pre className="px-6 py-2 text-xs text-text-dim bg-surface overflow-x-auto max-h-40 overflow-y-auto">
              {toolCall.result}
            </pre>
          )}
        </>
      )}
    </div>
  )
}