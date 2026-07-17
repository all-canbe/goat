import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { Search, Check, Settings, Cpu } from "lucide-react";
import { useConfigStore, type ProviderInfo } from "../../stores/configStore";

interface ModelPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  onOpenSettings: () => void;
}

export default function ModelPalette({
  isOpen,
  onClose,
  onOpenSettings,
}: ModelPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);

  const { providers, switchProvider } = useConfigStore();

  // 根据搜索词过滤 Provider
  const filtered = useMemo<ProviderInfo[]>(() => {
    if (!query.trim()) return providers;
    const lower = query.toLowerCase();
    return providers.filter(
      (p) =>
        p.name.toLowerCase().includes(lower) ||
        p.model.toLowerCase().includes(lower) ||
        p.provider_type.toLowerCase().includes(lower)
    );
  }, [providers, query]);

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
      if (e.target === overlayRef.current) {
        onClose();
      }
    },
    [onClose]
  );

  const handleOpenSettings = useCallback(() => {
    onOpenSettings();
    onClose();
  }, [onOpenSettings, onClose]);

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

  return (
    <div
      ref={overlayRef}
      className="fixed inset-0 z-50 flex items-start justify-center pt-[20vh] bg-black/50"
      onClick={handleOverlayClick}
    >
      <div className="bg-surface border border-border rounded-lg shadow-2xl w-full max-w-[480px] mx-4 overflow-hidden">
        {/* 搜索框 */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
          <Search size={16} className="text-text-secondary shrink-0" />
          <input
            ref={inputRef}
            type="text"
            className="flex-1 bg-transparent text-sm text-text outline-none placeholder:text-text-tertiary"
            placeholder="Search provider / model..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
          <kbd className="text-[10px] text-text-secondary bg-bg px-1.5 py-0.5 rounded border border-border hidden sm:inline-block">
            ESC
          </kbd>
        </div>

        {/* 列表 / 空状态 */}
        <div className="max-h-[320px] overflow-y-auto py-1">
          {providers.length === 0 ? (
            <div className="px-4 py-6 text-center">
              <div className="text-text-secondary text-xs mb-3">
                No providers configured
              </div>
              <button
                onClick={handleOpenSettings}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-primary text-white text-xs font-medium hover:bg-primary/90 transition-colors"
              >
                <Settings size={12} />
                Open Settings
              </button>
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
      </div>
    </div>
  );
}
