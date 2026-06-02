import { useEffect } from 'react'
import { WSClient } from '@/lib/ws-client'
import { useChatStore } from '@/stores/chatStore'
import { useConnectionStore } from '@/stores/connectionStore'
import { useSessionStore } from '@/stores/sessionStore'
import type { PendingApproval, PermissionMode, ToolCall } from '@/types'
import { useTaskStore } from '@/stores/taskStore'

function getSessionId(msg: any): string {
  return (msg.payload?.sessionId as string) || 'default'
}

export function useWebSocket() {
  useEffect(() => {
    const client = new WSClient()

    const unsubStatus = client.onStatusChange((status) => {
      useConnectionStore.getState().setStatus(status)
    })

    const unsubStream = client.on('chat.stream', (msg) => {
      const token = msg.payload.token as string
      const sid = getSessionId(msg)
      if (token) useChatStore.getState().appendStreamToken(sid, token)
    })

    const unsubStatusUpdate = client.on('status.update', (msg) => {
      const p = msg.payload
      if (typeof p.tokenCount === 'number') useConnectionStore.getState().setTokenCount(p.tokenCount)
      if (typeof p.contextPct === 'number') useConnectionStore.getState().setContextPct(p.contextPct)
      if (typeof p.isThinking === 'boolean') useConnectionStore.getState().setIsThinking(p.isThinking)
      if (typeof p.isToolRunning === 'boolean') useConnectionStore.getState().setIsToolRunning(p.isToolRunning)
      if (typeof p.toolCommand === 'string') useConnectionStore.getState().setToolCommand(p.toolCommand)
      if (typeof p.hasPendingApproval === 'boolean') useConnectionStore.getState().setHasPendingApproval(p.hasPendingApproval)
      if (typeof p.model === 'string') useConnectionStore.getState().setModelName(p.model)
      if (typeof p.provider === 'string') useConnectionStore.getState().setProviderName(p.provider)
      if (typeof p.tokenInput === 'number') useConnectionStore.getState().setTokenInput(p.tokenInput)
      if (typeof p.tokenOutput === 'number') useConnectionStore.getState().setTokenOutput(p.tokenOutput)
      if (typeof p.cost === 'number') useConnectionStore.getState().setCost(p.cost)
      if (typeof p.messageCount === 'number') useConnectionStore.getState().setMessageCount(p.messageCount)
      if (typeof p.mode === 'string') {
        const validModes: PermissionMode[] = ['plan', 'agent', 'yolo', 'flow']
        if (validModes.includes(p.mode as PermissionMode)) {
          useConnectionStore.getState().setMode(p.mode as PermissionMode)
        }
      }
    })

    const unsubResponse = client.on('chat.response', (msg) => {
      const messageId = msg.payload.messageId as string
      const sid = getSessionId(msg)
      useChatStore.getState().commitStream(sid, messageId)
    })

    const unsubError = client.on('chat.error', (msg) => {
      const error = msg.payload.error as string
      const sid = getSessionId(msg)
      useChatStore.getState().addMessage(sid, {
        id: crypto.randomUUID(),
        role: 'system',
        content: `Error: ${error}`,
        timestamp: Date.now(),
      })
      useChatStore.getState().clearStream(sid)
    })

    const unsubApproval = client.on('tool.require_approval', (msg) => {
      const sid = getSessionId(msg)
      const pending: PendingApproval = {
        toolCallId: (msg.payload.toolCallId as string) || crypto.randomUUID(),
        toolName: (msg.payload.toolName as string) || '',
        args: (msg.payload.args as Record<string, unknown>) || {},
        description: (msg.payload.description as string) || '',
        riskLevel: (msg.payload.riskLevel as 'low' | 'medium' | 'high' | 'critical' | undefined) || undefined,
        diffContent: (msg.payload.diffContent as string) || undefined,
      }
      useChatStore.getState().setPendingApproval(sid, pending)
    })

    const unsubToolComplete = client.on('tool.complete', (msg) => {
      const p = msg.payload
      const sid = getSessionId(msg)
      const tc: ToolCall = {
        name: (p.toolName as string) || '',
        args: (p.args as Record<string, unknown>) || {},
        result: (p.result as string) || '',
        status: 'complete',
      }
      useChatStore.getState().addToolCall(sid, tc)
    })

    const unsubAskUser = client.on('ask_user', (msg) => {
      const activeSid = useSessionStore.getState().activeSessionId
      const sid = activeSid || getSessionId(msg)
      useChatStore.getState().setPendingQuestion(sid, {
        questionId: (msg.payload.questionId as string) || '',
        question: (msg.payload.question as string) || '',
      })
    })

    const unsubNotification = client.on('status.notification', (msg) => {
      const text = msg.payload.text as string
      if (text) {
        useConnectionStore.getState().setNotificationText(text)
        setTimeout(() => useConnectionStore.getState().setNotificationText(''), 3000)
      }
    })

    const unsubSubagentLifecycle = client.on('subagent.lifecycle', (msg) => {
      const p = msg.payload
      const agentName = (p.agentName as string) || 'subagent'
      const status = (p.status as string) || ''

      if (status === 'started') {
        const store = useTaskStore.getState()
        const existing = store.tasks.find((t) => t.id === agentName)
        if (existing) {
          store.updateTask(agentName, { status: 'running', progress: 0, createdAt: Date.now() })
        } else {
          store.addTask({
            id: agentName,
            name: agentName,
            status: 'running',
            progress: 0,
            createdAt: Date.now(),
          })
        }
      } else if (status === 'completed') {
        useTaskStore.getState().updateTask(agentName, { status: 'completed', progress: 100 })
      } else if (status === 'failed') {
        useTaskStore.getState().updateTask(agentName, { status: 'failed', progress: 0 })
      } else if (status === 'cancelled') {
        useTaskStore.getState().updateTask(agentName, { status: 'cancelled', progress: 0 })
      }
    })

    const unsubAuditLog = client.on('audit.log', (msg) => {
      const content = msg.payload.content as string
      const sid = getSessionId(msg)
      if (content) {
        useChatStore.getState().addMessage(sid, {
          id: crypto.randomUUID(),
          role: 'system',
          content: `[审计] ${content}`,
          timestamp: Date.now(),
        })
      }
    })

    const unsubToolRetry = client.on('tool.retry', (msg) => {
      const sid = getSessionId(msg)
      const toolName = (msg.payload.toolName as string) || ''
      const message = (msg.payload.message as string) || ''
      useChatStore.getState().addMessage(sid, {
        id: crypto.randomUUID(),
        role: 'system',
        content: `[重试] ${toolName}: ${message}`,
        timestamp: Date.now(),
      })
    })

    ;(window as any).__wsClient = client
    client.connect()

    return () => {
      unsubStatus()
      unsubStream()
      unsubStatusUpdate()
      unsubResponse()
      unsubError()
      unsubApproval()
      unsubToolComplete()
      unsubAskUser()
      unsubNotification()
      unsubSubagentLifecycle()
      unsubAuditLog()
      unsubToolRetry()
      ;(window as any).__wsClient = null
      client.disconnect()
    }
  }, [])
}