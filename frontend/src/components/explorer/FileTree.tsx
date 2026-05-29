import { useState, useRef, useEffect, useCallback } from 'react'
import { ChevronRight, ChevronDown, File, Folder, FolderOpen, FilePlus } from 'lucide-react'

export interface FileNode {
  name: string
  path: string
  type: 'file' | 'directory'
  children?: FileNode[]
}

interface FileTreeProps {
  tree: FileNode[]
  onFileSelect?: (path: string) => void
}

interface ContextMenu {
  x: number
  y: number
  node: FileNode
}

export default function FileTree({ tree, onFileSelect }: FileTreeProps) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [menu, setMenu] = useState<ContextMenu | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!menu) return
    const handler = (e: MouseEvent) => {
      if (e.button !== 0) return
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenu(null)
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [menu])

  const toggleExpand = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }, [])

  const handleContextMenu = useCallback((e: React.MouseEvent, node: FileNode) => {
    e.preventDefault()
    e.stopPropagation()
    setMenu({ x: e.clientX, y: e.clientY, node })
  }, [])

  const renderNode = (node: FileNode, depth: number) => {
    const isExpanded = expanded.has(node.path)
    const paddingLeft = depth * 16 + 8

    return (
      <div key={node.path}>
        <div
          onClick={() => {
            if (node.type === 'directory') {
              toggleExpand(node.path)
            } else {
              onFileSelect?.(node.path)
            }
          }}
          onContextMenu={(e) => handleContextMenu(e, node)}
          className="flex items-center gap-1 px-2 py-1 cursor-pointer text-xs text-text-dim hover:text-text hover:bg-surface-light transition-colors rounded-none"
          style={{ paddingLeft: `${paddingLeft}px` }}
        >
          {node.type === 'directory' ? (
            isExpanded ? <ChevronDown size={12} className="flex-shrink-0" /> : <ChevronRight size={12} className="flex-shrink-0" />
          ) : (
            <span className="w-3 flex-shrink-0" />
          )}
          {node.type === 'directory' ? (
            isExpanded
              ? <FolderOpen size={14} className="flex-shrink-0 text-warning" />
              : <Folder size={14} className="flex-shrink-0 text-warning" />
          ) : (
            <File size={14} className="flex-shrink-0 text-text-darker" />
          )}
          <span className="truncate">{node.name}</span>
        </div>
        {node.type === 'directory' && isExpanded && node.children && (
          <div>
            {node.children.map((child) => renderNode(child, depth + 1))}
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="py-0.5" onContextMenu={(e) => { e.preventDefault(); setMenu(null) }}>
      {tree.map((node) => renderNode(node, 0))}
      {menu && (
        <div
          ref={menuRef}
          className="fixed z-50 bg-surface border border-border rounded shadow-lg py-1 min-w-[130px]"
          style={{ left: menu.x, top: menu.y }}
        >
          <button
            onClick={() => {
              (window as any).__addFileToInput?.(menu.node.path, menu.node.name)
              setMenu(null)
            }}
            className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-text-dim hover:text-text hover:bg-surface-light transition-colors"
          >
            <FilePlus size={12} />
            添加到对话
          </button>
        </div>
      )}
    </div>
  )
}