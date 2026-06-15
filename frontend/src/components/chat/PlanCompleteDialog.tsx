import { FileText, Bot, Zap, Pencil, X } from 'lucide-react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'

interface PlanCompleteDialogProps {
  planContent: string
  planPath: string
  onAgentMode: () => void
  onYoloMode: () => void
  onContinue: () => void
  onCancel: () => void
}

export default function PlanCompleteDialog({
  planContent,
  planPath,
  onAgentMode,
  onYoloMode,
  onContinue,
  onCancel,
}: PlanCompleteDialogProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg w-[90vw] max-w-5xl max-h-[85vh] shadow-2xl flex flex-col">
        {/* Header */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border shrink-0">
          <FileText size={16} className="text-primary" />
          <span className="text-text text-sm font-medium">计划已完成</span>
          {planPath && (
            <span className="text-text-dim text-xs ml-2 truncate">
              {planPath.split('/').pop() || planPath.split('\\').pop() || planPath}
            </span>
          )}
        </div>

        {/* Plan Content */}
        <div className="flex-1 overflow-y-auto p-4 min-h-0">
          <div className="prose prose-sm max-w-none text-text
            [&_h1]:text-text [&_h2]:text-text [&_h3]:text-text
            [&_strong]:text-text [&_code]:text-primary
            [&_pre]:bg-surface-light [&_pre]:text-text-dim
            [&_li]:text-text-dim [&_p]:text-text-dim
            [&_table]:border-border [&_th]:bg-surface-light [&_td]:border-border">
            <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
              {planContent}
            </ReactMarkdown>
          </div>
        </div>

        {/* Footer: Action Buttons */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-border shrink-0">
          <div className="text-text-dim text-xs">
            请选择执行方式
          </div>
          <div className="flex gap-2">
            <button
              onClick={onCancel}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors border border-border"
            >
              <X size={14} />
              取消
            </button>
            <button
              onClick={onContinue}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors border border-border"
            >
              <Pencil size={14} />
              继续修改
            </button>
            <button
              onClick={onYoloMode}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded text-sm bg-surface-lighter text-text hover:bg-surface-light transition-colors border border-border"
            >
              <Zap size={14} />
              YOLO 执行
            </button>
            <button
              onClick={onAgentMode}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors"
            >
              <Bot size={14} />
              Agent 执行
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}