import { Wifi, WifiOff } from "lucide-react";
import { useConfigStore } from "../../stores/configStore";
import { useSessionStore } from "../../stores/sessionStore";

interface StatusBarProps {
  isStreaming: boolean;
  currentMode: string;
}

export default function StatusBar({ isStreaming, currentMode }: StatusBarProps) {
  const { currentProvider, providers } = useConfigStore();
  const { sessions } = useSessionStore();

  const currentModel =
    providers.find((p) => p.is_current)?.model || currentProvider || "none";

  return (
    <footer className="flex items-center gap-3 px-4 py-0.5 bg-surface border-t border-border text-xs text-textMuted h-7 shrink-0">
      <span
        className={`inline-block w-2 h-2 rounded-full ${
          isStreaming ? "bg-warning animate-pulse" : "bg-success"
        }`}
      />
      <span>{isStreaming ? "Streaming" : "Ready"}</span>

      <span className="text-border">|</span>
      <span>
        {currentProvider ? `${currentProvider} / ${currentModel}` : "No provider"}
      </span>

      <span className="text-border">|</span>
      <span>Mode: {currentMode}</span>

      <span className="text-border">|</span>
      <span>{sessions.length} session(s)</span>

      <span className="flex-1" />

      {isStreaming ? (
        <Wifi size={12} className="text-warning" />
      ) : (
        <WifiOff size={12} className="text-textMuted" />
      )}
    </footer>
  );
}
