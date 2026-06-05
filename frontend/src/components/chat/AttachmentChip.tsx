import { File, X } from 'lucide-react'
import type { FileAttachment } from '@/types'

interface AttachmentChipProps {
  attachment: FileAttachment
  onRemove?: () => void
}

export default function AttachmentChip({ attachment, onRemove }: AttachmentChipProps) {
  return (
    <span className="inline-flex items-center gap-1 px-2 py-1 bg-emerald-500/10 text-emerald-400 rounded text-xs">
      <File size={12} />
      <span className="max-w-[120px] truncate">{attachment.name}</span>
      {onRemove && (
        <button
          onClick={onRemove}
          className="hover:text-emerald-200 transition-colors"
        >
          <X size={12} />
        </button>
      )}
    </span>
  )
}