import { Wifi, WifiOff, ShieldAlert, FileEdit, Coins, Wrench } from "lucide-react";
import { useConfigStore } from "../../stores/configStore";
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

  const currentModel =
    providers.find((p) => p.is_current)?.model || currentProvider || "none";

  const totalTokens = tokenUsage.inputTokens + tokenUsage.outputTokens;

  const providerModelLabel = currentProvider
    ? currentModel && currentModel !== currentProvider
      ? `${currentProvider} / ${currentModel}`
      : currentProvider
    : "No provider";

  return (
    <footer className="flex items-center gap-3 px-4 h-7 bg-bg-elevated border-t border-border text-2xs text-text-secondary shrink-0">
      {/* 左侧：状态指示灯 + 当前工作区 */}
      <span
        className={`inline-block w-2 h-2 rounded-full ${
          isStreaming ? "bg-warning animate-pulse" : "bg-success"
        }`}
      />
      <span>{isStreaming ? "Streaming" : "Ready"}</span>

      <span className="w-1 h-1 rounded-full bg-border-strong" />
      <span className="font-mono truncate max-w-[200px]">当前工作区</span>

      <span className="w-1 h-1 rounded-full bg-border-strong" />

      {/* 中部：Provider/Model + Mode */}
      <span className="font-mono truncate max-w-[260px]">
        {providerModelLabel}
      </span>

      <span className="w-1 h-1 rounded-full bg-border-strong" />
      <span>{currentMode}</span>

      <span className="flex-1" />

      {/* 右侧：Tokens / Changes（仅非零）/ Approvals（仅非零）/ WiFi */}
      <span
        className={`flex items-center gap-1 ${
          totalTokens > 0 ? "text-text" : ""
        }`}
        title={`Token 用量（本次会话）\nInput: ${tokenUsage.inputTokens}\nOutput: ${tokenUsage.outputTokens}\n估算成本: ${formatCost(tokenUsage.totalCost)}`}
      >
        <Coins size={11} />
        <span>
          {totalTokens > 0
            ? `${formatTokens(tokenUsage.inputTokens)}↓ ${formatTokens(tokenUsage.outputTokens)}↑ · ${formatCost(tokenUsage.totalCost)}`
            : "0 tokens"}
        </span>
      </span>

      {totalChanges > 0 && (
        <span className="flex items-center gap-1 text-text" title="File changes in current session">
          <FileEdit size={11} />
          <span>{totalChanges}</span>
        </span>
      )}

      {pendingApprovals > 0 && (
        <span className="flex items-center gap-1 text-warning" title="Pending approvals">
          <ShieldAlert size={11} />
          <span>{pendingApprovals}</span>
        </span>
      )}

      {toolCallCount > 0 && (
        <span className="flex items-center gap-1 text-text" title="Tool calls in current session">
          <Wrench size={11} />
          <span>Tools {toolCallCount}</span>
        </span>
      )}

      {isStreaming ? (
        <Wifi size={12} className="text-warning" aria-label="Streaming active" />
      ) : (
        <WifiOff size={12} className="text-text-tertiary" aria-label="Idle" />
      )}
    </footer>
  );
}
