import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'
import { useConnectionStore } from '@/stores/connectionStore'
import PlanCompleteDialog from './PlanCompleteDialog'

export default function PlanCompleteOverlay() {
  const activeState = useChatStore((s) => s.getActiveState())
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const pendingPlanComplete = activeState?.pendingPlanComplete ?? null

  if (!pendingPlanComplete) return null

  const sid = activeSessionId || 'default'

  // 优先使用事件 payload 中的 planContent（从磁盘读取的实际计划文档）
  // 为空时回退到 lastAssistant 消息内容（兼容旧行为）
  const messages = activeState?.messages ?? []
  const lastAssistant = [...messages].reverse().find((m) => m.role === 'assistant')
  const planContent = pendingPlanComplete.planContent || (lastAssistant?.content || '')
  const planPath = pendingPlanComplete.planPath

  const handleAgentMode = () => {
    const ws = (window as any).__wsClient
    if (ws) {
      ws.send('mode.change', { mode: 'agent' })
      useConnectionStore.getState().setMode('agent')
      ws.send('chat.send', {
        text: `请按照以下计划逐步执行：\n\n${planContent}`,
        sessionId: sid,
      })
    }
    useChatStore.getState().setPendingPlanComplete(sid, null)
  }

  const handleYoloMode = () => {
    const ws = (window as any).__wsClient
    if (ws) {
      ws.send('mode.change', { mode: 'yolo' })
      useConnectionStore.getState().setMode('yolo')
      ws.send('chat.send', {
        text: `请按照以下计划自动执行：\n\n${planContent}`,
        sessionId: sid,
      })
    }
    useChatStore.getState().setPendingPlanComplete(sid, null)
  }

  const handleContinue = () => {
    const ws = (window as any).__wsClient
    if (ws) {
      ws.send('chat.send', {
        text: '请继续完善上述计划',
        sessionId: sid,
      })
    }
    useChatStore.getState().setPendingPlanComplete(sid, null)
  }

  const handleCancel = () => {
    useChatStore.getState().setPendingPlanComplete(sid, null)
  }

  return (
    <PlanCompleteDialog
      planContent={planContent}
      planPath={planPath}
      onAgentMode={handleAgentMode}
      onYoloMode={handleYoloMode}
      onContinue={handleContinue}
      onCancel={handleCancel}
    />
  )
}