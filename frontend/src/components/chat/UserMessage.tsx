import { User } from 'lucide-react'
import AttachmentChip from './AttachmentChip'
import type { FileAttachment } from '@/types'

interface UserMessageProps {
  content: string
  attachments?: FileAttachment[]
}

export default function UserMessage({ content, attachments }: UserMessageProps) {
  return (
    <div className="flex gap-3 px-4 py-3">
      <div className="w-6 h-6 rounded bg-primary-dim flex items-center justify-center flex-shrink-0 mt-0.5">
        <User size={14} className="text-primary-light" />
      </div>
      <div className="flex-1 min-w-0">
        {attachments && attachments.length > 0 && (
          <div className="flex flex-wrap gap-1 mb-2">
            {attachments.map((att) => (
              <AttachmentChip key={att.path} attachment={att} />
            ))}
          </div>
        )}
        {content && <p className="text-text whitespace-pre-wrap break-words">{content}</p>}
      </div>
    </div>
  )
}