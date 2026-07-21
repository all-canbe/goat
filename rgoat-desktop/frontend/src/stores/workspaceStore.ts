import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";

export interface WorkspaceInfo {
  path: string;
  is_temporary: boolean;
}

interface WorkspaceState {
  workspace: WorkspaceInfo | null;
  loadWorkspace: () => Promise<void>;
  setWorkspace: (path: string) => Promise<void>;
}

export const useWorkspaceStore = create<WorkspaceState>((set) => ({
  workspace: null,

  loadWorkspace: async () => {
    const info = await tauriInvoke<WorkspaceInfo>("get_workspace");
    set({ workspace: info });
  },

  setWorkspace: async (path: string) => {
    // 失败时抛出给 UI，不静默吞错；只有成功才更新 state
    const info = await tauriInvoke<WorkspaceInfo>("set_workspace", { path });
    set({ workspace: info });
  },
}));
