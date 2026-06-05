import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'
import PlanCompareDialog from './PlanCompareDialog'

export default function PlanCompareOverlay() {
  const activeState = useChatStore((s) => s.getActiveState())
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const pendingPlanCompare = activeState?.pendingPlanCompare ?? null

  if (!pendingPlanCompare) return null

  const sendChoice = (choice: string) => {
    const ws = (window as any).__wsClient
    if (ws) {
      ws.send('flow.plan_choice', {
        choice,
        sessionId: activeSessionId || 'default',
      })
    }
    useChatStore.getState().setPendingPlanCompare(activeSessionId || 'default', null)
  }

  const handleChooseOriginal = () => sendChoice('original')
  const handleChooseReviewed = () => sendChoice('reviewed')
  const handleCancel = () => sendChoice('cancelled')

  return (
    <PlanCompareDialog
      planCompare={pendingPlanCompare}
      onChooseOriginal={handleChooseOriginal}
      onChooseReviewed={handleChooseReviewed}
      onCancel={handleCancel}
    />
  )
}