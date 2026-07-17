import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";

export interface Session {
  id: string;
  title: string;
  message_count: number;
  created_at: string;
  parent_session_id?: string | null;
  forked_from_message_id?: number | null;
}

interface SessionState {
  sessions: Session[];
  activeSessionId: string | null;
  loading: boolean;
  loadSessions: () => Promise<void>;
  createSession: () => void;
  selectSession: (id: string) => void;
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

  loadSessions: async () => {
    set({ loading: true });
    try {
      const sessions = await tauriInvoke<Session[]>("get_sessions");
      set({ sessions, loading: false });
    } catch {
      set({ loading: false });
    }
  },

  createSession: () => {
    set({ activeSessionId: null });
  },

  selectSession: (id) => {
    set({ activeSessionId: id });
  },

  deleteSession: async (id) => {
    try {
      await tauriInvoke("delete_session", { session_id: id });
      const { activeSessionId } = get();
      set((s) => ({
        sessions: s.sessions.filter((sess) => sess.id !== id),
        activeSessionId: activeSessionId === id ? null : activeSessionId,
      }));
    } catch {
      // silently fail
    }
  },

  renameSession: async (id, title) => {
    try {
      await tauriInvoke("rename_session", { session_id: id, title });
      set((s) => ({
        sessions: s.sessions.map((sess) =>
          sess.id === id ? { ...sess, title } : sess
        ),
      }));
    } catch {
      // silently fail
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
        session_id: sessionId,
        up_to_message_id: upToMessageId ?? null,
      });
      // 把新会话加到 sessions 列表头部（最新的在前），并切换到新会话
      // 不在此处 clearMessages，由调用方负责（与 createSession 行为一致）
      set((s) => ({
        sessions: [newSession, ...s.sessions],
        activeSessionId: newSession.id,
      }));
    } catch {
      // silently fail
    }
  },
}));
