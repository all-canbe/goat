import { useState } from 'react'
import AppLayout from '@/components/layout/AppLayout'
import ChatArea from '@/components/chat/ChatArea'
import ApprovalOverlay from '@/components/chat/ApprovalOverlay'
import AskUserDialog from '@/components/chat/AskUserDialog'
import CommandPalette from '@/components/command/CommandPalette'
import FileEditor from '@/components/editor/FileEditor'
import { useWebSocket } from '@/hooks/useWebSocket'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'

function App() {
  useWebSocket()
  useKeyboardShortcuts()

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