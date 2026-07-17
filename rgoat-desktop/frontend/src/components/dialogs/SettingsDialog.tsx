// P2: 设置页 — Provider 切换 / Mode 默认值 / 关于 / 主题切换
import { useState, useEffect, useCallback } from "react";
import { X, Check, Cpu, Tag, Sun, Moon, Monitor, Info } from "lucide-react";
import { useConfigStore } from "../../stores/configStore";
import { useToastStore } from "../../stores/toastStore";
import { useThemeStore } from "../../stores/themeStore";

interface SettingsDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /** 默认 Mode 设置变更回调 */
  onDefaultModeChange?: (mode: string) => void;
  /** 当前默认 Mode */
  defaultMode?: string;
}

const MODES = ["Agent", "Plan", "Flow", "YOLO"] as const;
const APP_VERSION = "0.1.0";

export default function SettingsDialog({
  isOpen,
  onClose,
  onDefaultModeChange,
  defaultMode = "Agent",
}: SettingsDialogProps) {
  const { providers, switchProvider, loadProviders } = useConfigStore();
  const { addToast } = useToastStore();
  const { theme, setTheme } = useThemeStore();
  const [selectedMode, setSelectedMode] = useState(defaultMode);

  useEffect(() => {
    if (isOpen) {
      loadProviders();
    }
  }, [isOpen, loadProviders]);

  // P2: Esc 关闭
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

  const handleSwitchProvider = useCallback(
    async (name: string) => {
      try {
        await switchProvider(name);
        addToast(`已切换到 Provider: ${name}`, "success");
      } catch {
        addToast("切换 Provider 失败", "error");
      }
    },
    [switchProvider, addToast]
  );

  const handleModeChange = useCallback(
    (mode: string) => {
      setSelectedMode(mode);
      onDefaultModeChange?.(mode);
    },
    [onDefaultModeChange]
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
          <span className="text-sm font-semibold text-text flex-1">设置</span>
          <button
            onClick={onClose}
            title="关闭 (Esc)"
            className="p-1 rounded text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          >
            <X size={16} />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-5 py-4 space-y-6">
          {/* Provider 区域 */}
          <section>
            <h3 className="flex items-center gap-1.5 text-xs font-semibold text-text-secondary uppercase tracking-wide mb-2">
              <Cpu size={12} />
              Provider
            </h3>
            <div className="space-y-1">
              {providers.length === 0 ? (
                <div className="text-xs text-text-secondary italic px-3 py-2">
                  暂无已配置的 Provider
                </div>
              ) : (
                providers.map((p) => (
                  <button
                    key={p.name}
                    onClick={() => handleSwitchProvider(p.name)}
                    className={`w-full flex items-center gap-3 px-3 py-2 rounded-md border transition-colors text-left ${
                      p.is_current
                        ? "border-primary bg-primary-subtle"
                        : "border-border hover:border-primary/50 hover:bg-surface-hover"
                    }`}
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className={`text-sm font-medium ${p.is_current ? "text-brand" : "text-text"}`}>
                          {p.name}
                        </span>
                        {p.is_current && (
                          <Check size={12} className="text-brand shrink-0" />
                        )}
                      </div>
                      <div className="flex items-center gap-1 mt-0.5">
                        <Tag size={10} className="text-text-secondary" />
                        <span className="text-[11px] text-text-secondary truncate">{p.model}</span>
                      </div>
                    </div>
                    <span className="text-[10px] text-text-secondary shrink-0 uppercase border border-border rounded px-1">
                      {p.provider_type}
                    </span>
                  </button>
                ))
              )}
            </div>
          </section>

          {/* 默认 Mode 设置 */}
          <section>
            <h3 className="flex items-center gap-1.5 text-xs font-semibold text-text-secondary uppercase tracking-wide mb-2">
              <Cpu size={12} />
              默认 Mode
            </h3>
            <div className="flex items-center gap-1 bg-bg border border-border rounded-md p-1 w-fit">
              {MODES.map((m) => (
                <button
                  key={m}
                  onClick={() => handleModeChange(m)}
                  className={`px-3 py-1.5 text-xs font-medium rounded transition-colors ${
                    selectedMode === m
                      ? "bg-primary-subtle text-brand"
                      : "text-text-secondary hover:text-text"
                  }`}
                >
                  {m}
                </button>
              ))}
            </div>
          </section>

          {/* 主题切换 */}
          <section>
            <h3 className="flex items-center gap-1.5 text-xs font-semibold text-text-secondary uppercase tracking-wide mb-2">
              <Sun size={12} />
              主题
            </h3>
            <div className="flex items-center gap-1 bg-bg border border-border rounded-md p-1 w-fit">
              {([
                { key: "light", label: "亮色", icon: Sun },
                { key: "dark", label: "暗色", icon: Moon },
                { key: "system", label: "跟随系统", icon: Monitor },
              ] as const).map(({ key, label, icon: Icon }) => (
                <button
                  key={key}
                  onClick={() => setTheme(key)}
                  className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded transition-colors ${
                    theme === key
                      ? "bg-primary-subtle text-brand"
                      : "text-text-secondary hover:text-text"
                  }`}
                >
                  <Icon size={12} />
                  {label}
                </button>
              ))}
            </div>
          </section>

          {/* 关于 */}
          <section>
            <h3 className="flex items-center gap-1.5 text-xs font-semibold text-text-secondary uppercase tracking-wide mb-2">
              <Info size={12} />
              关于
            </h3>
            <div className="bg-bg border border-border rounded-md px-3 py-2 space-y-1">
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-secondary">版本</span>
                <span className="text-text font-mono">v{APP_VERSION}</span>
              </div>
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-secondary">项目路径</span>
                <span className="text-text font-mono truncate ml-2" title="当前工作区">
                  当前工作区
                </span>
              </div>
            </div>
          </section>
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
