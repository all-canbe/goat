import { Folder, FolderClock, X } from "lucide-react";
import type { WorkspaceInfo } from "../../stores/workspaceStore";

interface WorkspaceSessionDialogProps {
  isOpen: boolean;
  currentWorkspace: WorkspaceInfo | null;
  onUseTemporary: () => Promise<void>;
  onChooseDirectory: () => Promise<void>;
  onClose: () => void;
}

function formatWorkspaceSubtitle(rawPath?: string): string {
  if (!rawPath) return "选择一个新的本地项目目录";
  const cleaned = rawPath.replace(/^\\\\\?\\/, "");
  if (cleaned.includes("temporary-workspace")) {
    return "当前: 临时工作空间 (未指定本地项目)";
  }
  const parts = cleaned.split(/[/\\]/).filter(Boolean);
  const basename = parts[parts.length - 1] || cleaned;
  return `最近使用: ${basename} (${cleaned})`;
}

export default function WorkspaceSessionDialog({
  isOpen,
  currentWorkspace,
  onUseTemporary,
  onChooseDirectory,
  onClose,
}: WorkspaceSessionDialogProps) {
  if (!isOpen) return null;

  const subtitle = formatWorkspaceSubtitle(currentWorkspace?.path);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      role="dialog"
      aria-modal="true"
      aria-label="选择工作空间"
      onMouseDown={onClose}
    >
      <div
        className="w-full max-w-md rounded-xl border border-border bg-surface p-5 shadow-xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <div>
            <h2 className="text-base font-medium text-text">选择工作空间</h2>
            <p className="mt-1 text-xs text-text-secondary">新会话将在所选目录中执行。</p>
          </div>
          <button
            type="button"
            className="rounded p-1 text-text-secondary hover:bg-surface-hover hover:text-text"
            onClick={onClose}
            aria-label="关闭"
          >
            <X size={16} />
          </button>
        </div>

        <div className="space-y-2">
          <button
            type="button"
            className="flex w-full items-center gap-3 rounded-lg border border-border p-3 text-left hover:bg-surface-hover transition-colors"
            aria-label="使用临时工作空间"
            onClick={() => void onUseTemporary()}
          >
            <FolderClock size={18} className="text-warning shrink-0" />
            <span className="min-w-0">
              <span className="block text-sm text-text font-medium">使用临时工作空间</span>
              <span className="block text-xs text-text-secondary">应用数据目录中的固定临时目录</span>
            </span>
          </button>
          <button
            type="button"
            className="flex w-full items-center gap-3 rounded-lg border border-border p-3 text-left hover:bg-surface-hover transition-colors"
            onClick={() => void onChooseDirectory()}
          >
            <Folder size={18} className="text-primary shrink-0" />
            <span className="min-w-0 flex-1">
              <span className="block text-sm text-text font-medium">选择本地目录</span>
              <span className="block text-xs text-text-secondary truncate" title={subtitle}>
                {subtitle}
              </span>
            </span>
          </button>
        </div>
      </div>
    </div>
  );
}
