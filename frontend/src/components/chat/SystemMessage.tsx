import { Info } from 'lucide-react'

interface SystemMessageProps {
  content: string
}

export default function SystemMessage({ content }: SystemMessageProps) {
  return (
    <div className="flex gap-3 px-4 py-3 bg-surface/50">
      <div className="w-6 h-6 rounded bg-surface-light flex items-center justify-center flex-shrink-0 mt-0.5">
        <Info size={14} className="text-text-dim" />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-text-dim whitespace-pre-wrap break-words">{content}</p>
      </div>
    </div>
  )
}