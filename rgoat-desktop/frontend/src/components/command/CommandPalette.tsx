import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { Search, Command } from "lucide-react";

interface CommandItem {
  id: string;
  label: string;
  category: string;
  action: () => void;
}

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  onModeChange?: (mode: string) => void;
  onNewSession?: () => void;
  onClearMessages?: () => void;
}

export default function CommandPalette({
  isOpen,
  onClose,
  onModeChange,
  onNewSession,
  onClearMessages,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);

  const commands = useMemo<CommandItem[]>(
    () => [
      {
        id: "new-session",
        label: "New Session",
        category: "Session",
        action: () => {
          onNewSession?.();
          onClose();
        },
      },
      {
        id: "switch-agent",
        label: "Switch Mode: Agent",
        category: "Mode",
        action: () => {
          onModeChange?.("Agent");
          onClose();
        },
      },
      {
        id: "switch-plan",
        label: "Switch Mode: Plan",
        category: "Mode",
        action: () => {
          onModeChange?.("Plan");
          onClose();
        },
      },
      {
        id: "switch-flow",
        label: "Switch Mode: Flow",
        category: "Mode",
        action: () => {
          onModeChange?.("Flow");
          onClose();
        },
      },
      {
        id: "switch-yolo",
        label: "Switch Mode: YOLO",
        category: "Mode",
        action: () => {
          onModeChange?.("YOLO");
          onClose();
        },
      },
      {
        id: "clear-messages",
        label: "Clear Messages",
        category: "Session",
        action: () => {
          onClearMessages?.();
          onClose();
        },
      },
      {
        id: "toggle-file-tree",
        label: "Toggle File Tree",
        category: "View",
        action: () => {
          onClose();
        },
      },
    ],
    [onClose, onModeChange, onNewSession, onClearMessages]
  );

  const filtered = useMemo(() => {
    if (!query.trim()) return commands;
    const lower = query.toLowerCase();
    return commands.filter((cmd) => cmd.label.toLowerCase().includes(lower));
  }, [commands, query]);

  // Reset state on open
  useEffect(() => {
    if (isOpen) {
      setQuery("");
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [isOpen]);

  // Reset selected index when filtered list changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

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
            filtered[selectedIndex].action();
          }
          break;
        case "Escape":
          e.preventDefault();
          onClose();
          break;
      }
    },
    [filtered, selectedIndex, onClose]
  );

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === overlayRef.current) {
        onClose();
      }
    },
    [onClose]
  );

  if (!isOpen) return null;

  function highlightMatch(text: string, searchQuery: string): React.ReactNode {
    if (!searchQuery.trim()) return text;
    const lower = text.toLowerCase();
    const idx = lower.indexOf(searchQuery.toLowerCase());
    if (idx === -1) return text;
    return (
      <>
        {text.slice(0, idx)}
        <span className="text-primary font-bold">{text.slice(idx, idx + searchQuery.length)}</span>
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
      <div className="bg-surface border border-border rounded-lg shadow-2xl w-full max-w-lg mx-4 overflow-hidden">
        {/* Search input */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
          <Search size={16} className="text-textMuted shrink-0" />
          <input
            ref={inputRef}
            type="text"
            className="flex-1 bg-transparent text-sm text-text outline-none placeholder:text-textMuted"
            placeholder="Type a command..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
          <kbd className="text-[10px] text-textMuted bg-bg px-1.5 py-0.5 rounded border border-border hidden sm:inline-block">
            ESC
          </kbd>
        </div>

        {/* Results */}
        <div className="max-h-[320px] overflow-y-auto py-1">
          {filtered.length === 0 ? (
            <div className="px-4 py-6 text-center text-textMuted text-xs">
              No commands found
            </div>
          ) : (
            filtered.map((cmd, idx) => (
              <button
                key={cmd.id}
                className={`w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors ${
                  idx === selectedIndex
                    ? "bg-primary/10 text-primary"
                    : "text-text hover:bg-surfaceLight"
                }`}
                onClick={cmd.action}
                onMouseEnter={() => setSelectedIndex(idx)}
              >
                <Command size={14} className="shrink-0" />
                <div className="flex-1 min-w-0">
                  <div className="text-sm truncate">
                    {highlightMatch(cmd.label, query)}
                  </div>
                </div>
                <span className="text-[10px] text-textMuted shrink-0">
                  {cmd.category}
                </span>
              </button>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center gap-3 px-4 py-2 border-t border-border text-[10px] text-textMuted">
          <span>
            <kbd className="bg-bg px-1 py-0.5 rounded border border-border text-[9px]">↑↓</kbd> Navigate
          </span>
          <span>
            <kbd className="bg-bg px-1 py-0.5 rounded border border-border text-[9px]">Enter</kbd> Select
          </span>
          <span>
            <kbd className="bg-bg px-1 py-0.5 rounded border border-border text-[9px]">Esc</kbd> Close
          </span>
        </div>
      </div>
    </div>
  );
}
