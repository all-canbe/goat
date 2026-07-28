import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";

export interface WorkspaceInfo {
  path: string;
  is_temporary: boolean;
}

/** 切换工作区的延迟（毫秒），用于 debounce 快速切换会话 */
const WORKSPACE_SWITCH_DELAY_MS = 5000;

/** 模块级定时器引用：不放入 store state，避免触发组件订阅 */
let switchTimerId: ReturnType<typeof setTimeout> | null = null;

/** 模块级 generation 计数器：每次新调度/取消时递增，使旧回调的 RPC 结果失效 */
let switchGeneration = 0;

interface WorkspaceState {
  workspace: WorkspaceInfo | null;
  /** 延迟切换期间的目标路径；非 null 时组件应显示等待占位 */
  pendingWorkspacePath: string | null;
  loadWorkspace: () => Promise<void>;
  setWorkspace: (path: string) => Promise<void>;
  setTemporaryWorkspace: () => Promise<void>;
  /**
   * 延迟切换工作区（5 秒 debounce）：
   * - path === null：立即清空 workspace（用于无关联工作区的会话）
   * - path === 当前 workspace.path：无操作
   * - 其他：5 秒后调用 set_workspace；期间连续调用会取消前次定时器
   * 返回 Promise<boolean>：true 表示切换成功，false 表示被取消或后端拒绝。
   */
  scheduleWorkspaceSwitch: (path: string | null) => Promise<boolean>;
  /** 取消尚未触发的延迟切换，使所有进行中的 RPC 结果失效 */
  cancelPendingSwitch: () => void;
  previewTarget: { path: string; name: string } | null;
  openPreview: (path: string, name: string) => void;
  closePreview: () => void;
}

/**
 * 模块级 helper：实际调用 set_workspace RPC 并更新 store。
 * 仅当 generation 仍为最新时写入 store，否则静默丢弃结果。
 */
async function invokeWorkspaceSwitch(
  path: string,
  generation: number,
): Promise<boolean> {
  try {
    const info = await tauriInvoke<WorkspaceInfo>("set_workspace", { path });
    if (generation !== switchGeneration) return false;
    useWorkspaceStore.setState({ workspace: info, previewTarget: null });
    return true;
  } catch (err) {
    console.warn("scheduleWorkspaceSwitch: set_workspace failed:", err);
    return false;
  }
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  workspace: null,
  pendingWorkspacePath: null,

  loadWorkspace: async () => {
    const info = await tauriInvoke<WorkspaceInfo>("get_workspace");
    set({ workspace: info });
  },

  setWorkspace: async (path: string) => {
    // 失败时抛出给 UI，不静默吞错；只有成功才更新 state
    const info = await tauriInvoke<WorkspaceInfo>("set_workspace", { path });
    // 切换 workspace 后关闭预览，避免 previewTarget 指向旧 workspace 文件
    set({ workspace: info, previewTarget: null });
  },

  setTemporaryWorkspace: async () => {
    const info = await tauriInvoke<WorkspaceInfo>("set_temporary_workspace");
    // 同理，临时 workspace 也清空预览目标
    set({ workspace: info, previewTarget: null });
  },

  scheduleWorkspaceSwitch: (path) => {
    // 递增 generation 使旧 RPC 回调失效
    switchGeneration += 1;
    const capturedGeneration = switchGeneration;

    // 清除已有定时器
    if (switchTimerId !== null) {
      clearTimeout(switchTimerId);
      switchTimerId = null;
    }

    // 无关联工作区：立即清空文件树
    if (path === null) {
      set({ workspace: null, pendingWorkspacePath: null });
      return Promise.resolve(true);
    }

    // 与当前 workspace 相同：不切换，清空 pending
    if (path === get().workspace?.path) {
      set({ pendingWorkspacePath: null });
      return Promise.resolve(true);
    }

    // 标记 pending，5 秒后真正切换
    set({ pendingWorkspacePath: path });
    return new Promise<boolean>((resolve) => {
      switchTimerId = setTimeout(async () => {
        switchTimerId = null;
        const success = await invokeWorkspaceSwitch(path, capturedGeneration);
        // 仅当 generation 仍为最新时才清理 pending（旧回调不清理）
        if (capturedGeneration === switchGeneration) {
          set({ pendingWorkspacePath: null });
        }
        resolve(success);
      }, WORKSPACE_SWITCH_DELAY_MS);
    });
  },

  cancelPendingSwitch: () => {
    switchGeneration += 1;
    if (switchTimerId !== null) {
      clearTimeout(switchTimerId);
      switchTimerId = null;
    }
    set({ pendingWorkspacePath: null });
  },

  previewTarget: null,

  openPreview: (path, name) => set({ previewTarget: { path, name } }),

  closePreview: () => set({ previewTarget: null }),
}));