// D1-T05: ApprovalDialog 重写 — 按 tool_type 分视图 + ApprovalScope 选择 + 行级 diff

import { useState, useEffect, useCallback } from "react";
import {
  Shield,
  ShieldAlert,
  ShieldOff,
  Loader2,
  CheckCircle2,
  XCircle,
  Terminal,
  FileEdit,
  Globe,
  Wrench,
} from "lucide-react";
import { useChatStore, type ApprovalData, type ApprovalScope } from "../../stores/chatStore";
import DiffViewer from "../diff/DiffViewer";
import DiffSummary from "../diff/DiffSummary";
import ApprovalScopeSelector from "./ApprovalScopeSelector";

// ── 风险等级展示 ─────────────────────────────────────────────────

function getRiskInfo(riskLevel: string, dangerScore: number) {
  const level = riskLevel.toUpperCase();
  if (level === "HIGH" || dangerScore >= 70) {
    return {
      label: "HIGH RISK",
      color: "text-error",
      bg: "bg-error-subtle border-error/40",
      bar: "bg-error",
      icon: <ShieldOff size={18} className="text-error" />,
    };
  }
  if (level === "MEDIUM" || dangerScore >= 30) {
    return {
      label: "MEDIUM RISK",
      color: "text-warning",
      bg: "bg-warning-subtle border-warning/40",
      bar: "bg-warning",
      icon: <ShieldAlert size={18} className="text-warning" />,
    };
  }
  return {
    label: "LOW RISK",
    color: "text-success",
    bg: "bg-success-subtle border-success/40",
    bar: "bg-success",
    icon: <Shield size={18} className="text-success" />,
  };
}

// ── 按 tool_type 分视图 ─────────────────────────────────────────

function WriteApprovalView({ data }: { data: ApprovalData }) {
  const path = data.path || (data.affected_files && data.affected_files[0]) || "(unknown)";
  return (
    <div className="space-y-2">
      <div>
        <div className="text-xs text-text-secondary mb-1">File</div>
        <div className="text-sm text-text font-mono bg-bg rounded px-2 py-1 break-all">
          {path}
        </div>
      </div>
      {data.diff ? (
        <div>
          <div className="flex items-center justify-between mb-1">
            <div className="text-xs text-text-secondary">Diff Preview</div>
            <DiffSummary diff={data.diff} />
          </div>
          <DiffViewer diff={data.diff} maxHeightClass="max-h-80" />
        </div>
      ) : (
        <div className="text-xs text-text-secondary italic">No diff available</div>
      )}
    </div>
  );
}

function ShellApprovalView({ data }: { data: ApprovalData }) {
  return (
    <div className="space-y-2">
      <div>
        <div className="text-xs text-text-secondary mb-1">Command</div>
        <pre className="text-xs text-warning font-mono bg-bg rounded p-2 whitespace-pre-wrap break-all border border-warning/30">
          {data.command || "(empty)"}
        </pre>
      </div>
      <div className="text-xs text-text-secondary">
        Shell 命令将对系统产生影响，请确认命令内容安全。
      </div>
    </div>
  );
}

function NetworkApprovalView({ data }: { data: ApprovalData }) {
  return (
    <div className="space-y-2">
      <div>
        <div className="text-xs text-text-secondary mb-1">URL</div>
        <div className="text-sm text-text font-mono bg-bg rounded px-2 py-1 break-all">
          {data.url || "(unknown)"}
        </div>
      </div>
      <div className="text-xs text-text-secondary">
        网络请求将访问外部资源，请确认目标地址可信。
      </div>
    </div>
  );
}

function GenericApprovalView({ data }: { data: ApprovalData }) {
  const argsString = data.arguments
    ? JSON.stringify(data.arguments, null, 2)
    : "";
  return (
    <div className="space-y-2">
      {data.summary && (
        <div>
          <div className="text-xs text-text-secondary mb-1">Summary</div>
          <div className="text-sm text-text">{data.summary}</div>
        </div>
      )}
      {argsString && (
        <div>
          <div className="text-xs text-text-secondary mb-1">Arguments</div>
          <pre className="text-xs text-text-secondary font-mono bg-bg rounded p-2 whitespace-pre-wrap break-all max-h-60 overflow-y-auto">
            {argsString}
          </pre>
        </div>
      )}
    </div>
  );
}

// ── 主组件 ───────────────────────────────────────────────────────

export default function ApprovalDialog() {
  const { pendingApproval, respondApproval, setApproval } = useChatStore();

  const [status, setStatus] = useState<"idle" | "loading" | "success" | "error">("idle");
  const [errorMessage, setErrorMessage] = useState("");
  const [scope, setScope] = useState<ApprovalScope>("once");

  // 新审批出现时重置状态
  useEffect(() => {
    if (pendingApproval) {
      setStatus("idle");
      setErrorMessage("");
      // 默认选中 allow_options 第一个
      const first = pendingApproval.allow_options?.[0];
      setScope((first as ApprovalScope) || "once");
    }
  }, [pendingApproval]);

  const respond = useCallback(
    async (approved: boolean, scopeOverride?: ApprovalScope) => {
      if (!pendingApproval || status === "loading") return;
      const finalScope = scopeOverride || scope;
      setStatus("loading");
      try {
        await respondApproval(approved, finalScope);
        setStatus("success");
        setTimeout(() => {
          setApproval(null);
        }, 400);
      } catch (err) {
        setStatus("error");
        setErrorMessage(err instanceof Error ? err.message : "Failed to respond");
      }
    },
    [pendingApproval, status, scope, respondApproval, setApproval]
  );

  // 键盘快捷键：Y/A/S/W/N/Esc
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!pendingApproval) return;
      // P1 修复：Esc 等价于 Deny，必须向后端发送拒绝决策，否则 Agent 会永久挂起
      if (e.key === "Escape") {
        respond(false, "once");
        return;
      }
      const opts = pendingApproval.allow_options || [];
      const k = e.key.toLowerCase();
      // P2 修复：Y 也需检查 allow_options（与其他快捷键一致）
      if (k === "y" && opts.includes("once")) respond(true, "once");
      else if (k === "a" && opts.includes("session")) respond(true, "session");
      else if (k === "s" && opts.includes("all_similar")) respond(true, "all_similar");
      else if (k === "w" && opts.includes("always")) respond(true, "always");
      else if (k === "n") respond(false, "once");
    },
    [pendingApproval, respond, setApproval]
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  if (!pendingApproval) return null;

  const risk = getRiskInfo(pendingApproval.risk_level, pendingApproval.danger_score);

  // 选择 tool_type 对应视图
  const toolType = (pendingApproval.tool_type || "").toUpperCase();
  let viewNode;
  let toolTypeIcon;
  if (toolType === "WRITE" || pendingApproval.diff || pendingApproval.path) {
    viewNode = <WriteApprovalView data={pendingApproval} />;
    toolTypeIcon = <FileEdit size={14} className="text-text-secondary" />;
  } else if (toolType === "SHELL" || pendingApproval.command) {
    viewNode = <ShellApprovalView data={pendingApproval} />;
    toolTypeIcon = <Terminal size={14} className="text-text-secondary" />;
  } else if (toolType === "NETWORK" || pendingApproval.url) {
    viewNode = <NetworkApprovalView data={pendingApproval} />;
    toolTypeIcon = <Globe size={14} className="text-text-secondary" />;
  } else {
    viewNode = <GenericApprovalView data={pendingApproval} />;
    toolTypeIcon = <Wrench size={14} className="text-text-secondary" />;
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg shadow-2xl w-full max-w-2xl mx-4 overflow-hidden flex flex-col max-h-[90vh]">
        {/* Header — 风险徽章 + danger_score 进度条 + tool_name + tool_type */}
        <div className={`flex items-center gap-3 px-4 py-3 border-b ${risk.bg}`}>
          {risk.icon}
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-text truncate">
                {pendingApproval.tool_name}
              </span>
              {toolTypeIcon}
              <span className="text-[11px] text-text-secondary uppercase">
                {pendingApproval.tool_type || "other"}
              </span>
            </div>
            <div className={`text-xs ${risk.color}`}>{risk.label}</div>
          </div>
          {/* danger_score 进度条 */}
          <div className="flex items-center gap-2 shrink-0">
            <span className="text-[10px] text-text-secondary">danger</span>
            <div className="w-16 h-1.5 bg-bg rounded-full overflow-hidden">
              <div
                className={`h-full ${risk.bar} transition-all`}
                style={{ width: `${Math.min(pendingApproval.danger_score, 100)}%` }}
              />
            </div>
            <span className={`text-[10px] font-mono ${risk.color}`}>
              {pendingApproval.danger_score}
            </span>
          </div>
        </div>

        {/* Body — 按 tool_type 分视图 */}
        <div className="px-4 py-3 space-y-3 overflow-y-auto flex-1">
          {pendingApproval.summary && (
            <div className="text-sm text-text">{pendingApproval.summary}</div>
          )}
          {viewNode}

          {/* 状态指示 */}
          {status === "success" && (
            <div className="flex items-center gap-2 text-success text-xs">
              <CheckCircle2 size={14} />
              <span>Response sent</span>
            </div>
          )}
          {status === "error" && (
            <div className="flex items-center gap-2 text-error text-xs">
              <XCircle size={14} />
              <span>{errorMessage || "Failed to respond"}</span>
            </div>
          )}
        </div>

        {/* Footer — ApprovalScopeSelector + Deny/Approve */}
        <div className="px-4 py-3 border-t border-border bg-bg/50 space-y-2">
          <div className="flex items-center gap-2">
            <span className="text-[11px] text-text-secondary shrink-0">Scope:</span>
            <ApprovalScopeSelector
              allowOptions={pendingApproval.allow_options || ["once"]}
              value={scope}
              onChange={(s) => setScope(s as ApprovalScope)}
              disabled={status === "loading"}
            />
          </div>
          <div className="flex items-center justify-end gap-2">
            <button
              onClick={() => respond(false)}
              disabled={status === "loading"}
              className="px-4 py-1.5 rounded-md text-xs font-medium text-text-secondary hover:text-text border border-border hover:border-text-secondary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {status === "loading" ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                "Deny (N)"
              )}
            </button>
            <button
              onClick={() => respond(true)}
              disabled={status === "loading"}
              className="px-4 py-1.5 rounded-md text-xs font-medium text-white bg-primary hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {status === "loading" ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                `Approve (${(scope[0] || "Y").toUpperCase()})`
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
