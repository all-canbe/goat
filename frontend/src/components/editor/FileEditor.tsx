import { useState, useEffect } from 'react'
import { X } from 'lucide-react'

interface FileEditorProps {
  isOpen: boolean
  onClose: () => void
  filePath?: string
  initialContent?: string
}

export default function FileEditor({ isOpen, onClose, filePath, initialContent }: FileEditorProps) {
  const [content, setContent] = useState(initialContent || '')
  const [hasChanges, setHasChanges] = useState(false)
  const [showConfirm, setShowConfirm] = useState(false)

  useEffect(() => {
    if (isOpen) {
      setContent(initialContent || '')
      setHasChanges(false)
      setShowConfirm(false)
    }
  }, [isOpen, initialContent])

  const handleSave = () => {
    const ws = (window as any).__wsClient
    if (ws) {
      ws.send('file.save', { filePath, content })
    }
    setHasChanges(false)
    onClose()
  }

  const handleCancel = () => {
    if (hasChanges) {
      setShowConfirm(true)
    } else {
      onClose()
    }
  }

  if (!isOpen) return null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg w-[640px] max-w-[90vw] max-h-[80vh] flex flex-col shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 border-b border-border">
          <div className="flex items-center gap-2">
            <span className="text-text text-sm font-medium">文件编辑</span>
            {filePath && (
              <span className="text-text-dim text-xs font-mono">{filePath}</span>
            )}
          </div>
          <button onClick={handleCancel} className="text-text-darker hover:text-text transition-colors">
            <X size={16} />
          </button>
        </div>

        <textarea
          value={content}
          onChange={(e) => {
            setContent(e.target.value)
            setHasChanges(true)
          }}
          className="flex-1 p-4 bg-bg text-text font-mono text-sm outline-none resize-none border-none"
          spellCheck={false}
        />

        <div className="flex justify-end gap-2 px-4 py-3 border-t border-border">
          {showConfirm && (
            <div className="flex items-center gap-2 mr-auto">
              <span className="text-xs text-warning">有未保存的更改，确定关闭？</span>
              <button
                onClick={() => {
                  setShowConfirm(false)
                  onClose()
                }}
                className="text-xs px-2 py-1 rounded bg-surface-light text-text hover:bg-surface-lighter transition-colors"
              >
                确认关闭
              </button>
            </div>
          )}
          <button
            onClick={handleCancel}
            className="px-3 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors"
          >
            取消
          </button>
          <button
            onClick={handleSave}
            className="px-3 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  )
}