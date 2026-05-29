import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'
import ApprovalDialog from './ApprovalDialog'

export default function ApprovalOverlay() {
  const activeState = useChatStore((s) => s.getActiveState())
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const pendingApproval = activeState?.pendingApproval ?? null

  if (!pendingApproval) return null

  const handleApprove = () => {
    const ws = (window as any).__wsClient
    if (ws) {
      ws.send('tool.approve', { toolCallId: pendingApproval.toolCallId })
    }
    useChatStore.getState().setPendingApproval(activeSessionId || 'default', null)
  }

  const handleReject = () => {
    const ws = (window as any).__wsClient
    if (ws) {
      ws.send('tool.reject', { toolCallId: pendingApproval.toolCallId })
    }
    useChatStore.getState().setPendingApproval(activeSessionId || 'default', null)
  }

  return (
    <ApprovalDialog
      approval={pendingApproval}
      onApprove={handleApprove}
      onReject={handleReject}
    />
  )
}