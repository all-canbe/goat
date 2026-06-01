import { create } from 'zustand'
import type { Session, Message } from '@/types'
import { useChatStore } from './chatStore'

interface SessionStore {
  sessions: Session[]
  activeSessionId: string | null
  isLoading: boolean
  setActiveSession: (id: string) => void
  createSession: () => Promise<void>
  deleteSession: (id: string) => Promise<void>
  loadSessions: () => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  loadSessionMessages: (sessionId: string) => Promise<void>
}

function mapSession(raw: any): Session {
  return {
    id: raw.id || raw.session_id,
    title: raw.title,
    createdAt: raw.createdAt || raw.created_at,
    messageCount: raw.messageCount ?? raw.message_count ?? 0,
    isRunning: raw.isRunning ?? false,
  }
}

function mapMessage(raw: any): Message {
  return {
    id: raw.id,
    role: raw.role,
    content: raw.content,
    timestamp: raw.createdAt ? new Date(raw.createdAt).getTime() : Date.now(),
    toolCalls: raw.toolCalls || undefined,
  }
}

export const useSessionStore = create<SessionStore>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  isLoading: false,

  setActiveSession: async (id) => {
    set({ activeSessionId: id })
    useChatStore.getState().setActiveSessionId(id)
    await get().loadSessionMessages(id)
  },

  createSession: async () => {
    const res = await fetch('/api/sessions', { method: 'POST' })
    const data = await res.json()
    await get().loadSessions()
    const newId = data.session_id || data.sessionId
    if (newId) {
      set({ activeSessionId: newId })
      useChatStore.getState().setActiveSessionId(newId)
      await get().loadSessionMessages(newId)
    }
  },

  deleteSession: async (id) => {
    await fetch(`/api/sessions/${id}`, { method: 'DELETE' })
    const { sessions, activeSessionId } = get()
    const remaining = sessions.filter((s) => s.id !== id)
    const isDeletingActive = activeSessionId === id
    const nextId = isDeletingActive ? (remaining[0]?.id ?? null) : activeSessionId
    set({ sessions: remaining, activeSessionId: nextId })
    useChatStore.getState().setActiveSessionId(nextId)
    useChatStore.getState().removeSession(id)
    if (nextId) {
      await get().loadSessionMessages(nextId)
    }
  },

  loadSessions: async () => {
    set({ isLoading: true })
    try {
      const res = await fetch('/api/sessions')
      const data = await res.json()
      const sessions = (data.sessions || []).map(mapSession)
      const { activeSessionId } = get()
      set({ sessions, isLoading: false })
      if (!activeSessionId && sessions.length > 0) {
        await get().setActiveSession(sessions[0].id)
      }
    } catch {
      set({ isLoading: false })
    }
  },

  renameSession: async (id, title) => {
    await fetch(`/api/sessions/${id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title }),
    })
    set((state) => ({
      sessions: state.sessions.map((s) => (s.id === id ? { ...s, title } : s)),
    }))
  },

  loadSessionMessages: async (sessionId) => {
    const chatStore = useChatStore.getState()
    chatStore.clearSession(sessionId)
    try {
      const res = await fetch(`/api/sessions/${sessionId}/messages`)
      const data = await res.json()
      const rawMessages = data.messages || []
      for (const m of rawMessages) {
        chatStore.addMessage(sessionId, mapMessage(m))
      }
    } catch {
      // silently fail
    }
  },
}))