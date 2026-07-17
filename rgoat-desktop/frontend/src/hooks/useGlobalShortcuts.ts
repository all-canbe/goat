// P2: 全局快捷键集中管理 hook
import { useEffect } from "react";

interface UseGlobalShortcutsOptions {
  /** 切换 CommandPalette 开关 */
  onToggleCommandPalette: () => void;
  /** 新建会话 */
  onNewSession: () => void;
  /** 切换 Sidebar 折叠状态 */
  onToggleSidebar: () => void;
  /** 聚焦输入框 */
  onFocusInput: () => void;
  /** 关闭当前打开的对话框 */
  onCloseDialog: () => void;
}

/**
 * 集中管理全局快捷键：
 * - Cmd/Ctrl+K → 切换 CommandPalette
 * - Cmd/Ctrl+N → 新建会话
 * - Cmd/Ctrl+B → 切换 Sidebar 折叠
 * - Cmd/Ctrl+/ → 聚焦输入框
 * - Escape → 关闭当前打开的对话框
 *
 * 注意：输入框聚焦时不应触发全局快捷键，但 Cmd 组合键除外。
 */
export function useGlobalShortcuts({
  onToggleCommandPalette,
  onNewSession,
  onToggleSidebar,
  onFocusInput,
  onCloseDialog,
}: UseGlobalShortcutsOptions) {
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      const isMod = e.ctrlKey || e.metaKey;
      // 判断当前焦点是否在输入框内
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName ?? "";
      const isInInput = tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable === true;

      // Cmd/Ctrl 组合键：即使在输入框内也触发
      if (isMod) {
        const key = e.key.toLowerCase();
        if (key === "k") {
          e.preventDefault();
          onToggleCommandPalette();
          return;
        }
        if (key === "n") {
          e.preventDefault();
          onNewSession();
          return;
        }
        if (key === "b") {
          e.preventDefault();
          onToggleSidebar();
          return;
        }
        if (key === "/") {
          e.preventDefault();
          onFocusInput();
          return;
        }
        return;
      }

      // 非 Cmd 组合键：输入框聚焦时不触发
      if (isInInput) return;

      if (e.key === "Escape") {
        e.preventDefault();
        onCloseDialog();
        return;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    onToggleCommandPalette,
    onNewSession,
    onToggleSidebar,
    onFocusInput,
    onCloseDialog,
  ]);
}
