import { create } from "zustand";
import { generateId } from "../lib/utils";

export interface ChatMessage {
  id: string;
  type: "user" | "assistant" | "thought" | "tool_call" | "tool_result" | "system" | "error";
  content?: string;
  toolName?: string;
  arguments?: Record<string, unknown>;
  success?: boolean;
  output?: string;
  role?: string;
}

export interface ApprovalData {
  tool_name: string;
  decision: string;
  message: string;
  arguments?: Record<string, unknown>;
}

interface ChatState {
  messages: ChatMessage[];
  isStreaming: boolean;
  streamingContent: string;
  pendingApproval: ApprovalData | null;
  addUserMessage: (text: string) => string;
  appendThought: (text: string) => void;
  addToolCall: (toolName: string, args: Record<string, unknown>) => string;
  updateToolResult: (toolName: string, success: boolean, output: string) => void;
  commitStream: () => void;
  addSystemMessage: (text: string) => void;
  addErrorMessage: (text: string) => void;
  addAssistantMessage: (text: string) => void;
  clearMessages: () => void;
  setStreaming: (v: boolean) => void;
  setApproval: (data: ApprovalData | null) => void;
}

export const useChatStore = create<ChatState>((set, get) => ({
  messages: [],
  isStreaming: false,
  streamingContent: "",
  pendingApproval: null,

  addUserMessage: (text) => {
    const id = generateId();
    set((s) => ({
      messages: [...s.messages, { id, type: "user", content: text }],
    }));
    return id;
  },

  appendThought: (text) => {
    set((s) => ({
      streamingContent: s.streamingContent + text,
    }));
  },

  addToolCall: (toolName, args) => {
    const id = generateId();
    set((s) => ({
      messages: [...s.messages, { id, type: "tool_call", toolName, arguments: args }],
    }));
    return id;
  },

  updateToolResult: (toolName, success, output) => {
    set((s) => ({
      messages: s.messages.map((m) => {
        if (m.type === "tool_call" && m.toolName === toolName && m.success === undefined) {
          return { ...m, success, output };
        }
        return m;
      }),
    }));
  },

  commitStream: () => {
    const { streamingContent } = get();
    if (streamingContent) {
      set((s) => ({
        messages: [
          ...s.messages,
          { id: generateId(), type: "assistant", content: streamingContent },
        ],
        streamingContent: "",
        isStreaming: false,
      }));
    }
  },

  addSystemMessage: (text) => {
    set((s) => ({
      messages: [...s.messages, { id: generateId(), type: "system", content: text }],
    }));
  },

  addErrorMessage: (text) => {
    set((s) => ({
      messages: [...s.messages, { id: generateId(), type: "error", content: text }],
    }));
  },

  addAssistantMessage: (text) => {
    set((s) => ({
      messages: [...s.messages, { id: generateId(), type: "assistant", content: text }],
    }));
  },

  clearMessages: () => {
    set({ messages: [], streamingContent: "", isStreaming: false });
  },

  setStreaming: (v) => set({ isStreaming: v }),

  setApproval: (data) => set({ pendingApproval: data }),
}));
