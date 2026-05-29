import { X, Check, ShieldAlert } from 'lucide-react'
import type { PendingApproval } from '@/types'

interface ApprovalDialogProps {
  approval: PendingApproval
  onApprove: () => void
  onReject: () => void
}

export default function ApprovalDialog({ approval, onApprove, onReject }: ApprovalDialogProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg w-[420px] max-w-[90vw] shadow-2xl">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
          <ShieldAlert size={16} className="text-warning" />
          <span className="text-text text-sm font-medium">工具调用审批</span>
        </div>

        <div className="px-4 py-3">
          {approval.riskLevel && (
            <div className="mb-3">
              <span className="text-text-dim text-xs">风险等级</span>
              <div className="flex items-center gap-2 mt-0.5">
                <span className={`inline-block w-2 h-2 rounded-full ${
                  approval.riskLevel === 'critical' ? 'bg-error animate-pulse' :
                  approval.riskLevel === 'high' ? 'bg-error' :
                  approval.riskLevel === 'medium' ? 'bg-warning' :
                  'bg-text-darker'
                }`} />
                <span className={`text-sm font-medium ${
                  approval.riskLevel === 'critical' ? 'text-error' :
                  approval.riskLevel === 'high' ? 'text-error' :
                  approval.riskLevel === 'medium' ? 'text-warning' :
                  'text-text-dim'
                }`}>
                  {approval.riskLevel === 'critical' ? '极高' :
                   approval.riskLevel === 'high' ? '高' :
                   approval.riskLevel === 'medium' ? '中' : '低'}
                </span>
              </div>
            </div>
          )}

          <div className="mb-3">
            <span className="text-text-dim text-xs">工具名称</span>
            <p className="text-text font-mono text-sm mt-0.5">{approval.toolName}</p>
          </div>

          <div className="mb-3">
            <span className="text-text-dim text-xs">描述</span>
            <p className="text-text text-sm mt-0.5 whitespace-pre-wrap break-words">
              {approval.description || '无描述'}
            </p>
          </div>

          <div>
            <span className="text-text-dim text-xs">参数</span>
            <pre className="mt-0.5 p-2 bg-surface-light rounded text-xs text-text-dim font-mono overflow-x-auto max-h-32 overflow-y-auto">
              {JSON.stringify(approval.args, null, 2)}
            </pre>
          </div>

          {approval.diffContent && (
            <div className="mt-3">
              <span className="text-text-dim text-xs">变更预览</span>
              <pre className="mt-0.5 p-2 bg-surface-light rounded text-xs font-mono overflow-x-auto max-h-48 overflow-y-auto whitespace-pre-wrap">
                {approval.diffContent.split('\n').map((line, i) => (
                  <div key={i} className={
                    line.startsWith('+') ? 'text-success' :
                    line.startsWith('-') ? 'text-error' :
                    'text-text-dim'
                  }>{line}</div>
                ))}
              </pre>
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 px-4 py-3 border-t border-border">
          <button
            onClick={onReject}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors"
          >
            <X size={14} />
            拒绝
          </button>
          <button
            onClick={onApprove}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors"
          >
            <Check size={14} />
            批准
          </button>
        </div>
      </div>
    </div>
  )
}