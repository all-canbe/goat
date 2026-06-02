import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Bot } from 'lucide-react'

interface StreamingBubbleProps {
  content: string
}

function ThinkingDots() {
  return (
    <div className="flex gap-1 items-center py-2">
      <span className="w-1.5 h-1.5 bg-primary rounded-full animate-bounce [animation-delay:0ms]" />
      <span className="w-1.5 h-1.5 bg-primary rounded-full animate-bounce [animation-delay:150ms]" />
      <span className="w-1.5 h-1.5 bg-primary rounded-full animate-bounce [animation-delay:300ms]" />
    </div>
  )
}

export default function StreamingBubble({ content }: StreamingBubbleProps) {
  return (
    <div className="flex gap-3 px-4 py-3 bg-surface/50">
      <div className="w-6 h-6 rounded bg-primary-dim/50 flex items-center justify-center flex-shrink-0 mt-0.5">
        <Bot size={14} className="text-primary animate-pulse" />
      </div>
      <div className="flex-1 min-w-0">
        {content ? (
          <div className="prose prose-invert prose-sm max-w-none">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
            <span className="inline-block w-2 h-4 bg-primary-light animate-pulse ml-0.5 align-middle" />
          </div>
        ) : (
          <ThinkingDots />
        )}
      </div>
    </div>
  )
}