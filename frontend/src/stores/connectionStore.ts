import { create } from 'zustand'
import type { ConnectionStatus, PermissionMode } from '@/types'

interface ConnectionStore {
  status: ConnectionStatus
  mode: PermissionMode
  tokenCount: number
  contextPct: number
  isThinking: boolean
  isToolRunning: boolean
  toolCommand: string
  hasPendingApproval: boolean
  notificationText: string
  providerName: string
  modelName: string
  tokenInput: number
  tokenOutput: number
  cost: number
  messageCount: number
  setStatus: (status: ConnectionStatus) => void
  setMode: (mode: PermissionMode) => void
  setTokenCount: (count: number) => void
  setContextPct: (pct: number) => void
  setIsThinking: (v: boolean) => void
  setIsToolRunning: (v: boolean) => void
  setToolCommand: (cmd: string) => void
  setHasPendingApproval: (v: boolean) => void
  setNotificationText: (text: string) => void
  setProviderName: (name: string) => void
  setModelName: (name: string) => void
  setTokenInput: (n: number) => void
  setTokenOutput: (n: number) => void
  setCost: (n: number) => void
  setMessageCount: (n: number) => void
}

export const useConnectionStore = create<ConnectionStore>((set) => ({
  status: 'disconnected',
  mode: 'agent',
  tokenCount: 0,
  contextPct: 0,
  isThinking: false,
  isToolRunning: false,
  toolCommand: '',
  hasPendingApproval: false,
  notificationText: '',
  providerName: '',
  modelName: '',
  tokenInput: 0,
  tokenOutput: 0,
  cost: 0,
  messageCount: 0,
  setStatus: (status) => set({ status }),
  setMode: (mode) => set({ mode }),
  setTokenCount: (count) => set({ tokenCount: count }),
  setContextPct: (pct) => set({ contextPct: pct }),
  setIsThinking: (v) => set({ isThinking: v }),
  setIsToolRunning: (v) => set({ isToolRunning: v }),
  setToolCommand: (cmd) => set({ toolCommand: cmd }),
  setHasPendingApproval: (v) => set({ hasPendingApproval: v }),
  setNotificationText: (text) => set({ notificationText: text }),
  setProviderName: (name) => set({ providerName: name }),
  setModelName: (name) => set({ modelName: name }),
  setTokenInput: (n) => set({ tokenInput: n }),
  setTokenOutput: (n) => set({ tokenOutput: n }),
  setCost: (n) => set({ cost: n }),
  setMessageCount: (n) => set({ messageCount: n }),
}))