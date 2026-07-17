import { Wifi, WifiOff, ShieldAlert, FileEdit, Wrench, Coins } from "lucide-react";
import { useConfigStore } from "../../stores/configStore";
import { useSessionStore } from "../../stores/sessionStore";
import type { TokenUsage } from "../../stores/chatStore";

interface StatusBarProps {
  isStreaming: boolean;
  currentMode: string;
  // D1-T07: 审批/变更/工具调用统计
  pendingApprovals: number;
  totalChanges: number;
  toolCallCount: number;
  // P0-2: Token 用量
  tokenUsage: TokenUsage;
}

// P0-2: 格式化 token 数为紧凑显示
function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

// P0-2: 格式化成本
function formatCost(cost: number): string {
  if (cost >= 1) return `$${cost.toFixed(2)}`;
  if (cost >= 0.01) return `$${cost.toFixed(3)}`;
  if (cost > 0) return `$${cost.toFixed(4)}`;
  return "$0";
}

export default function StatusBar({
  isStreaming,
  currentMode,
  pendingApprovals,
  totalChanges,
  toolCallCount,
  tokenUsage,
}: StatusBarProps) {
  const { currentProvider, providers } = useConfigStore();
  const { sessions } = useSessionStore();

  const currentModel =
    providers.find((p) => p.is_current)?.model || currentProvider || "none";

  const totalTokens = tokenUsage.inputTokens + tokenUsage.outputTokens;

  return (
    <footer className="flex items-center gap-3 px-4 py-0.5 bg-surface border-t border-border text-xs text-text-secondary h-7 shrink-0">
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

      {/* D1-T07: 审批/变更/工具调用统计 */}
      <span className="text-border">|</span>
      <span
        className={`flex items-center gap-1 ${
          pendingApprovals > 0 ? "text-warning" : "text-text-secondary"
        }`}
        title="Pending approvals"
      >
        <ShieldAlert size={11} />
        <span>Approvals: {pendingApprovals}</span>
      </span>

      <span className="text-border">|</span>
      <span
        className={`flex items-center gap-1 ${
          totalChanges > 0 ? "text-text" : "text-text-secondary"
        }`}
        title="File changes in current session"
      >
        <FileEdit size={11} />
        <span>Changes: {totalChanges}</span>
      </span>

      <span className="text-border">|</span>
      <span className="flex items-center gap-1 text-text-secondary" title="Tool calls in current session">
        <Wrench size={11} />
        <span>Tools: {toolCallCount}</span>
      </span>

      {/* P0-2: Token 用量 + 成本估算 */}
      <span className="text-border">|</span>
      <span
        className={`flex items-center gap-1 ${totalTokens > 0 ? "text-text" : "text-text-secondary"}`}
        title={`Token 用量（本次会话）\nInput: ${tokenUsage.inputTokens}\nOutput: ${tokenUsage.outputTokens}\n估算成本: ${formatCost(tokenUsage.totalCost)}`}
      >
        <Coins size={11} />
        <span>
          {totalTokens > 0
            ? `${formatTokens(tokenUsage.inputTokens)}↓ ${formatTokens(tokenUsage.outputTokens)}↑ · ${formatCost(tokenUsage.totalCost)}`
            : "0 tokens"}
        </span>
      </span>

      <span className="flex-1" />

      {isStreaming ? (
        <Wifi size={12} className="text-warning" />
      ) : (
        <WifiOff size={12} className="text-text-secondary" />
      )}
    </footer>
  );
}
