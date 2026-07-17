// P1: Plan Mode 计划文档预览 Modal

import { useCallback, useEffect } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { FileText, Check, X } from "lucide-react";
import { useChatStore } from "../../stores/chatStore";

interface PlanPreviewDialogProps {
  // P1: 接受计划时切换到 Agent Mode
  onAccept?: () => void;
}

export default function PlanPreviewDialog({ onAccept }: PlanPreviewDialogProps) {
  const { planContent, clearPlanContent } = useChatStore();

  // P1: Esc 关闭（拒绝）
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (planContent && e.key === "Escape") {
        e.preventDefault();
        clearPlanContent();
      }
    },
    [planContent, clearPlanContent]
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  // P1: 接受计划 → 切换到 Agent Mode 并关闭 Modal
  const handleAccept = useCallback(() => {
    onAccept?.();
    clearPlanContent();
  }, [onAccept, clearPlanContent]);

  // P1: 拒绝 → 仅关闭 Modal
  const handleReject = useCallback(() => {
    clearPlanContent();
  }, [clearPlanContent]);

  if (!planContent) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg shadow-2xl w-full max-w-3xl mx-4 overflow-hidden flex flex-col max-h-[90vh]">
        {/* Header */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-surface">
          <FileText size={16} className="text-brand" />
          <span className="text-sm font-semibold text-text flex-1">计划文档预览</span>
          <button
            onClick={handleReject}
            title="关闭 (Esc)"
            className="p-1 rounded text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          >
            <X size={16} />
          </button>
        </div>

        {/* Body — Markdown 渲染计划文档 */}
        <div className="flex-1 overflow-y-auto px-5 py-4 prose prose-invert prose-sm max-w-none">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[rehypeHighlight]}
          >
            {planContent}
          </ReactMarkdown>
        </div>

        {/* Footer — 接受/拒绝 */}
        <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border bg-bg/50">
          <button
            onClick={handleReject}
            className="px-4 py-1.5 rounded-md text-xs font-medium text-text-secondary hover:text-text border border-border hover:border-text-secondary transition-colors"
          >
            拒绝 (Esc)
          </button>
          <button
            onClick={handleAccept}
            className="px-4 py-1.5 rounded-md text-xs font-medium text-white bg-primary hover:bg-primary/90 transition-colors flex items-center gap-1.5"
          >
            <Check size={14} />
            接受计划（切换到 Agent Mode）
          </button>
        </div>
      </div>
    </div>
  );
}
