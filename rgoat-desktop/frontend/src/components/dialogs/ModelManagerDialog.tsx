import { useState, useEffect, useMemo, useCallback } from "react";
import { X, Search, Trash2, Plus } from "lucide-react";
import { useConfigStore, type ProviderInfo } from "../../stores/configStore";
import { useToastStore } from "../../stores/toastStore";

interface ModelManagerDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onAddProvider: () => void;
}

function ConfirmDelete({
  name,
  onConfirm,
  onCancel,
}: {
  name: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="absolute inset-0 z-10 bg-surface/95 flex flex-col items-center justify-center p-4 gap-3">
      <div className="text-sm text-text text-center">
        确认删除 Provider <span className="font-semibold">{name}</span>？
        <div className="text-xs text-text-secondary mt-1">此操作不可撤销。</div>
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="px-3 py-1.5 rounded-md border border-border text-text-secondary text-xs hover:bg-surface-hover transition-colors"
        >
          取消
        </button>
        <button
          type="button"
          onClick={onConfirm}
          className="px-3 py-1.5 rounded-md bg-error text-white text-xs font-medium hover:bg-error/90 transition-colors"
        >
          删除
        </button>
      </div>
    </div>
  );
}

export default function ModelManagerDialog({
  isOpen,
  onClose,
  onAddProvider,
}: ModelManagerDialogProps) {
  const { providers, setProviderEnabled, deleteProvider } = useConfigStore();
  const { addToast } = useToastStore();
  const [query, setQuery] = useState("");
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);

  // 仅展示 settings 来源 Provider（含禁用项）；env/fallback 不在管理范围
  const settingsProviders = useMemo<ProviderInfo[]>(
    () => providers.filter((p) => p.source === "settings"),
    [providers]
  );

  const filtered = useMemo<ProviderInfo[]>(() => {
    if (!query.trim()) return settingsProviders;
    const lower = query.toLowerCase();
    return settingsProviders.filter((p) =>
      p.name.toLowerCase().includes(lower)
    );
  }, [settingsProviders, query]);

  useEffect(() => {
    if (!isOpen) {
      setQuery("");
      setPendingDelete(null);
    }
  }, [isOpen]);

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

  const handleToggleEnabled = useCallback(
    async (p: ProviderInfo, next: boolean) => {
      if (p.is_current) {
        addToast("当前 Provider 不能被禁用，请先切换到其他 Provider", "error");
        return;
      }
      try {
        await setProviderEnabled(p.name, next);
        addToast(
          next ? `已启用 ${p.name}` : `已禁用 ${p.name}`,
          "success"
        );
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        addToast(msg || "操作失败", "error");
      }
    },
    [setProviderEnabled, addToast]
  );

  const handleConfirmDelete = useCallback(
    async (name: string) => {
      try {
        await deleteProvider(name);
        addToast(`已删除 ${name}`, "success");
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        addToast(msg || "删除失败", "error");
      } finally {
        setPendingDelete(null);
      }
    },
    [deleteProvider, addToast]
  );

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="bg-surface border border-border rounded-lg shadow-2xl w-full max-w-2xl mx-4 overflow-hidden flex flex-col max-h-[90vh]">
        {/* Header */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-surface">
          <div className="flex-1 min-w-0">
            <div className="text-sm font-semibold text-text">管理模型</div>
            <div className="text-[11px] text-text-secondary truncate">
              自定义模型选择器中显示的模型
            </div>
          </div>
          <button
            type="button"
            onClick={onAddProvider}
            className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-primary text-white text-xs font-medium hover:bg-primary/90 transition-colors"
          >
            <Plus size={12} />
            连接提供商
          </button>
          <button
            onClick={onClose}
            title="关闭 (Esc)"
            className="p-1 rounded text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          >
            <X size={16} />
          </button>
        </div>

        {/* 搜索 */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
          <Search size={16} className="text-text-secondary shrink-0" />
          <input
            type="text"
            className="flex-1 bg-transparent text-sm text-text outline-none placeholder:text-text-tertiary"
            placeholder="搜索 Provider..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>

        {/* 列表 */}
        <div className="flex-1 overflow-y-auto relative">
          {filtered.length === 0 ? (
            <div className="px-4 py-8 text-center text-text-secondary text-xs">
              {settingsProviders.length === 0
                ? "没有已配置的 Provider，点击“连接提供商”添加。"
                : "没有匹配的 Provider。"}
            </div>
          ) : (
            filtered.map((p) => {
              const disabled = p.is_current;
              return (
                <div
                  key={p.name}
                  className="flex items-center gap-3 px-4 py-2.5 border-b border-border/50 last:border-b-0"
                >
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span
                        className={`text-sm truncate ${
                          p.enabled ? "text-text" : "text-text-secondary"
                        }`}
                      >
                        {p.name}
                      </span>
                      {p.is_current && (
                        <span className="text-[10px] text-brand shrink-0">
                          当前
                        </span>
                      )}
                    </div>
                    <div className="text-[11px] text-text-secondary truncate">
                      {p.model}
                    </div>
                  </div>

                  {/* 启用/禁用开关 */}
                  <button
                    type="button"
                    role="switch"
                    aria-checked={p.enabled}
                    aria-label={`切换 ${p.name} 启用状态`}
                    disabled={disabled}
                    title={
                      disabled
                        ? "当前 Provider 不能被禁用"
                        : p.enabled
                        ? "点击禁用"
                        : "点击启用"
                    }
                    onClick={() => handleToggleEnabled(p, !p.enabled)}
                    className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors ${
                      p.enabled ? "bg-primary" : "bg-surface-active"
                    } ${disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"}`}
                  >
                    <span
                      className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform ${
                        p.enabled ? "translate-x-4" : "translate-x-1"
                      }`}
                    />
                  </button>

                  {/* 删除 */}
                  <button
                    type="button"
                    onClick={() => setPendingDelete(p.name)}
                    disabled={disabled}
                    title={
                      disabled
                        ? "当前 Provider 不能删除"
                        : "删除 Provider"
                    }
                    className="p-1 rounded text-text-secondary hover:text-error hover:bg-surface-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:text-text-secondary"
                    aria-label={`删除 ${p.name}`}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              );
            })
          )}

          {pendingDelete && (
            <ConfirmDelete
              name={pendingDelete}
              onConfirm={() => handleConfirmDelete(pendingDelete)}
              onCancel={() => setPendingDelete(null)}
            />
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border bg-bg/50">
          <button
            onClick={onClose}
            className="px-4 py-1.5 rounded-md text-xs font-medium text-white bg-primary hover:bg-primary/90 transition-colors"
          >
            完成
          </button>
        </div>
      </div>
    </div>
  );
}
