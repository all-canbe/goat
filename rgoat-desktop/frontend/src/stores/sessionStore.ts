import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";
import { useChatStore, type ChatMessage } from "./chatStore";
import { useChangesStore } from "./changesStore";
import { useWorkspaceStore } from "./workspaceStore";

export interface Session {
  id: string;
  title: string;
  message_count: number;
  created_at: string;
  parent_session_id?: string | null;
  forked_from_message_id?: number | null;
  workspace?: string | null;
}

export interface SessionMessage {
  id: number;
  role: string;
  content: string;
  tool_calls?: string | null;
  tool_call_id?: string | null;
}

/** 内部矫正/熔断提示：不作为用户可见历史展示（兼容旧脏数据） */
export function isInternalIntervention(content: string | undefined | null): boolean {
  if (!content) return false;
  return (
    content.includes("检测到重复调用") ||
    content.includes("前两步工具调用连续失败") ||
    content.includes("即将形成重复循环") ||
    content.includes("[replan]") ||
    content.includes("请重新评估当前计划") ||
    content.includes("可能陷入无效循环") ||
    content.includes("finish_reason=") ||
    content.includes("请立即调用工具") ||
    content.includes("请立刻调用工具") ||
    content.includes("最后一次警告") ||
    content.includes("Mid-Flow Review Findings") ||
    content.includes("[verify]")
  );
}

interface StoredToolCall {
  id?: string;
  type?: string;
  function?: {
    name?: string;
    arguments?: string;
  };
}

function parseToolArguments(raw: string | undefined): Record<string, unknown> {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
    return { value: parsed };
  } catch {
    return { raw };
  }
}

/**
 * 将 DB 会话消息映射为前端 ChatMessage 时间线：
 * - 过滤内部矫正提示
 * - assistant.tool_calls → tool_call 卡片
 * - role=tool → 合并到对应 tool_call 或独立 tool_result
 */
export function mapSessionMessagesToChat(messages: SessionMessage[]): ChatMessage[] {
  const result: ChatMessage[] = [];
  // tool_call_id → result 数组索引（type=tool_call）
  const toolIndexById = new Map<string, number>();

  for (const message of messages) {
    const role = message.role;
    const content = message.content ?? "";

    if (role === "user") {
      if (isInternalIntervention(content)) continue;
      result.push({
        id: String(message.id),
        type: "user",
        content,
        role,
      });
      continue;
    }

    if (role === "assistant") {
      // 有正文才渲染 assistant 气泡（纯 tool_calls 的 assistant 可无正文）
      if (content.trim()) {
        result.push({
          id: String(message.id),
          type: "assistant",
          content,
          role,
        });
      }

      if (message.tool_calls) {
        try {
          const calls = JSON.parse(message.tool_calls) as StoredToolCall[];
          if (Array.isArray(calls)) {
            for (const [i, call] of calls.entries()) {
              const toolCallId = call.id || `${message.id}-tc-${i}`;
              const toolName = call.function?.name || "unknown";
              const args = parseToolArguments(call.function?.arguments);
              const idx = result.length;
              result.push({
                id: `${message.id}-tool-${i}`,
                type: "tool_call",
                toolName,
                arguments: args,
                toolCallId,
                role: "assistant",
              });
              toolIndexById.set(toolCallId, idx);
            }
          }
        } catch {
          // tool_calls JSON 损坏时忽略，保留 assistant 正文
        }
      }
      continue;
    }

    if (role === "tool") {
      const toolCallId = message.tool_call_id || undefined;
      const isError = content.startsWith("[error] ");
      const output = isError ? content.slice("[error] ".length) : content;
      const success = !isError;

      if (toolCallId && toolIndexById.has(toolCallId)) {
        const idx = toolIndexById.get(toolCallId)!;
        const prev = result[idx];
        result[idx] = {
          ...prev,
          success,
          output,
        };
      } else {
        // 孤立 tool 行仍渲染为卡片，不当 system 灰字
        result.push({
          id: String(message.id),
          type: "tool_call",
          toolName: "tool",
          arguments: {},
          success,
          output,
          toolCallId,
          role,
        });
      }
      continue;
    }

    if (role === "system") {
      if (isInternalIntervention(content)) continue;
      result.push({
        id: String(message.id),
        type: "system",
        content,
        role,
      });
      continue;
    }

    // 未知 role：跳过，避免污染时间线
  }

  return result;
}

interface SessionState {
  sessions: Session[];
  activeSessionId: string | null;
  loading: boolean;
  /** 当前正在加载历史的会话 ID，用于 UI 展示加载态 */
  selectingId: string | null;
  /** 当前正在删除的会话 ID，用于 UI 展示加载态 */
  deletingId: string | null;
  loadSessions: () => Promise<void>;
  createSession: () => Promise<void>;
  selectSession: (id: string) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  renameSession: (id: string, title: string) => Promise<void>;
  setActiveSessionId: (id: string | null) => void;
  updateSessionMessageCount: (id: string) => void;
  forkSession: (sessionId: string, upToMessageId?: number) => Promise<void>;
}

export const useSessionStore = create<SessionState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  loading: false,
  selectingId: null,
  deletingId: null,

  loadSessions: async () => {
    set({ loading: true });
    try {
      const sessions = await tauriInvoke<Session[]>("get_sessions");
      set({ sessions, loading: false });
    } catch (err) {
      set({ loading: false });
      throw err;
    }
  },

  createSession: async () => {
    // 调用后端 create_session 命令，立即创建会话（归属当前 workspace）
    // 失败时抛出错误给 UI，不静默吞错
    const newSession = await tauriInvoke<Session>("create_session");
    // 预创建 chatStore 条目，避免首条消息/事件写入时无 sessions[id]
    useChatStore.getState().ensureSession(newSession.id);
    set((s) => ({
      sessions: [newSession, ...s.sessions],
      activeSessionId: newSession.id,
    }));
  },

  selectSession: async (id) => {
    set({ selectingId: id });
    try {
      // 后台仍在运行/有审批的会话：保留内存 live messages，只切换激活 ID
      // 避免 DB 历史覆盖 tool_call 卡片等未完整持久化的 UI 状态
      const existing = useChatStore.getState().sessions[id];
      if (
        existing &&
        (existing.isStreaming || !!existing.streamingContent || !!existing.pendingApproval)
      ) {
        // 对流式会话：检查目标 workspace 是否与当前 workspace 相同
        // 运行中 Agent 不能与 workspace 解绑，拒绝跨 workspace 切换
        const session = get().sessions.find((s) => s.id === id);
        const targetWs = session?.workspace ?? null;
        const currentWs = useWorkspaceStore.getState().workspace?.path ?? null;
        if (targetWs !== null && currentWs !== null && targetWs !== currentWs) {
          throw new Error("Cannot switch to a streaming session in a different workspace");
        }
        set({ activeSessionId: id });
        // 即便保留 live messages，也要联动文件树延迟切换到该会话 workspace
        useWorkspaceStore.getState().scheduleWorkspaceSwitch(session?.workspace ?? null);
        return;
      }

      const messages = await tauriInvoke<SessionMessage[]>("get_session_messages", {
        sessionId: id,
      });
      const chatMessages = mapSessionMessagesToChat(messages);
      // ★ P0 fix：先确保会话状态存在，再只更新 messages（replaceMessages 不再重置 streaming/stats）
      // 这样切换会话加载历史时，不会截断后台运行会话的流式输出
      useChatStore.getState().ensureSession(id);
      useChatStore.getState().replaceMessages(chatMessages, id);

      // 先切换 workspace，成功后再激活会话（避免延迟窗口内 InputPanel 用旧 workspace 发送）
      const session = get().sessions.find((s) => s.id === id);
      const switched = await useWorkspaceStore.getState().scheduleWorkspaceSwitch(
        session?.workspace ?? null,
      );
      if (!switched) {
        // 切换失败（被后续请求取代或后端拒绝），不改变 activeSessionId
        return;
      }
      set({ activeSessionId: id });
    } finally {
      set({ selectingId: null });
    }
  },

  deleteSession: async (id) => {
    set({ deletingId: id });
    try {
      // 先取消运行中 agent，避免删除后事件经 updateSession 复活会话条目
      try {
        await useChatStore.getState().cancelAgent(id);
      } catch {
        // 无运行中 agent 时忽略
      }
      await tauriInvoke("delete_session", { sessionId: id });
      const { activeSessionId } = get();
      set((s) => ({
        sessions: s.sessions.filter((sess) => sess.id !== id),
        activeSessionId: activeSessionId === id ? null : activeSessionId,
      }));
      // 清理 chatStore 中该会话的状态（messages/fileRefs/stats 等），避免内存泄漏
      useChatStore.getState().clearMessages(id);
      // 从 sessions Record 中删除该条目（clearMessages 重置为空状态，但条目仍存在，需显式删除）
      useChatStore.setState(() => {
        const sessions = { ...useChatStore.getState().sessions };
        delete sessions[id];
        return { sessions };
      });
      // 清理前端 changesStore，避免删除会话后内存泄漏
      useChangesStore.getState().clearChanges(id);
    } catch (err) {
      throw err;
    } finally {
      set({ deletingId: null });
    }
  },

  renameSession: async (id, title) => {
    try {
      await tauriInvoke("rename_session", { sessionId: id, title });
      set((s) => ({
        sessions: s.sessions.map((sess) =>
          sess.id === id ? { ...sess, title } : sess
        ),
      }));
    } catch (err) {
      throw err;
    }
  },

  setActiveSessionId: (id) => set({ activeSessionId: id }),

  updateSessionMessageCount: (id) => {
    set((s) => ({
      sessions: s.sessions.map((sess) =>
        sess.id === id ? { ...sess, message_count: sess.message_count + 1 } : sess
      ),
    }));
  },

  forkSession: async (sessionId, upToMessageId) => {
    try {
      const newSession = await tauriInvoke<Session>("fork_session", {
        sessionId,
        upToMessageId: upToMessageId ?? null,
      });
      // 把新会话加到 sessions 列表头部（最新的在前），并切换到新会话
      // 不在此处 clearMessages，由调用方负责（与 createSession 行为一致）
      set((s) => ({
        sessions: [newSession, ...s.sessions],
        activeSessionId: newSession.id,
      }));
    } catch (err) {
      // 抛出错误让调用方处理 UX 反馈（不再 silently fail）
      throw err;
    }
  },
}));
