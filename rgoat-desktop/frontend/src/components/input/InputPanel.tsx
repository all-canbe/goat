import { useState, useRef, useCallback, useEffect } from "react";
import { Send, ChevronDown } from "lucide-react";
import { tauriInvoke } from "../../lib/tauri-bridge";
import { useConfigStore } from "../../stores/configStore";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";

const MODES = ["Agent", "Plan", "Flow", "YOLO"] as const;
type Mode = (typeof MODES)[number];

interface InputPanelProps {
  onModeChange?: (mode: string) => void;
}

interface SendPromptResponse {
  session_id: string;
  message: string;
}

export default function InputPanel({ onModeChange }: InputPanelProps) {
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<Mode>("Agent");
  const [showProviderMenu, setShowProviderMenu] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const { providers, currentProvider, switchProvider, loadProviders } =
    useConfigStore();
  const { activeSessionId, setActiveSessionId } = useSessionStore();
  const { isStreaming, setStreaming, addUserMessage } = useChatStore();

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  const handleModeChange = useCallback(
    (newMode: Mode) => {
      setMode(newMode);
      onModeChange?.(newMode);
    },
    [onModeChange]
  );

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text || isStreaming) return;

    setInput("");
    addUserMessage(text);

    try {
      setStreaming(true);
      const res = await tauriInvoke<SendPromptResponse>("send_prompt", {
        prompt: text,
        session_id: activeSessionId || undefined,
        mode: mode.toLowerCase(),
      });
      if (res?.session_id) {
        setActiveSessionId(res.session_id);
      }
    } catch (err) {
      setStreaming(false);
      const errorMsg = err instanceof Error ? err.message : String(err);
      useChatStore.getState().addErrorMessage(errorMsg);
    }
  }, [
    input,
    isStreaming,
    activeSessionId,
    mode,
    addUserMessage,
    setStreaming,
    setActiveSessionId,
  ]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend]
  );

  const adjustHeight = useCallback(() => {
    const ta = textareaRef.current;
    if (ta) {
      ta.style.height = "auto";
      ta.style.height = Math.min(ta.scrollHeight, 200) + "px";
    }
  }, []);

  useEffect(() => {
    adjustHeight();
  }, [input, adjustHeight]);

  return (
    <footer className="flex gap-2 px-4 py-3 bg-surface border-t border-border shrink-0">
      {/* Provider selector */}
      <div className="relative">
        <button
          onClick={() => setShowProviderMenu(!showProviderMenu)}
          className="flex items-center gap-1 px-2.5 py-2 rounded-md bg-bg border border-border text-xs text-textMuted hover:border-primary transition-colors h-full whitespace-nowrap"
        >
          {currentProvider || "Select"}
          <ChevronDown size={12} />
        </button>

        {showProviderMenu && (
          <>
            <div
              className="fixed inset-0 z-10"
              onClick={() => setShowProviderMenu(false)}
            />
            <div className="absolute bottom-full left-0 mb-1 z-20 bg-surface border border-border rounded-md shadow-lg py-1 min-w-[180px]">
              {providers.map((p) => (
                <button
                  key={p.name}
                  onClick={() => {
                    switchProvider(p.name);
                    setShowProviderMenu(false);
                  }}
                  className={`block w-full text-left px-3 py-1.5 text-xs hover:bg-surfaceLight transition-colors ${
                    p.is_current ? "text-primary" : "text-textMuted"
                  }`}
                >
                  <div>{p.name}</div>
                  <div className="text-[10px] text-textMuted">{p.model}</div>
                </button>
              ))}
              {providers.length === 0 && (
                <div className="px-3 py-1.5 text-xs text-textMuted">
                  No providers
                </div>
              )}
            </div>
          </>
        )}
      </div>

      {/* Mode selector */}
      <div className="flex items-center bg-bg border border-border rounded-md overflow-hidden">
        {MODES.map((m) => (
          <button
            key={m}
            onClick={() => handleModeChange(m)}
            className={`px-2.5 py-2 text-xs font-medium transition-colors ${
              mode === m
                ? "bg-primary text-white"
                : "text-textMuted hover:text-text"
            }`}
          >
            {m}
          </button>
        ))}
      </div>

      {/* Textarea */}
      <textarea
        ref={textareaRef}
        className="flex-1 px-3 py-2 rounded-md bg-bg border border-border text-text text-sm resize-none outline-none focus:border-primary min-h-[36px] max-h-[200px]"
        placeholder="Ask anything... (Enter to send, Shift+Enter for newline)"
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKeyDown}
        disabled={isStreaming}
        rows={1}
      />

      {/* Send button */}
      <button
        onClick={handleSend}
        disabled={!input.trim() || isStreaming}
        className="px-4 py-2 rounded-md bg-primary text-white font-semibold text-sm hover:bg-primary/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors shrink-0 self-end"
      >
        {isStreaming ? (
          <span className="inline-block w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
        ) : (
          <Send size={16} />
        )}
      </button>
    </footer>
  );
}
