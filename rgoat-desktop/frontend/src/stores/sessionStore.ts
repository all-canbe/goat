import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";

export interface Session {
  id: string;
  title: string;
  message_count: number;
  created_at: string;
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
}));
