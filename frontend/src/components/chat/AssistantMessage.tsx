import ReactMarkdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import { Bot, Copy, Check } from 'lucide-react'
import { useState } from 'react'

interface AssistantMessageProps {
  content: string
}

function CodeBlock({ className, children }: { className?: string; children?: React.ReactNode }) {
  const [copied, setCopied] = useState(false)
  const code = String(children).replace(/\n$/, '')
  const handleCopy = () => {
    navigator.clipboard.writeText(code)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }
  return (
    <div className="relative group">
      <button onClick={handleCopy} className="absolute right-2 top-2 p-1 rounded bg-surface-light text-text-dim hover:text-text opacity-0 group-hover:opacity-100 transition-opacity">
        {copied ? <Check size={14} className="text-success" /> : <Copy size={14} />}
      </button>
      <pre><code className={className}>{children}</code></pre>
    </div>
  )
}

export default function AssistantMessage({ content }: AssistantMessageProps) {
  return (
    <div className="flex gap-3 px-4 py-3 bg-surface/50">
      <div className="w-6 h-6 rounded bg-primary-dim/50 flex items-center justify-center flex-shrink-0 mt-0.5">
        <Bot size={14} className="text-primary" />
      </div>
      <div className="flex-1 min-w-0 prose prose-invert prose-sm max-w-none">
        <ReactMarkdown
          rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
          components={{
            code({ className, children, ...props }) {
              const isInline = !className
              if (isInline) {
                return <code className="bg-surface-light px-1 py-0.5 rounded text-sm" {...props}>{children}</code>
              }
              return <CodeBlock className={className}>{children}</CodeBlock>
            },
            table({ children }) {
              return <div className="overflow-x-auto"><table className="min-w-full border-collapse border border-surface-light">{children}</table></div>
            },
            th({ children }) {
              return <th className="border border-surface-light px-3 py-2 bg-surface text-left text-sm font-medium">{children}</th>
            },
            td({ children }) {
              return <td className="border border-surface-light px-3 py-2 text-sm">{children}</td>
            },
          }}
        >{content}</ReactMarkdown>
      </div>
    </div>
  )
}