import { useState, useEffect, useRef, useCallback, PointerEvent as ReactPointerEvent } from 'react'
import { X, Maximize2, Minimize2 } from 'lucide-react'

interface FileEditorProps {
  isOpen: boolean
  onClose: () => void
  filePath?: string
  initialContent?: string
}

const DEFAULT_WIDTH = 1100
const DEFAULT_HEIGHT = 720
const MIN_WIDTH = 480
const MIN_HEIGHT = 320

export default function FileEditor({ isOpen, onClose, filePath, initialContent }: FileEditorProps) {
  const [content, setContent] = useState(initialContent || '')
  const [hasChanges, setHasChanges] = useState(false)
  const [showConfirm, setShowConfirm] = useState(false)
  const [size, setSize] = useState({ width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT })
  const [isMaximized, setIsMaximized] = useState(false)
  const [prevSize, setPrevSize] = useState<{ width: number; height: number; left: number; top: number } | null>(null)
  const [position, setPosition] = useState({ left: 0, top: 0 })
  const draggingRef = useRef<{ startX: number; startY: number; startW: number; startH: number; startL: number; startT: number } | null>(null)
  const moveHandlerRef = useRef<((ev: PointerEvent) => void) | null>(null)
  const upHandlerRef = useRef<((ev: PointerEvent) => void) | null>(null)
  const dialogRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    return () => {
      if (moveHandlerRef.current) {
        document.removeEventListener('pointermove', moveHandlerRef.current)
      }
      if (upHandlerRef.current) {
        document.removeEventListener('pointerup', upHandlerRef.current)
      }
      document.body.style.cursor = ''
    }
  }, [])

  useEffect(() => {
    if (isOpen) {
      setContent(initialContent || '')
      setHasChanges(false)
      setShowConfirm(false)
      setIsMaximized(false)
      setSize({ width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT })
      if (typeof window !== 'undefined') {
        setPosition({
          left: Math.max(20, (window.innerWidth - DEFAULT_WIDTH) / 2),
          top: Math.max(20, (window.innerHeight - DEFAULT_HEIGHT) / 2),
        })
      }
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

  const handleResizeStart = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    if (isMaximized) return
    e.preventDefault()
    e.stopPropagation()
    draggingRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      startW: size.width,
      startH: size.height,
      startL: position.left,
      startT: position.top,
    }
    const onMove = (ev: PointerEvent) => {
      if (!draggingRef.current) return
      const dx = ev.clientX - draggingRef.current.startX
      const dy = ev.clientY - draggingRef.current.startY
      const maxW = typeof window !== 'undefined' ? window.innerWidth - draggingRef.current.startL - 20 : DEFAULT_WIDTH
      const maxH = typeof window !== 'undefined' ? window.innerHeight - draggingRef.current.startT - 20 : DEFAULT_HEIGHT
      const newW = Math.max(MIN_WIDTH, Math.min(maxW, draggingRef.current.startW + dx))
      const newH = Math.max(MIN_HEIGHT, Math.min(maxH, draggingRef.current.startH + dy))
      setSize({ width: newW, height: newH })
    }
    const onUp = () => {
      draggingRef.current = null
      moveHandlerRef.current = null
      upHandlerRef.current = null
      document.removeEventListener('pointermove', onMove)
      document.removeEventListener('pointerup', onUp)
      document.body.style.cursor = ''
    }
    moveHandlerRef.current = onMove
    upHandlerRef.current = onUp
    document.addEventListener('pointermove', onMove)
    document.addEventListener('pointerup', onUp)
    document.body.style.cursor = 'nwse-resize'
  }, [isMaximized, size.width, size.height, position.left, position.top])

  const handleHeaderDragStart = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    if (isMaximized) return
    if ((e.target as HTMLElement).closest('button')) return
    e.preventDefault()
    draggingRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      startW: size.width,
      startH: size.height,
      startL: position.left,
      startT: position.top,
    }
    const onMove = (ev: PointerEvent) => {
      if (!draggingRef.current) return
      const dx = ev.clientX - draggingRef.current.startX
      const dy = ev.clientY - draggingRef.current.startY
      const maxL = typeof window !== 'undefined' ? window.innerWidth - 100 : 0
      const maxT = typeof window !== 'undefined' ? window.innerHeight - 60 : 0
      setPosition({
        left: Math.max(0, Math.min(maxL, draggingRef.current.startL + dx)),
        top: Math.max(0, Math.min(maxT, draggingRef.current.startT + dy)),
      })
    }
    const onUp = () => {
      draggingRef.current = null
      moveHandlerRef.current = null
      upHandlerRef.current = null
      document.removeEventListener('pointermove', onMove)
      document.removeEventListener('pointerup', onUp)
    }
    moveHandlerRef.current = onMove
    upHandlerRef.current = onUp
    document.addEventListener('pointermove', onMove)
    document.addEventListener('pointerup', onUp)
  }, [isMaximized, size.width, size.height, position.left, position.top])

  const toggleMaximize = () => {
    if (isMaximized) {
      if (prevSize) {
        setSize({ width: prevSize.width, height: prevSize.height })
        setPosition({ left: prevSize.left, top: prevSize.top })
      }
      setIsMaximized(false)
    } else {
      setPrevSize({ ...size, ...position })
      if (typeof window !== 'undefined') {
        setSize({ width: window.innerWidth - 40, height: window.innerHeight - 40 })
        setPosition({ left: 20, top: 20 })
      }
      setIsMaximized(true)
    }
  }

  if (!isOpen) return null

  const dialogStyle: React.CSSProperties = isMaximized
    ? { left: 20, top: 20, width: 'calc(100vw - 40px)', height: 'calc(100vh - 40px)' }
    : { left: position.left, top: position.top, width: size.width, height: size.height }

  return (
    <div className="fixed inset-0 z-50 bg-black/60">
      <div
        ref={dialogRef}
        className="absolute bg-surface border border-border rounded-lg flex flex-col shadow-2xl"
        style={dialogStyle}
      >
        <div
          onPointerDown={handleHeaderDragStart}
          className="flex items-center justify-between px-4 py-3 border-b border-border cursor-move select-none flex-shrink-0"
        >
          <div className="flex items-center gap-2 min-w-0">
            <span className="text-text text-sm font-medium">文件编辑</span>
            {filePath && (
              <span className="text-text-dim text-xs font-mono truncate">{filePath}</span>
            )}
          </div>
          <div className="flex items-center gap-1">
            <button
              onClick={toggleMaximize}
              className="text-text-darker hover:text-text transition-colors p-1"
              title={isMaximized ? '还原' : '最大化'}
            >
              {isMaximized ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
            </button>
            <button
              onClick={handleCancel}
              className="text-text-darker hover:text-text transition-colors p-1"
              title="关闭"
            >
              <X size={16} />
            </button>
          </div>
        </div>

        <textarea
          value={content}
          onChange={(e) => {
            setContent(e.target.value)
            setHasChanges(true)
          }}
          className="flex-1 p-4 bg-bg text-text font-mono text-sm outline-none resize-none border-none scrollbar-thin"
          spellCheck={false}
        />

        <div className="flex justify-end gap-2 px-4 py-3 border-t border-border flex-shrink-0">
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

        {!isMaximized && (
          <div
            onPointerDown={handleResizeStart}
            className="absolute bottom-0 right-0 w-4 h-4 cursor-nwse-resize z-10"
            style={{
              background: 'linear-gradient(135deg, transparent 50%, var(--color-text-darker) 50%)',
              borderBottomRightRadius: '0.5rem',
            }}
            title="拖动调整大小"
          />
        )}
      </div>
    </div>
  )
}
