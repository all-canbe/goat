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
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div
        role="dialog"
        aria-modal="true"
        aria-label="连接提供商"
        className="bg-surface border border-border rounded-lg shadow-2xl w-full max-w-md mx-4 overflow-hidden flex flex-col max-h-[90vh]"
      >
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-surface">
          <span className="text-sm font-semibold text-text flex-1">
            连接提供商
          </span>
          <button
            onClick={onClose}
            aria-label="关闭"
            title="关闭"
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
