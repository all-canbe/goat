import ReactMarkdown from 'react-markdown'
import { Bot } from 'lucide-react'

interface StreamingBubbleProps {
  content: string
}

export default function StreamingBubble({ content }: StreamingBubbleProps) {
  if (!content) return null

  return (
    <div className="flex gap-3 px-4 py-3 bg-surface/50">
      <div className="w-6 h-6 rounded bg-primary-dim/50 flex items-center justify-center flex-shrink-0 mt-0.5">
        <Bot size={14} className="text-primary animate-pulse" />
      </div>
      <div className="flex-1 min-w-0 prose prose-invert prose-sm max-w-none">
        <ReactMarkdown>{content}</ReactMarkdown>
        <span className="inline-block w-2 h-4 bg-primary-light animate-pulse ml-0.5 align-middle" />
      </div>
    </div>
  )
}