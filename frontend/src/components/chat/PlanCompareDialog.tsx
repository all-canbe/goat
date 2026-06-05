import { X, Check, FileText, ArrowRight } from 'lucide-react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import type { PendingPlanCompare } from '@/types'

interface PlanCompareDialogProps {
  planCompare: PendingPlanCompare
  onChooseOriginal: () => void
  onChooseReviewed: () => void
  onCancel: () => void
}

export default function PlanCompareDialog({
  planCompare,
  onChooseOriginal,
  onChooseReviewed,
  onCancel,
}: PlanCompareDialogProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg w-[90vw] max-w-5xl max-h-[85vh] shadow-2xl flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
          <div className="flex items-center gap-2">
            <FileText size={16} className="text-primary" />
            <span className="text-text text-sm font-medium">计划对比审查</span>
            <span className="text-text-dim text-xs ml-2 truncate max-w-[300px]">
              {planCompare.task}
            </span>
          </div>
          <button
            onClick={onCancel}
            className="text-text-dim hover:text-text transition-colors"
          >
            <X size={16} />
          </button>
        </div>

        {/* Body: Dual Panel */}
        <div className="flex-1 flex min-h-0">
          {/* Original Plan Panel */}
          <div className="flex-1 flex flex-col min-w-0 border-r border-border">
            <div className="flex items-center justify-between px-3 py-2 border-b border-border shrink-0 bg-surface-light">
              <span className="text-text-dim text-xs font-medium">原始计划 (A)</span>
              <span className="text-text-darker text-xs">by 规划模型</span>
            </div>
            <div className="flex-1 overflow-y-auto p-3">
              <div className="prose prose-sm max-w-none text-text [&_h1]:text-text [&_h2]:text-text [&_h3]:text-text [&_strong]:text-text [&_code]:text-primary [&_pre]:bg-surface-light [&_pre]:text-text-dim [&_li]:text-text-dim [&_p]:text-text-dim">
                <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                  {planCompare.originalPlan}
                </ReactMarkdown>
              </div>
            </div>
          </div>

          {/* Reviewed Plan Panel */}
          <div className="flex-1 flex flex-col min-w-0">
            <div className="flex items-center justify-between px-3 py-2 border-b border-border shrink-0 bg-surface-light">
              <span className="text-primary text-xs font-medium flex items-center gap-1">
                <ArrowRight size={12} />
                审查后计划 (B)
              </span>
              <span className="text-text-darker text-xs">by 审查模型</span>
            </div>
            <div className="flex-1 overflow-y-auto p-3">
              <div className="prose prose-sm max-w-none text-text [&_h1]:text-text [&_h2]:text-text [&_h3]:text-text [&_strong]:text-text [&_code]:text-primary [&_pre]:bg-surface-light [&_pre]:text-text-dim [&_li]:text-text-dim [&_p]:text-text-dim">
                <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                  {planCompare.reviewedPlan}
                </ReactMarkdown>
              </div>
            </div>
          </div>
        </div>

        {/* Footer: Action Buttons */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-border shrink-0">
          <div className="text-text-dim text-xs">
            请选择要执行的计划
          </div>
          <div className="flex gap-2">
            <button
              onClick={onCancel}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors"
            >
              <X size={14} />
              取消
            </button>
            <button
              onClick={onChooseOriginal}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded text-sm bg-surface-lighter text-text hover:bg-surface-light border border-border transition-colors"
            >
              <FileText size={14} />
              执行原始计划 (A)
            </button>
            <button
              onClick={onChooseReviewed}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors"
            >
              <Check size={14} />
              执行审查后计划 (B)
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}