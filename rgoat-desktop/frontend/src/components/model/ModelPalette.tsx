import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import {
  Search,
  Check,
  Settings as SettingsIcon,
  Cpu,
  Plus,
} from "lucide-react";
import { useConfigStore, type ProviderInfo } from "../../stores/configStore";

export type ModelPaletteVariant = "overlay" | "dropdown";

interface ModelPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  /** overlay 模式下打开设置；dropdown 模式下打开管理弹窗。 */
  onOpenSettings?: () => void;
  /** dropdown 模式下打开新增 Provider 弹窗。 */
  onAddProvider?: () => void;
  /** dropdown 模式下打开模型管理弹窗。 */
  onManageProviders?: () => void;
  variant?: ModelPaletteVariant;
}

export default function ModelPalette({
  isOpen,
  onClose,
  onOpenSettings,
  onAddProvider,
  onManageProviders,
  variant = "overlay",
}: ModelPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);

  const { providers, switchProvider } = useConfigStore();

  // 展示所有启用的 Provider（含 settings/env/fallback）；禁用项不显示
  const visibleProviders = useMemo<ProviderInfo[]>(
    () => providers.filter((p) => p.enabled),
    [providers]
  );

  // 根据搜索词过滤 Provider
  const filtered = useMemo<ProviderInfo[]>(() => {
    if (!query.trim()) return visibleProviders;
    const lower = query.toLowerCase();
    return visibleProviders.filter(
      (p) =>
        p.name.toLowerCase().includes(lower) ||
        p.model.toLowerCase().includes(lower) ||
        p.provider_type.toLowerCase().includes(lower)
    );
  }, [visibleProviders, query]);

  // 打开时重置状态并聚焦
  useEffect(() => {
    if (isOpen) {
      setQuery("");
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [isOpen]);

  // 过滤列表变化时重置选中索引
  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  // 打开期间始终响应 Esc，避免焦点离开搜索框后无法关闭。
  useEffect(() => {
    if (!isOpen) return;
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [isOpen, onClose]);

  const handleSwitch = useCallback(
    (provider: ProviderInfo) => {
      switchProvider(provider.name);
      onClose();
    },
    [switchProvider, onClose]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setSelectedIndex((prev) => Math.min(prev + 1, filtered.length - 1));
          break;
        case "ArrowUp":
          e.preventDefault();
          setSelectedIndex((prev) => Math.max(prev - 1, 0));
          break;
        case "Enter":
          e.preventDefault();
          if (filtered[selectedIndex]) {
            handleSwitch(filtered[selectedIndex]);
          }
          break;
        case "Escape":
          e.preventDefault();
          onClose();
          break;
      }
    },
    [filtered, selectedIndex, handleSwitch, onClose]
  );

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (variant === "overlay" && e.target === overlayRef.current) {
        onClose();
      }
    },
    [variant, onClose]
  );

  const handleOpenSettings = useCallback(() => {
    onOpenSettings?.();
    onClose();
  }, [onOpenSettings, onClose]);

  const handleManage = useCallback(() => {
    onManageProviders?.();
    onClose();
  }, [onManageProviders, onClose]);

  const handleAdd = useCallback(() => {
    onAddProvider?.();
    onClose();
  }, [onAddProvider, onClose]);

  if (!isOpen) return null;

  function highlightMatch(text: string, searchQuery: string): React.ReactNode {
    if (!searchQuery.trim()) return text;
    const lower = text.toLowerCase();
    const idx = lower.indexOf(searchQuery.toLowerCase());
    if (idx === -1) return text;
    return (
      <>
        {text.slice(0, idx)}
        <span className="text-brand font-bold">
          {text.slice(idx, idx + searchQuery.length)}
        </span>
        {text.slice(idx + searchQuery.length)}
      </>
    );
  }

  const isDropdown = variant === "dropdown";

  const containerClass = isDropdown
    ? "absolute bottom-full left-0 mb-2 z-50 bg-surface border border-border rounded-lg shadow-xl w-full min-w-[320px] max-w-[420px] overflow-hidden"
    : "bg-surface border border-border rounded-lg shadow-2xl w-full max-w-[480px] mx-4 overflow-hidden";

  const wrapper = isDropdown ? (
    <>
      <div
        data-testid="model-palette-backdrop"
        className="fixed inset-0 z-40"
        onClick={onClose}
      />
      <div className={containerClass}>
        <PaletteBody />
      </div>
    </>
  ) : (
    <div
      ref={overlayRef}
      className="fixed inset-0 z-50 flex items-start justify-center pt-[20vh] bg-black/50"
      onClick={handleOverlayClick}
    >
      <div className={containerClass}>
        <PaletteBody />
      </div>
    </div>
  );

  function PaletteBody() {
    return (
      <>
        {/* 搜索框 + 右上角操作 */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
          <Search size={16} className="text-text-secondary shrink-0" />
          <input
            ref={inputRef}
            type="text"
            className="flex-1 bg-transparent text-sm text-text outline-none focus-visible:outline-none placeholder:text-text-tertiary"
            placeholder="Search provider / model..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
          {isDropdown && (
            <div className="flex items-center gap-1 shrink-0">
              <button
                type="button"
                onClick={handleAdd}
                title="连接提供商"
                aria-label="连接提供商"
                className="p-1 rounded text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
              >
                <Plus size={14} />
              </button>
              <button
                type="button"
                onClick={handleManage}
                title="管理模型"
                aria-label="管理模型"
                className="p-1 rounded text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
              >
                <SettingsIcon size={14} />
              </button>
            </div>
          )}
          {!isDropdown && (
            <kbd className="text-[10px] text-text-secondary bg-bg px-1.5 py-0.5 rounded border border-border hidden sm:inline-block">
              ESC
            </kbd>
          )}
        </div>

        {/* 列表 / 空状态 */}
        <div className="max-h-[320px] overflow-y-auto py-1">
          {visibleProviders.length === 0 ? (
            <div className="px-4 py-6 text-center">
              <div className="text-text-secondary text-xs mb-3">
                No providers configured
              </div>
              {isDropdown ? (
                <button
                  onClick={handleAdd}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-primary text-white text-xs font-medium hover:bg-primary/90 transition-colors"
                >
                  <Plus size={12} />
                  Add provider
                </button>
              ) : (
                <button
                  onClick={handleOpenSettings}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-primary text-white text-xs font-medium hover:bg-primary/90 transition-colors"
                >
                  <SettingsIcon size={12} />
                  Open Settings
                </button>
              )}
            </div>
          ) : filtered.length === 0 ? (
            <div className="px-4 py-6 text-center text-text-secondary text-xs">
              No matching providers
            </div>
          ) : (
            filtered.map((p, idx) => (
              <button
                key={p.name}
                className={`w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors ${
                  idx === selectedIndex
                    ? "bg-primary-subtle text-brand"
                    : "text-text hover:bg-surface-hover"
                }`}
                onClick={() => handleSwitch(p)}
                onMouseEnter={() => setSelectedIndex(idx)}
              >
                <Cpu size={14} className="shrink-0" />
                <div className="flex-1 min-w-0">
                  <div className="text-sm truncate">
                    {highlightMatch(p.name, query)}
                  </div>
                  <div className="text-[11px] text-text-secondary truncate">
                    {highlightMatch(p.model, query)}
                  </div>
                </div>
                {p.is_current && (
                  <span className="inline-flex items-center gap-1 text-[10px] text-brand shrink-0">
                    <Check size={11} />
                    Current
                  </span>
                )}
              </button>
            ))
          )}
        </div>

        {/* 底部快捷键提示 */}
        <div className="flex items-center gap-3 px-4 py-2 border-t border-border text-[10px] text-text-secondary">
          <span>
            <kbd className="bg-bg px-1 py-0.5 rounded border border-border text-[9px]">
              ↑↓
            </kbd>{" "}
            Navigate
          </span>
          <span>
            <kbd className="bg-bg px-1 py-0.5 rounded border border-border text-[9px]">
              Enter
            </kbd>{" "}
            Switch
          </span>
          <span>
            <kbd className="bg-bg px-1 py-0.5 rounded border border-border text-[9px]">
              Esc
            </kbd>{" "}
            Close
          </span>
        </div>
      </>
    );
  }

  return wrapper;
}
