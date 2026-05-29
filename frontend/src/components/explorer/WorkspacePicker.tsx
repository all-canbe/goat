import { useState } from 'react'
import { X, Check, AlertCircle } from 'lucide-react'
import { useConfigStore } from '@/stores/configStore'

interface WorkspacePickerProps {
  isOpen: boolean
  onClose: () => void
  onSuccess?: () => void
}

export default function WorkspacePicker({ isOpen, onClose, onSuccess }: WorkspacePickerProps) {
  const currentWorkspace = useConfigStore((s) => s.workspace)
  const setWorkspace = useConfigStore((s) => s.setWorkspace)
  const [inputValue, setInputValue] = useState(currentWorkspace)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  if (!isOpen) return null

  const handleSubmit = async () => {
    setError('')
    setLoading(true)
    try {
      const ok = await setWorkspace(inputValue.trim())
      if (ok) {
        onSuccess?.()
        onClose()
      } else {
        setError('修改失败，请检查路径是否正确')
      }
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSubmit()
    }
    if (e.key === 'Escape') {
      onClose()
    }
  }

  return (
    <div className="fixed inset-0 bg-black/50 z-50 flex items-center justify-center p-4">
      <div className="bg-bg border border-border rounded-lg w-full max-w-lg shadow-xl">
        <div className="flex items-center justify-between p-3 border-b border-border">
          <h3 className="text-sm font-semibold text-text">修改工作空间</h3>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-surface-light text-text-dim transition-colors"
          >
            <X size={16} />
          </button>
        </div>
        <div className="p-4">
          <div className="mb-3">
            <label className="block text-xs text-text-dim mb-1">工作空间路径</label>
            <input
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入绝对路径..."
              className="w-full px-2 py-1.5 text-xs bg-input-bg border border-border rounded focus:outline-none focus:border-primary text-text"
              autoFocus
            />
          </div>
          {error && (
            <div className="flex items-center gap-1 text-xs text-error mb-3">
              <AlertCircle size={12} />
              {error}
            </div>
          )}
          <div className="flex justify-end gap-2">
            <button
              onClick={onClose}
              className="px-3 py-1.5 text-xs text-text-dim hover:text-text transition-colors"
            >
              取消
            </button>
            <button
              onClick={handleSubmit}
              disabled={loading}
              className="px-3 py-1.5 text-xs bg-primary text-primary-contrast rounded hover:bg-primary/90 disabled:opacity-50 transition-colors flex items-center gap-1"
            >
              {loading ? '保存中...' : '确认'}
              <Check size={12} />
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
