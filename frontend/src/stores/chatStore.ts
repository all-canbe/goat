import { create } from 'zustand'
import type { Message, PendingApproval, PendingQuestion } from '@/types'

interface SessionChatState {
  messages: Message[]
  streamingContent: string
  isStreaming: boolean
  pendingApproval: PendingApproval | null
  pendingQuestion: PendingQuestion | null
}

function ensureState(state: Record<string, SessionChatState>, sid: string): SessionChatState {
  if (!state[sid]) {
    state[sid] = {
      messages: [],
      streamingContent: '',
      isStreaming: false,
      pendingApproval: null,
      pendingQuestion: null,
    }
  }
  return state[sid]
}

interface ChatStore {
  sessions: Record<string, SessionChatState>
  activeSessionId: string | null

  getActiveState: () => SessionChatState | undefined
  ensureSession: (sessionId: string) => SessionChatState

  addMessage: (sessionId: string, msg: Message) => void
  appendStreamToken: (sessionId: string, token: string) => void
  commitStream: (sessionId: string, messageId: string) => void
  clearStream: (sessionId: string) => void
  clearSession: (sessionId: string) => void
  removeSession: (sessionId: string) => void

  setPendingApproval: (sessionId: string, approval: PendingApproval | null) => void
  setPendingQuestion: (sessionId: string, question: PendingQuestion | null) => void

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

  ensureSession: (sessionId) => {
    const { sessions } = get()
    const updated = { ...sessions }
    const state = ensureState(updated, sessionId)
    set({ sessions: updated })
    return state
  },

  addMessage: (sessionId, msg) =>
    set((state) => {
      const updated = { ...state.sessions }
      ensureState(updated, sessionId).messages = [...updated[sessionId].messages, msg]
      return { sessions: updated }
    }),

  appendStreamToken: (sessionId, token) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      s.streamingContent += token
      s.isStreaming = true
      return { sessions: updated }
    }),

  commitStream: (sessionId, messageId) => {
    const { sessions } = get()
    const s = sessions[sessionId]
    if (!s || !s.streamingContent) return
    const msg: Message = {
      id: messageId || crypto.randomUUID(),
      role: 'assistant',
      content: s.streamingContent,
      timestamp: Date.now(),
    }
    const updated = { ...sessions }
    const us = ensureState(updated, sessionId)
    us.messages = [...us.messages, msg]
    us.streamingContent = ''
    us.isStreaming = false
    set({ sessions: updated })
  },

  clearStream: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      if (updated[sessionId]) {
        updated[sessionId] = { ...updated[sessionId], streamingContent: '', isStreaming: false }
      }
      return { sessions: updated }
    }),

  clearSession: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      updated[sessionId] = {
        messages: [],
        streamingContent: '',
        isStreaming: false,
        pendingApproval: null,
        pendingQuestion: null,
      }
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
      ensureState(updated, sessionId).pendingApproval = approval
      return { sessions: updated }
    }),

  setPendingQuestion: (sessionId, question) =>
    set((state) => {
      const updated = { ...state.sessions }
      ensureState(updated, sessionId).pendingQuestion = question
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