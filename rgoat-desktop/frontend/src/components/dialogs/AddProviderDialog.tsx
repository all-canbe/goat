import { useEffect } from "react";
import { X } from "lucide-react";
import ProviderForm from "../provider/ProviderForm";

interface AddProviderDialogProps {
  isOpen: boolean;
  onClose: () => void;
}

export default function AddProviderDialog({
  isOpen,
  onClose,
}: AddProviderDialogProps) {
  // Esc 关闭
  useEffect(() => {
    if (!isOpen) return;
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="bg-surface border border-border rounded-lg shadow-2xl w-full max-w-md mx-4 overflow-hidden flex flex-col max-h-[90vh]">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-surface">
          <span className="text-sm font-semibold text-text flex-1">
            连接提供商
          </span>
          <button
            onClick={onClose}
            title="关闭 (Esc)"
            className="p-1 rounded text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          >
            <X size={16} />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-6">
          <ProviderForm onSuccess={onClose} onCancel={onClose} />
        </div>
      </div>
    </div>
  );
}
