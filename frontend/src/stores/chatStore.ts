import { create } from 'zustand'
import type { Message, PendingApproval, PendingQuestion, ToolCall } from '@/types'

interface SessionChatState {
  messages: Message[]
  streamingContent: string
  isStreaming: boolean
  pendingApproval: PendingApproval | null
  pendingQuestion: PendingQuestion | null
  pendingToolCalls: ToolCall[]
}

function getInitialState(): SessionChatState {
  return {
    messages: [],
    streamingContent: '',
    isStreaming: false,
    pendingApproval: null,
    pendingQuestion: null,
    pendingToolCalls: [],
  }
}

function ensureState(state: Record<string, SessionChatState>, sid: string): SessionChatState {
  const existing = state[sid]
  if (!existing) {
    state[sid] = getInitialState()
    return state[sid]
  }
  return existing
}

interface ChatStore {
  sessions: Record<string, SessionChatState>
  activeSessionId: string | null

  getActiveState: () => SessionChatState | undefined
  setActiveSessionId: (id: string | null) => void
  ensureSession: (sessionId: string) => SessionChatState

  addMessage: (sessionId: string, msg: Message) => void
  appendStreamToken: (sessionId: string, token: string) => void
  commitStream: (sessionId: string, messageId: string) => void
  clearStream: (sessionId: string) => void
  clearSession: (sessionId: string) => void
  removeSession: (sessionId: string) => void

  setPendingApproval: (sessionId: string, approval: PendingApproval | null) => void
  setPendingQuestion: (sessionId: string, question: PendingQuestion | null) => void
  addToolCall: (sessionId: string, toolCall: ToolCall) => void
  clearToolCalls: (sessionId: string) => void

  searchQuery: string
  searchResults: number[]
  activeSearchIndex: number
  isSearchActive: boolean
  setSearchQuery: (query: string) => void
  clearSearch: () => void
  toggleSearch: () => void
  navigateSearch: (direction: 'next' | 'prev') => void
}

export const useChatStore = create<ChatStore>((set, get) => ({
  sessions: {},
  activeSessionId: null,

  getActiveState: () => {
    const { sessions, activeSessionId } = get()
    return activeSessionId ? sessions[activeSessionId] : undefined
  },

  setActiveSessionId: (id) => set({ activeSessionId: id }),

  ensureSession: (sessionId) => {
    const { sessions } = get()
    const updated = { ...sessions }
    ensureState(updated, sessionId)
    set({ sessions: updated })
    return updated[sessionId]
  },

  addMessage: (sessionId, msg) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, messages: [...s.messages, msg] }
      return { sessions: updated }
    }),

  appendStreamToken: (sessionId, token) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, streamingContent: s.streamingContent + token, isStreaming: true }
      return { sessions: updated }
    }),

  commitStream: (sessionId, messageId) => {
    const { sessions } = get()
    const s = sessions[sessionId]
    if (!s || (!s.streamingContent && !s.isStreaming)) return
    const content = s.streamingContent || '(空回复)'
    const toolCalls = s.pendingToolCalls.length > 0 ? s.pendingToolCalls : undefined
    const msg: Message = {
      id: messageId || crypto.randomUUID(),
      role: 'assistant',
      content,
      toolCalls,
      timestamp: Date.now(),
    }
    set((state) => {
      const updated = { ...state.sessions }
      updated[sessionId] = {
        messages: [...(updated[sessionId]?.messages ?? []), msg],
        streamingContent: '',
        isStreaming: false,
        pendingApproval: null,
        pendingQuestion: null,
        pendingToolCalls: [],
      }
      return { sessions: updated }
    })
  },

  clearStream: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = updated[sessionId]
      if (s) {
        updated[sessionId] = { ...s, streamingContent: '', isStreaming: false }
      }
      return { sessions: updated }
    }),

  clearSession: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      updated[sessionId] = getInitialState()
      return { sessions: updated }
    }),

  removeSession: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      delete updated[sessionId]
      return { sessions: updated }
    }),

  setPendingApproval: (sessionId, approval) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, pendingApproval: approval }
      return { sessions: updated }
    }),

  setPendingQuestion: (sessionId, question) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, pendingQuestion: question }
      return { sessions: updated }
    }),

  addToolCall: (sessionId, toolCall) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, pendingToolCalls: [...s.pendingToolCalls, toolCall] }
      return { sessions: updated }
    }),

  clearToolCalls: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, pendingToolCalls: [] }
      return { sessions: updated }
    }),

  searchQuery: '',
  searchResults: [],
  activeSearchIndex: -1,
  isSearchActive: false,

  setSearchQuery: (query) => {
    const { sessions, activeSessionId } = get()
    const s = activeSessionId ? sessions[activeSessionId] : undefined
    const messages = s?.messages ?? []
    const lower = query.toLowerCase()
    const results = query.trim()
      ? messages
          .map((msg, i) => ({ content: msg.content.toLowerCase(), i }))
          .filter(({ content }) => content.includes(lower))
          .map(({ i }) => i)
      : []
    set({
      searchQuery: query,
      searchResults: results,
      activeSearchIndex: results.length > 0 ? 0 : -1,
    })
  },

  clearSearch: () => set({
    searchQuery: '',
    searchResults: [],
    activeSearchIndex: -1,
    isSearchActive: false,
  }),

  toggleSearch: () => set((state) => ({
    isSearchActive: !state.isSearchActive,
    searchQuery: '',
    searchResults: [],
    activeSearchIndex: -1,
  })),

  navigateSearch: (direction) => {
    const { searchResults, activeSearchIndex } = get()
    if (searchResults.length === 0) return
    const delta = direction === 'next' ? 1 : -1
    const next = (activeSearchIndex + delta + searchResults.length) % searchResults.length
    set({ activeSearchIndex: next })
  },
}))
