import { PanelLeft, Search, PanelRight, Settings } from "lucide-react";
import { useSessionStore } from "../../stores/sessionStore";

interface TitleBarProps {
  onToggleSidebar: () => void;
  onToggleCommandPalette: () => void;
  onToggleRightPanel: () => void;
  onOpenSettings: () => void;
}

export default function TitleBar({
  onToggleSidebar,
  onToggleCommandPalette,
  onToggleRightPanel,
  onOpenSettings,
}: TitleBarProps) {
  const { sessions, activeSessionId } = useSessionStore();
  const activeSession = sessions.find((s) => s.id === activeSessionId);
  const tabTitle = activeSession?.title || "New Session";

  return (
    <header className="flex items-center h-10 px-3 bg-bg-elevated border-b border-border shrink-0">
      {/* Left: sidebar toggle + brand */}
      <div className="flex items-center gap-2">
        <button
          onClick={onToggleSidebar}
          className="p-1.5 rounded-md text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          aria-label="Toggle sidebar"
          title="Toggle sidebar"
        >
          <PanelLeft size={16} />
        </button>
        <div className="flex items-center gap-2">
          <div className="w-5 h-5 rounded bg-primary flex items-center justify-center">
            <span className="text-white text-[10px] font-bold">R</span>
          </div>
          <span className="font-semibold text-sm text-brand">RGoat</span>
        </div>
      </div>

      {/* Center: session tabs */}
      <div className="flex-1 flex items-center justify-center px-4">
        <div className="flex items-center">
          <button
            className="relative px-3 py-2 text-sm font-medium text-text transition-colors"
            aria-label={`Current session: ${tabTitle}`}
            title={tabTitle}
          >
            <span className="max-w-[200px] truncate block">{tabTitle}</span>
            <span className="absolute bottom-0 left-0 right-0 h-0.5 bg-primary rounded-t" />
          </button>
        </div>
      </div>

      {/* Right: search + right panel + settings */}
      <div className="flex items-center gap-1">
        <button
          onClick={onToggleCommandPalette}
          className="p-1.5 rounded-md text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          aria-label="Open command palette"
          title="Command palette"
        >
          <Search size={16} />
        </button>
        <button
          onClick={onToggleRightPanel}
          className="p-1.5 rounded-md text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          aria-label="Toggle right panel"
          title="Toggle right panel"
        >
          <PanelRight size={16} />
        </button>
        <button
          onClick={onOpenSettings}
          className="p-1.5 rounded-md text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          aria-label="Open settings"
          title="Settings"
        >
          <Settings size={16} />
        </button>
      </div>
    </header>
  );
}
