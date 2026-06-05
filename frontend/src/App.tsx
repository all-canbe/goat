import { useState, useEffect } from 'react'
import AppLayout from '@/components/layout/AppLayout'
import ChatArea from '@/components/chat/ChatArea'
import ApprovalOverlay from '@/components/chat/ApprovalOverlay'
import AskUserDialog from '@/components/chat/AskUserDialog'
import PlanCompareOverlay from '@/components/chat/PlanCompareOverlay'
import CommandPalette from '@/components/command/CommandPalette'
import FileEditor from '@/components/editor/FileEditor'
import { useWebSocket } from '@/hooks/useWebSocket'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import { useSkillStore } from '@/stores/skillStore'

function App() {
  useWebSocket()
  useKeyboardShortcuts()

  // 启动时预加载技能列表，使 `/技能名` 触发范式在用户首次唤起命令面板时即可用
  useEffect(() => {
    useSkillStore.getState().loadSkills()
  }, [])

  const [fileEditorOpen, setFileEditorOpen] = useState(false)
  const [fileEditorPath, setFileEditorPath] = useState('')
  const [fileEditorContent, setFileEditorContent] = useState('')

  const handleOpenFile = () => {
    const path = prompt('输入文件路径:')
    if (path) {
      setFileEditorPath(path)
      setFileEditorContent('')
      setFileEditorOpen(true)
    }
  }

  const handleNewFile = () => {
    const path = prompt('输入新文件路径:')
    if (path) {
      setFileEditorPath(path)
      setFileEditorContent('')
      setFileEditorOpen(true)
    }
  }

  const handleEditFile = (path: string, content: string) => {
    setFileEditorPath(path)
    setFileEditorContent(content)
    setFileEditorOpen(true)
  }

  ;(window as any).__editFile = handleEditFile

  return (
    <AppLayout>
      <ChatArea />
      <ApprovalOverlay />
      <AskUserDialog />
      <PlanCompareOverlay />
      <CommandPalette onOpenFile={handleOpenFile} onNewFile={handleNewFile} onEditFile={handleEditFile} />
      <FileEditor
        isOpen={fileEditorOpen}
        onClose={() => setFileEditorOpen(false)}
        filePath={fileEditorPath}
        initialContent={fileEditorContent}
      />
    </AppLayout>
  )
}

export default App