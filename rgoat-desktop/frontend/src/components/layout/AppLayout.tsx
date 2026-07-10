import { useState, useCallback, useEffect } from "react";
import Sidebar from "./Sidebar";
import ChatArea from "../chat/ChatArea";
import StatusBar from "../status/StatusBar";
import ApprovalDialog from "../dialogs/ApprovalDialog";
import CommandPalette from "../command/CommandPalette";
import { useAgentEvents } from "../../hooks/useAgentEvents";
import { useConfigStore } from "../../stores/configStore";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";

export default function AppLayout() {
  const { isStreaming } = useAgentEvents();
  const { currentProvider, loadProviders } = useConfigStore();
  const [currentMode, setCurrentMode] = useState("Agent");
  const [isCommandPaletteOpen, setCommandPaletteOpen] = useState(false);

  const { createSession } = useSessionStore();
  const { clearMessages } = useChatStore();

  const handleModeChange = useCallback((mode: string) => {
    setCurrentMode(mode);
  }, []);

  const handleNewSession = useCallback(() => {
    clearMessages();
    createSession();
  }, [clearMessages, createSession]);

  const handleClearMessages = useCallback(() => {
    clearMessages();
  }, [clearMessages]);

  // Global Ctrl+K listener
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key === "k") {
        e.preventDefault();
        setCommandPaletteOpen((prev) => !prev);
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  return (
    <div className="flex flex-col h-screen bg-bg text-text">
      {/* Header */}
      <header className="flex items-center gap-3 px-4 h-12 bg-surface border-b border-border shrink-0">
        <div className="flex items-center gap-2">
          <div className="w-5 h-5 rounded bg-primary flex items-center justify-center">
            <span className="text-white text-[10px] font-bold">R</span>
          </div>
          <span className="font-bold text-sm text-primary">RGoat</span>
        </div>
        <span className="text-[11px] px-2 py-0.5 rounded bg-surfaceLight text-textMuted">
          Desktop
        </span>
        <div className="flex-1" />
        <span className="text-xs text-textMuted">
          {currentProvider || "No provider"}
        </span>
      </header>

      {/* Main area */}
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <ChatArea onModeChange={handleModeChange} />
      </div>

      {/* Status bar */}
      <StatusBar isStreaming={isStreaming} currentMode={currentMode} />

      {/* Approval dialog */}
      <ApprovalDialog />

      {/* Command palette */}
      <CommandPalette
        isOpen={isCommandPaletteOpen}
        onClose={() => setCommandPaletteOpen(false)}
        onModeChange={handleModeChange}
        onNewSession={handleNewSession}
        onClearMessages={handleClearMessages}
      />
    </div>
  );
}
