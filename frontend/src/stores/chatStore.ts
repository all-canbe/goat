import { create } from 'zustand'
import type { Message, PendingApproval, PendingQuestion, PendingPlanCompare, PendingPlanComplete, ToolCall, FileAttachment } from '@/types'

interface SessionChatState {
  messages: Message[]
  streamingContent: string
  isStreaming: boolean
  streamError: string | null
  lastUserText: string | null
  lastAttachments: FileAttachment[]
  pendingApproval: PendingApproval | null
  pendingQuestion: PendingQuestion | null
  pendingPlanCompare: PendingPlanCompare | null
  pendingPlanComplete: PendingPlanComplete | null
  pendingToolCalls: ToolCall[]
}

function getInitialState(): SessionChatState {
  return {
    messages: [],
    streamingContent: '',
    isStreaming: false,
    streamError: null,
    lastUserText: null,
    lastAttachments: [],
    pendingApproval: null,
    pendingQuestion: null,
    pendingPlanCompare: null,
    pendingPlanComplete: null,
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
  startStream: (sessionId: string) => void
  commitStream: (sessionId: string, messageId: string, contentOverride?: string) => void
  clearStream: (sessionId: string) => void
  clearSession: (sessionId: string) => void
  clearSessionKeepStream: (sessionId: string) => void
  removeSession: (sessionId: string) => void

  setPendingApproval: (sessionId: string, approval: PendingApproval | null) => void
  setPendingQuestion: (sessionId: string, question: PendingQuestion | null) => void
  setPendingPlanCompare: (sessionId: string, planCompare: PendingPlanCompare | null) => void
  setPendingPlanComplete: (sessionId: string, planComplete: PendingPlanComplete | null) => void
  addToolCall: (sessionId: string, toolCall: ToolCall) => void
  clearToolCalls: (sessionId: string) => void

  setStreamError: (sessionId: string, error: string | null) => void
  clearStreamError: (sessionId: string) => void
  setLastUserInput: (sessionId: string, text: string, attachments: FileAttachment[]) => void

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
      // 空 token 不应启动流式占位(避免假启动);保留现有 streamingContent
      if (!token) return { sessions: updated }
      // 若 streamingContent 刚被 commitStream 清空且 isStreaming 已 false,
      // 说明该 token 属于已完成轮次,跳过避免虚假重新激活流式状态
      if (!s.isStreaming && s.streamingContent === '') return { sessions: updated }
      updated[sessionId] = { ...s, streamingContent: s.streamingContent + token, isStreaming: true }
      return { sessions: updated }
    }),

  startStream: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, isStreaming: true, streamError: null }
      return { sessions: updated }
    }),

  commitStream: (sessionId, messageId, contentOverride) => {
    const { sessions } = get()
    const s = sessions[sessionId]
    // 仅在 session 完全不存在时返回;即使内容空/未 streaming,后端主动 chat.response 仍要处理
    if (!s) return
    // 优先使用 override(后端 chat.response 直接带来的完整 content),否则用 streamingContent
    const content = (s.streamingContent && s.streamingContent.trim())
      ? s.streamingContent
      : (contentOverride && contentOverride.trim())
        ? contentOverride
        : (s.streamingContent || '(空回复)')
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
        streamError: null,
        lastUserText: null,
        lastAttachments: [],
        pendingApproval: null,
        pendingQuestion: null,
        pendingPlanCompare: null,
        pendingPlanComplete: null,
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
        updated[sessionId] = { ...s, streamingContent: '', isStreaming: false, streamError: null, lastUserText: null, lastAttachments: [] }
      }
      return { sessions: updated }
    }),

  clearSession: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      updated[sessionId] = getInitialState()
      return { sessions: updated }
    }),

  clearSessionKeepStream: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = updated[sessionId]
      if (s) {
        updated[sessionId] = { ...s, messages: [] }
      } else {
        updated[sessionId] = { ...getInitialState(), messages: [] }
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

  setPendingPlanCompare: (sessionId, planCompare) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, pendingPlanCompare: planCompare }
      return { sessions: updated }
    }),

  setPendingPlanComplete: (sessionId, planComplete) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, pendingPlanComplete: planComplete }
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

  setStreamError: (sessionId, error) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, streamError: error, isStreaming: error !== null ? true : s.isStreaming, streamingContent: error !== null ? '' : s.streamingContent }
      return { sessions: updated }
    }),

  clearStreamError: (sessionId) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = updated[sessionId]
      if (s) {
        updated[sessionId] = { ...s, streamError: null }
      }
      return { sessions: updated }
    }),

  setLastUserInput: (sessionId, text, attachments) =>
    set((state) => {
      const updated = { ...state.sessions }
      const s = ensureState(updated, sessionId)
      updated[sessionId] = { ...s, lastUserText: text, lastAttachments: attachments }
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
