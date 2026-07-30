import { PanelLeft, Search, PanelRight, Settings } from "lucide-react";
import { useSessionStore } from "../../stores/sessionStore";
import GoatLogo from "../common/GoatLogo";
import WindowControls from "./WindowControls";

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

  const dragStyle = { WebkitAppRegion: "drag" } as React.CSSProperties;
  const noDragStyle = { WebkitAppRegion: "no-drag" } as React.CSSProperties;

  return (
    <header
      data-tauri-drag-region
      style={dragStyle}
      className="flex items-center h-9 pl-3 pr-0 bg-bg-elevated border-b border-border shrink-0 select-none cursor-default"
    >
      {/* Left: sidebar toggle + brand */}
      <div className="flex items-center gap-2" style={noDragStyle}>
        <button
          onClick={onToggleSidebar}
          style={noDragStyle}
          className="p-1.5 rounded-md text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          aria-label="Toggle sidebar"
          title="Toggle sidebar"
        >
          <PanelLeft size={15} />
        </button>
        <div className="flex items-center gap-2 pointer-events-none">
          <GoatLogo size={18} />
          <span className="font-semibold text-xs text-text tracking-wide">Goat</span>
        </div>
      </div>

      {/* Center: session tabs (drag region) */}
      <div className="flex-1 flex items-center justify-center px-4 h-full" style={dragStyle} data-tauri-drag-region>
        <div className="flex items-center" style={noDragStyle}>
          <button
            style={noDragStyle}
            className="relative px-3 py-1.5 text-xs font-medium text-text transition-colors"
            aria-label={`Current session: ${tabTitle}`}
            title={tabTitle}
          >
            <span className="max-w-[220px] truncate block">{tabTitle}</span>
            <span className="absolute bottom-0 left-0 right-0 h-0.5 bg-primary rounded-t" />
          </button>
        </div>
      </div>

      {/* Right: search + right panel + settings + window controls */}
      <div className="flex items-center h-full gap-0.5" style={noDragStyle}>
        <button
          onClick={onToggleCommandPalette}
          style={noDragStyle}
          className="p-1.5 rounded-md text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          aria-label="Open command palette"
          title="Command palette (Ctrl+K)"
        >
          <Search size={15} />
        </button>
        <button
          onClick={onToggleRightPanel}
          style={noDragStyle}
          className="p-1.5 rounded-md text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          aria-label="Toggle right panel"
          title="Toggle right panel (Ctrl+J)"
        >
          <PanelRight size={15} />
        </button>
        <button
          onClick={onOpenSettings}
          style={noDragStyle}
          className="p-1.5 rounded-md text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          aria-label="Open settings"
          title="Settings"
        >
          <Settings size={15} />
        </button>

        {/* Custom Window Minimize / Maximize / Close Controls */}
        <WindowControls />
      </div>
    </header>
  );
}
