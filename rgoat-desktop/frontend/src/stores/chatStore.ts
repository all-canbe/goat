import { create } from "zustand";
import { generateId } from "../lib/utils";
import { tauriInvoke } from "../lib/tauri-bridge";
import { useSessionStore } from "./sessionStore";

export interface ChatMessage {
  id: string;
  type: "user" | "assistant" | "thought" | "tool_call" | "tool_result" | "system" | "error";
  content?: string;
  toolName?: string;
  arguments?: Record<string, unknown>;
  success?: boolean;
  output?: string;
  role?: string;
  /** 关联 DB tool_call_id，用于历史加载时合并 tool 结果 */
  toolCallId?: string;
}

// Task 3: 文件引用
export interface FileRef {
  id: string;
  name: string;
  path: string;
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

// ── 多会话：每个会话独立的聊天状态（仅数据，不含 action）──
export interface SessionChatState {
  messages: ChatMessage[];
  isStreaming: boolean;
  streamingContent: string;
  pendingApproval: ApprovalData | null;
  // D1-T07: 统计计数
  toolCallCount: number;
  pendingApprovals: number;
  // P0-2: Token 统计
  tokenUsage: TokenUsage;
  // Task 3: 文件引用
  fileRefs: FileRef[];
  // P1: 上下文压缩通知
  compactionNotices: CompactionNotice[];
  // P1: Plan Mode 计划文档预览（Modal 控制）
  planContent: string;
}

/** 创建一个空的会话状态（每次返回新引用） */
export function createEmptySession(): SessionChatState {
  return {
    messages: [],
    isStreaming: false,
    streamingContent: "",
    pendingApproval: null,
    toolCallCount: 0,
    pendingApprovals: 0,
    tokenUsage: { inputTokens: 0, outputTokens: 0, totalCost: 0 },
    fileRefs: [],
    compactionNotices: [],
    planContent: "",
  };
}

/** 无激活会话时返回的稳定空引用（避免每次渲染创建新对象） */
const EMPTY_SESSION: SessionChatState = createEmptySession();

interface ChatState {
  // 多会话：session_id → 该会话的聊天状态
  sessions: Record<string, SessionChatState>;
  // 全局字段（不随会话切换）
  // P1: Empty State 示例提问填入输入框（InputPanel 订阅）
  draft: string;
  setDraft: (text: string) => void;
  // P2: 全局快捷键聚焦输入框触发器（InputPanel 监听变化后聚焦）
  focusInputTrigger: number;
  triggerFocusInput: () => void;

  // ── per-session action（接受可选 sid，默认用激活会话）──
  ensureSession: (sid: string) => void;
  addUserMessage: (text: string, sid?: string) => string;
  appendThought: (text: string, sid?: string) => void;
  addToolCall: (toolName: string, args: Record<string, unknown>, sid?: string) => string;
  updateToolResult: (toolName: string, success: boolean, output: string, sid?: string) => void;
  commitStream: (sid?: string) => boolean;
  addSystemMessage: (text: string, sid?: string) => void;
  addErrorMessage: (text: string, sid?: string) => void;
  addAssistantMessage: (text: string, sid?: string) => void;
  clearMessages: (sid?: string) => void;
  // ★ P0 fix：replaceMessages 只更新 messages，不重置 streaming/stats
  replaceMessages: (messages: ChatMessage[], sid?: string) => void;
  setStreaming: (v: boolean, sid?: string) => void;
  setApproval: (data: ApprovalData | null, sid?: string) => void;
  // D1-T05: 统一审批响应入口
  respondApproval: (approved: boolean, scope: ApprovalScope, sid?: string) => Promise<void>;
  // D1-T07: stats 操作
  incrementPendingApprovals: (sid?: string) => void;
  decrementPendingApprovals: (sid?: string) => void;
  resetStats: (sid?: string) => void;
  // P0-2: Token 用量累积
  addUsage: (inputTokens: number, outputTokens: number, sid?: string) => void;
  // P0-1: 取消 Agent
  cancelAgent: (sid?: string) => Promise<void>;
  // Task 3: 文件引用
  addFileRef: (file: { name: string; path: string }, sid?: string) => void;
  removeFileRef: (id: string, sid?: string) => void;
  clearFileRefs: (sid?: string) => void;
  // P1: 上下文压缩通知
  addCompactionNotice: (before: number, after: number, sid?: string) => void;
  dismissCompactionNotice: (id: string, sid?: string) => void;
  // P1: Plan Mode 计划文档
  setPlanContent: (content: string, sid?: string) => void;
  clearPlanContent: (sid?: string) => void;
}

/** 解析 sid：优先用显式参数，否则取激活会话 */
function resolveSid(sid?: string): string | null {
  return sid ?? useSessionStore.getState().activeSessionId ?? null;
}

/** 更新指定会话的状态（若不存在则创建） */
function updateSession(
  s: ChatState,
  id: string,
  updater: (session: SessionChatState) => SessionChatState,
): Partial<ChatState> {
  const session = s.sessions[id] ?? createEmptySession();
  return {
    sessions: {
      ...s.sessions,
      [id]: updater(session),
    },
  };
}

export const useChatStore = create<ChatState>((set, get) => ({
  sessions: {},

  // P1: 设置输入框草稿（Empty State 示例提问点击调用）
  draft: "",
  setDraft: (text) => set({ draft: text }),
  // P2: 全局快捷键聚焦输入框（递增计数器触发 InputPanel useEffect）
  focusInputTrigger: 0,
  triggerFocusInput: () => set((s) => ({ focusInputTrigger: s.focusInputTrigger + 1 })),

  ensureSession: (sid) => {
    set((s) => {
      if (s.sessions[sid]) return s;
      return {
        sessions: { ...s.sessions, [sid]: createEmptySession() },
      };
    });
  },

  addUserMessage: (text, sid) => {
    const id = resolveSid(sid);
    if (!id) return "";
    const msgId = generateId();
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        messages: [...sess.messages, { id: msgId, type: "user", content: text }],
      })),
    );
    return msgId;
  },

  appendThought: (text, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        streamingContent: sess.streamingContent + text,
      })),
    );
  },

  addToolCall: (toolName, args, sid) => {
    const id = resolveSid(sid);
    if (!id) return "";
    const msgId = generateId();
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        messages: [...sess.messages, { id: msgId, type: "tool_call", toolName, arguments: args }],
        toolCallCount: sess.toolCallCount + 1,
      })),
    );
    return msgId;
  },

  updateToolResult: (toolName, success, output, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        messages: sess.messages.map((m) => {
          if (m.type === "tool_call" && m.toolName === toolName && m.success === undefined) {
            return { ...m, success, output };
          }
          return m;
        }),
      })),
    );
  },

  commitStream: (sid) => {
    const id = resolveSid(sid);
    if (!id) return false;
    const session = get().sessions[id];
    if (!session || !session.streamingContent) return false;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        messages: [
          ...sess.messages,
          { id: generateId(), type: "assistant", content: sess.streamingContent },
        ],
        streamingContent: "",
        isStreaming: false,
      })),
    );
    return true;
  },

  addSystemMessage: (text, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        messages: [...sess.messages, { id: generateId(), type: "system", content: text }],
      })),
    );
  },

  addErrorMessage: (text, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        messages: [...sess.messages, { id: generateId(), type: "error", content: text }],
      })),
    );
  },

  addAssistantMessage: (text, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        messages: [...sess.messages, { id: generateId(), type: "assistant", content: text }],
      })),
    );
  },

  clearMessages: (sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) => ({
      sessions: { ...s.sessions, [id]: createEmptySession() },
    }));
  },

  // ★ P0 fix：replaceMessages 只更新 messages，不再重置 streaming/stats
  // 这样切换会话加载历史时，不会截断后台运行会话的流式输出
  // fileRefs 为 per-session 字段，加载历史时保留，避免切走再切回丢失未发送引用
  replaceMessages: (messages, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        messages,
      })),
    );
  },

  setStreaming: (v, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) => updateSession(s, id, (sess) => ({ ...sess, isStreaming: v })));
  },

  setApproval: (data, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => {
        if (data) {
          // 已有审批时只覆盖内容，不重复累加计数（避免 approval_required 重入导致虚高）
          const pendingApprovals = sess.pendingApproval
            ? sess.pendingApprovals
            : sess.pendingApprovals + 1;
          return { ...sess, pendingApproval: data, pendingApprovals };
        }
        // 仅在确实有 pending 时减计数，避免二次清空导致负数/错位
        return {
          ...sess,
          pendingApproval: null,
          pendingApprovals: sess.pendingApproval
            ? Math.max(sess.pendingApprovals - 1, 0)
            : sess.pendingApprovals,
        };
      }),
    );
  },

  // D1-T05: 调用 respond_approval IPC，成功后关闭弹窗（多会话：传 session_id）
  respondApproval: async (approved, scope, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    const session = get().sessions[id];
    if (!session?.pendingApproval) return;
    const toolName = session.pendingApproval.tool_name;
    // 顶层 sessionId 用 camelCase；嵌套 response 字段按 serde 保持 snake_case
    await tauriInvoke("respond_approval", {
      response: {
        tool_name: toolName,
        approved,
        scope,
      },
      sessionId: id,
    });
    get().setApproval(null, id);
  },

  incrementPendingApprovals: (sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) => updateSession(s, id, (sess) => ({ ...sess, pendingApprovals: sess.pendingApprovals + 1 })));
  },

  decrementPendingApprovals: (sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({ ...sess, pendingApprovals: Math.max(sess.pendingApprovals - 1, 0) })),
    );
  },

  resetStats: (sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        toolCallCount: 0,
        pendingApprovals: 0,
        tokenUsage: { inputTokens: 0, outputTokens: 0, totalCost: 0 },
      })),
    );
  },

  // P0-2: 累积 Token 用量（粗略成本估算：input $3/M, output $15/M，对标 Claude Sonnet 价位）
  addUsage: (inputTokens, outputTokens, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    const cost = (inputTokens / 1_000_000) * 3 + (outputTokens / 1_000_000) * 15;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        tokenUsage: {
          inputTokens: sess.tokenUsage.inputTokens + inputTokens,
          outputTokens: sess.tokenUsage.outputTokens + outputTokens,
          totalCost: sess.tokenUsage.totalCost + cost,
        },
      })),
    );
  },

  // P0-1: 取消 Agent（多会话：传 session_id）
  cancelAgent: async (sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    try {
      await tauriInvoke("cancel_agent", { sessionId: id });
      set((s) => updateSession(s, id, (sess) => ({ ...sess, isStreaming: false, streamingContent: "" })));
    } catch (err) {
      console.error("[chatStore] cancelAgent failed:", err);
    }
  },

  // Task 3: 文件引用
  addFileRef: (file, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => {
        if (sess.fileRefs.some((r) => r.path === file.path)) return sess;
        return {
          ...sess,
          fileRefs: [...sess.fileRefs, { id: generateId(), name: file.name, path: file.path }],
        };
      }),
    );
  },

  removeFileRef: (refId, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        fileRefs: sess.fileRefs.filter((r) => r.id !== refId),
      })),
    );
  },

  clearFileRefs: (sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) => updateSession(s, id, (sess) => ({ ...sess, fileRefs: [] })));
  },

  // P1: 上下文压缩通知（仅保留最新一条，新通知替换旧通知）
  addCompactionNotice: (before, after, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        compactionNotices: [
          {
            id: generateId(),
            before,
            after,
            timestamp: Date.now(),
          },
        ],
      })),
    );
  },

  dismissCompactionNotice: (noticeId, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) =>
      updateSession(s, id, (sess) => ({
        ...sess,
        compactionNotices: sess.compactionNotices.filter((n) => n.id !== noticeId),
      })),
    );
  },

  // P1: Plan Mode 计划文档（空字符串表示不显示 Modal）
  setPlanContent: (content, sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) => updateSession(s, id, (sess) => ({ ...sess, planContent: content })));
  },

  clearPlanContent: (sid) => {
    const id = resolveSid(sid);
    if (!id) return;
    set((s) => updateSession(s, id, (sess) => ({ ...sess, planContent: "" })));
  },
}));

/**
 * 订阅当前激活会话的聊天状态。
 * - 无激活会话时返回模块级 EMPTY_SESSION（引用稳定，不触发重渲染）
 * - sessions[sid] 引用仅在该会话更新时变化，其他会话更新不影响当前组件
 */
export function useActiveSessionState(): SessionChatState {
  const sid = useSessionStore((s) => s.activeSessionId);
  return useChatStore((s) => (sid ? (s.sessions[sid] ?? EMPTY_SESSION) : EMPTY_SESSION));
}
