// D1-T06: 会话内文件变更 store — 镜像后端 session_changes

import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";

export interface FileChangeRecord {
  file_path: string;
  change_type: string; // "create" | "edit" | "delete"
  diff: string;
  tool_name: string;
  timestamp: string;
  additions: number;
  deletions: number;
}

interface ChangesState {
  changesBySession: Record<string, FileChangeRecord[]>;
  selectedChangeIndex: number | null;
  addChange: (sessionId: string, change: FileChangeRecord) => void;
  clearChanges: (sessionId: string) => void;
  loadChanges: (sessionId: string) => Promise<void>;
  setSelectedIndex: (idx: number | null) => void;
}

export const useChangesStore = create<ChangesState>((set) => ({
  changesBySession: {},
  selectedChangeIndex: null,
  addChange: (sessionId, change) =>
    set((s) => ({
      changesBySession: {
        ...s.changesBySession,
        [sessionId]: [...(s.changesBySession[sessionId] || []), change],
      },
    })),
  clearChanges: (sessionId) => {
    tauriInvoke("clear_session_changes", { sessionId }).catch(() => {});
    set((s) => {
      const next = { ...s.changesBySession };
      delete next[sessionId];
      return { changesBySession: next, selectedChangeIndex: null };
    });
  },
  loadChanges: async (sessionId) => {
    try {
      const changes = await tauriInvoke<FileChangeRecord[]>(
        "get_session_changes",
        { sessionId }
      );
      set((s) => ({
        changesBySession: { ...s.changesBySession, [sessionId]: changes },
      }));
    } catch {
      // 后端命令不可用时忽略
    }
  },
  setSelectedIndex: (idx) => set({ selectedChangeIndex: idx }),
}));
