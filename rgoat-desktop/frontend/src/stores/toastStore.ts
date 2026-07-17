// P0-4: 全局 Toast 通知 store

import { create } from "zustand";

export type ToastKind = "success" | "error" | "warning" | "info";

export interface Toast {
  id: string;
  message: string;
  kind: ToastKind;
  /** 自动消失时长（ms），0 表示不自动消失 */
  duration: number;
}

interface ToastState {
  toasts: Toast[];
  addToast: (message: string, kind?: ToastKind, duration?: number) => string;
  removeToast: (id: string) => void;
}

let counter = 0;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],

  addToast: (message, kind = "info", duration = 4000) => {
    const id = `toast-${Date.now()}-${counter++}`;
    set((s) => ({
      toasts: [...s.toasts, { id, message, kind, duration }],
    }));
    // 自动消失
    if (duration > 0) {
      setTimeout(() => {
        set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
      }, duration);
    }
    return id;
  },

  removeToast: (id) =>
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
