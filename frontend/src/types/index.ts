export interface WSMessage {
  type: string
  payload: Record<string, unknown>
  timestamp?: number
}

export interface Message {
  id: string
  role: 'user' | 'assistant' | 'system' | 'error'
  content: string
  toolCalls?: ToolCall[]
  timestamp: number
}

export interface ToolCall {
  name: string
  args: Record<string, unknown>
  result?: string
  status: 'running' | 'complete' | 'error'
}

export interface Session {
  id: string
  title: string
  createdAt: string
  messageCount: number
  isRunning?: boolean
}

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected'
export type PermissionMode = 'plan' | 'agent' | 'yolo' | 'flow'

export interface PendingApproval {
  toolCallId: string
  toolName: string
  args: Record<string, unknown>
  description: string
  riskLevel?: 'low' | 'medium' | 'high' | 'critical'
  diffContent?: string
}

export interface PendingQuestion {
  questionId: string
  question: string
}