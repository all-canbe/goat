import { create } from "zustand";
import { generateId } from "../lib/utils";
import { tauriInvoke } from "../lib/tauri-bridge";

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

// D1-T05: ApprovalData 扩展 — 与 rgoat-core AgentEvent::ApprovalRequired 字段对齐
export interface ApprovalData {
  tool_name: string;
  tool_type: string; // "Shell" | "Write" | "Network" | "other"
  summary: string;
  risk_level: string; // "LOW" | "MEDIUM" | "HIGH"
  danger_score: number; // 0-100
  command?: string;
  path?: string;
  url?: string;
  diff?: string;
  affected_files: string[];
  allow_options: string[]; // ["once", "session", "all_similar", "always"]
  arguments?: Record<string, unknown>;
  // 兼容旧字段（保留）
  decision?: string;
  message?: string;
}

export type ApprovalScope = "once" | "session" | "all_similar" | "always";

// P0-2: Token 用量统计
export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  /** 累计估算成本（USD），基于粗略单价 */
  totalCost: number;
}

// P1: 上下文压缩通知
export interface CompactionNotice {
  id: string;
  before: number;
  after: number;
  timestamp: number;
}

interface ChatState {
  messages: ChatMessage[];
  isStreaming: boolean;
  streamingContent: string;
  pendingApproval: ApprovalData | null;
  // D1-T07: 统计计数
  toolCallCount: number;
  pendingApprovals: number;
  // P0-2: Token 统计
  tokenUsage: TokenUsage;
  // P1: Empty State 示例提问填入输入框（InputPanel 订阅）
  draft: string;
  setDraft: (text: string) => void;
  // P2: 全局快捷键聚焦输入框触发器（InputPanel 监听变化后聚焦）
  focusInputTrigger: number;
  triggerFocusInput: () => void;
  // P1: 上下文压缩通知
  compactionNotices: CompactionNotice[];
  addCompactionNotice: (before: number, after: number) => void;
  dismissCompactionNotice: (id: string) => void;
  // P1: Plan Mode 计划文档预览（Modal 控制）
  planContent: string;
  setPlanContent: (content: string) => void;
  clearPlanContent: () => void;
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
  // D1-T05: 统一审批响应入口
  respondApproval: (approved: boolean, scope: ApprovalScope) => Promise<void>;
  // D1-T07: stats 操作
  incrementPendingApprovals: () => void;
  decrementPendingApprovals: () => void;
  resetStats: () => void;
  // P0-2: Token 用量累积
  addUsage: (inputTokens: number, outputTokens: number) => void;
  // P0-1: 取消 Agent
  cancelAgent: () => Promise<void>;
}

export const useChatStore = create<ChatState>((set, get) => ({
  messages: [],
  isStreaming: false,
  streamingContent: "",
  pendingApproval: null,
  toolCallCount: 0,
  pendingApprovals: 0,
  tokenUsage: { inputTokens: 0, outputTokens: 0, totalCost: 0 },
  draft: "",

  // P1: 设置输入框草稿（Empty State 示例提问点击调用）
  setDraft: (text) => set({ draft: text }),

  // P2: 全局快捷键聚焦输入框（递增计数器触发 InputPanel useEffect）
  focusInputTrigger: 0,
  triggerFocusInput: () => set((s) => ({ focusInputTrigger: s.focusInputTrigger + 1 })),

  // P1: 上下文压缩通知（仅保留最新一条，新通知替换旧通知）
  compactionNotices: [],
  addCompactionNotice: (before, after) =>
    set(() => ({
      compactionNotices: [
        {
          id: generateId(),
          before,
          after,
          timestamp: Date.now(),
        },
      ],
    })),
  dismissCompactionNotice: (id) =>
    set((s) => ({
      compactionNotices: s.compactionNotices.filter((n) => n.id !== id),
    })),

  // P1: Plan Mode 计划文档（空字符串表示不显示 Modal）
  planContent: "",
  setPlanContent: (content) => set({ planContent: content }),
  clearPlanContent: () => set({ planContent: "" }),

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
      toolCallCount: s.toolCallCount + 1,
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
    set({ messages: [], streamingContent: "", isStreaming: false, toolCallCount: 0, pendingApprovals: 0, tokenUsage: { inputTokens: 0, outputTokens: 0, totalCost: 0 } });
  },

  setStreaming: (v) => set({ isStreaming: v }),

  setApproval: (data) =>
    set((s) => ({
      pendingApproval: data,
      // 进入审批弹窗时 pendingApprovals 计数 +1；清空时 -1（不小于 0）
      pendingApprovals: data ? Math.max(s.pendingApprovals + 1, 0) : Math.max(s.pendingApprovals - 1, 0),
    })),

  // D1-T05: 调用 respond_approval IPC，成功后关闭弹窗
  respondApproval: async (approved, scope) => {
    const { pendingApproval, setApproval } = get();
    if (!pendingApproval) return;
    await tauriInvoke("respond_approval", {
      tool_name: pendingApproval.tool_name,
      approved,
      scope,
    });
    setApproval(null);
  },

  incrementPendingApprovals: () =>
    set((s) => ({ pendingApprovals: s.pendingApprovals + 1 })),

  decrementPendingApprovals: () =>
    set((s) => ({ pendingApprovals: Math.max(s.pendingApprovals - 1, 0) })),

  resetStats: () => set({ toolCallCount: 0, pendingApprovals: 0, tokenUsage: { inputTokens: 0, outputTokens: 0, totalCost: 0 } }),

  // P0-2: 累积 Token 用量（粗略成本估算：input $3/M, output $15/M，对标 Claude Sonnet 价位）
  addUsage: (inputTokens, outputTokens) => {
    const cost = (inputTokens / 1_000_000) * 3 + (outputTokens / 1_000_000) * 15;
    set((s) => ({
      tokenUsage: {
        inputTokens: s.tokenUsage.inputTokens + inputTokens,
        outputTokens: s.tokenUsage.outputTokens + outputTokens,
        totalCost: s.tokenUsage.totalCost + cost,
      },
    }));
  },

  // P0-1: 取消 Agent 执行
  cancelAgent: async () => {
    try {
      await tauriInvoke("cancel_agent");
    } catch (err) {
      console.error("[chatStore] cancelAgent failed:", err);
    }
  },
}));
