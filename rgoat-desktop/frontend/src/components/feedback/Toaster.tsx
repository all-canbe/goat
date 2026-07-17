// P0-4: 全局 Toast 通知组件

import { CheckCircle2, XCircle, AlertTriangle, Info, X } from "lucide-react";
import { useToastStore, type ToastKind } from "../../stores/toastStore";

const KIND_STYLES: Record<ToastKind, { icon: typeof Info; color: string; bg: string; border: string }> = {
  success: { icon: CheckCircle2, color: "text-success", bg: "bg-success-subtle", border: "border-success/40" },
  error: { icon: XCircle, color: "text-error", bg: "bg-error-subtle", border: "border-error/40" },
  warning: { icon: AlertTriangle, color: "text-warning", bg: "bg-warning-subtle", border: "border-warning/40" },
  info: { icon: Info, color: "text-brand", bg: "bg-primary-subtle", border: "border-primary/40" },
};

export default function Toaster() {
  const { toasts, removeToast } = useToastStore();

  if (toasts.length === 0) return null;

  return (
    <div className="fixed top-12 right-4 z-[60] flex flex-col gap-2 max-w-sm pointer-events-none">
      {toasts.map((toast) => {
        const style = KIND_STYLES[toast.kind];
        const Icon = style.icon;
        return (
          <div
            key={toast.id}
            className={`flex items-start gap-2 px-3 py-2 rounded-md border shadow-lg ${style.bg} ${style.border} pointer-events-auto animate-in fade-in slide-in-from-top-2`}
          >
            <Icon size={16} className={`${style.color} shrink-0 mt-0.5`} />
            <div className={`flex-1 text-sm ${style.color} break-words`}>
              {toast.message}
            </div>
            <button
              onClick={() => removeToast(toast.id)}
              className="shrink-0 text-text-secondary hover:text-text"
            >
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
