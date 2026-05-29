import { AlertTriangle } from 'lucide-react'

interface ErrorMessageProps {
  content: string
}

export default function ErrorMessage({ content }: ErrorMessageProps) {
  return (
    <div className="flex gap-3 px-4 py-3 bg-error/10 border-l-2 border-error/50">
      <div className="w-6 h-6 rounded bg-error/20 flex items-center justify-center flex-shrink-0 mt-0.5">
        <AlertTriangle size={14} className="text-error" />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-error whitespace-pre-wrap break-words">{content}</p>
      </div>
    </div>
  )
}