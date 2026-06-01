import { useState, useEffect } from 'react'
import { Plus, Server, FolderTree, ListTodo, Pencil } from 'lucide-react'
import { useSessionStore } from '@/stores/sessionStore'
import { useConfigStore } from '@/stores/configStore'
import SessionList from '@/components/session/SessionList'
import McpConfigPage from '@/components/mcp/McpConfigPage'
import FileTree, { type FileNode } from '@/components/explorer/FileTree'
import TaskList from '@/components/explorer/TaskList'
import WorkspacePicker from '@/components/explorer/WorkspacePicker'

export default function Sidebar() {
  const createSession = useSessionStore((s) => s.createSession)
  const workspace = useConfigStore((s) => s.workspace)
  const loadWorkspace = useConfigStore((s) => s.loadWorkspace)
  const [mcpOpen, setMcpOpen] = useState(false)
  const [tree, setTree] = useState<FileNode[]>([])
  const [treeExpanded, setTreeExpanded] = useState(false)
  const [workspacePickerOpen, setWorkspacePickerOpen] = useState(false)

  const fetchTree = (path?: string) => {
    const root = path ?? workspace ?? '.'
    fetch(`/api/files/tree?path=${encodeURIComponent(root)}`)
      .then((r) => r.json())
      .then((d) => setTree(d.tree || []))
      .catch(() => {})
  }

  useEffect(() => {
    loadWorkspace()
  }, [])

  useEffect(() => {
    if (treeExpanded && workspace) {
      fetchTree()
    }
  }, [workspace])

  useEffect(() => {
    if (treeExpanded) {
      fetchTree()
    }
  }, [treeExpanded])

  const truncatePath = (p: string, maxLen = 40) => {
    if (p.length <= maxLen) return p
    return '...' + p.slice(-(maxLen - 3))
  }

  return (
    <div className="w-[260px] bg-surface border-r border-border flex flex-col h-full">
      <div className="p-3 border-b border-border flex items-center justify-between">
        <span className="text-primary-light font-bold text-sm">GOAT</span>
        <button
          onClick={createSession}
          className="p-1 rounded hover:bg-surface-light text-text-dim hover:text-text transition-colors"
          title="新建会话"
        >
          <Plus size={16} />
        </button>
      </div>
      <div className="flex-1 overflow-y-auto">
        <SessionList />
        <div className="border-t border-border mt-2">
          <button
            onClick={() => setTreeExpanded((v) => !v)}
            className="flex items-center gap-2 w-full px-3 py-2 text-xs text-text-dim hover:text-text hover:bg-surface-light transition-colors"
          >
            <FolderTree size={14} />
            <span>文件树</span>
            <span className="text-text-darker ml-auto">{treeExpanded ? '▾' : '▸'}</span>
          </button>
          {treeExpanded && (
            <>
              <div className="flex items-center gap-1 px-3 py-1 border-b border-border">
                <span className="text-xs text-text-darker truncate flex-1" title={workspace}>
                  {workspace ? truncatePath(workspace) : ''}
                </span>
                <button
                  onClick={() => setWorkspacePickerOpen(true)}
                  className="p-0.5 rounded hover:bg-surface-light text-text-darker hover:text-text transition-colors flex-shrink-0"
                  title="修改工作空间"
                >
                  <Pencil size={11} />
                </button>
              </div>
              <div className="max-h-[200px] overflow-y-auto">
                <FileTree tree={tree} onFileSelect={(path) => {
                  fetch(`/api/files/read?path=${encodeURIComponent(path)}`)
                    .then((r) => r.json())
                    .then((d) => {
                      (window as any).__editFile?.(d.path, d.content)
                    })
                    .catch(() => {})
                }} />
              </div>
            </>
          )}
        </div>
        <div className="border-t border-border">
          <div className="flex items-center gap-2 px-3 py-2 text-xs text-text-dim">
            <ListTodo size={14} />
            <span>后台任务</span>
          </div>
          <TaskList />
        </div>
      </div>
      <div className="p-3 border-t border-border">
        <button
          onClick={() => setMcpOpen(true)}
          className="flex items-center gap-2 text-xs text-text-dim hover:text-text transition-colors w-full"
          title="MCP 配置"
        >
          <Server size={14} />
          MCP Servers
        </button>
      </div>
      <McpConfigPage isOpen={mcpOpen} onClose={() => setMcpOpen(false)} />
      <WorkspacePicker
        isOpen={workspacePickerOpen}
        onClose={() => setWorkspacePickerOpen(false)}
        onSuccess={fetchTree}
      />
    </div>
  )
}