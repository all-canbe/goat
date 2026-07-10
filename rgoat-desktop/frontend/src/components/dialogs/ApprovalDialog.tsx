import { useState, useEffect, useCallback } from "react";
import { Shield, ShieldAlert, ShieldOff, Loader2, CheckCircle2, XCircle } from "lucide-react";
import { useChatStore } from "../../stores/chatStore";
import { tauriInvoke } from "../../lib/tauri-bridge";

function getRiskInfo(decision: string) {
  switch (decision) {
    case "Allow":
      return {
        label: "Low Risk",
        color: "text-success",
        bg: "bg-success/10 border-success/40",
        icon: <Shield size={18} className="text-success" />,
      };
    case "Block":
      return {
        label: "Blocked",
        color: "text-error",
        bg: "bg-error/10 border-error/40",
        icon: <ShieldOff size={18} className="text-error" />,
      };
    case "Defer":
      return {
        label: "Deferred",
        color: "text-warning",
        bg: "bg-warning/10 border-warning/40",
        icon: <ShieldAlert size={18} className="text-warning" />,
      };
    default:
      return {
        label: "Approval Required",
        color: "text-warning",
        bg: "bg-warning/10 border-warning/40",
        icon: <ShieldAlert size={18} className="text-warning" />,
      };
  }
}

export default function ApprovalDialog() {
  const { pendingApproval, setApproval } = useChatStore();

  const [status, setStatus] = useState<"idle" | "loading" | "success" | "error">("idle");
  const [errorMessage, setErrorMessage] = useState("");

  // Reset status when new approval appears
  useEffect(() => {
    if (pendingApproval) {
      setStatus("idle");
      setErrorMessage("");
    }
  }, [pendingApproval]);

  const respond = useCallback(
    async (approved: boolean) => {
      if (!pendingApproval || status === "loading") return;

      setStatus("loading");
      try {
        await tauriInvoke("respond_approval", {
          tool_name: pendingApproval.tool_name,
          approved,
        });
        setStatus("success");
        // Close after a brief delay to show success
        setTimeout(() => {
          setApproval(null);
        }, 600);
      } catch (err) {
        setStatus("error");
        setErrorMessage(err instanceof Error ? err.message : "Failed to respond");
      }
    },
    [pendingApproval, status, setApproval]
  );

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!pendingApproval) return;
      if (e.key === "y" || e.key === "Y") {
        respond(true);
      } else if (e.key === "n" || e.key === "N") {
        respond(false);
      } else if (e.key === "Escape") {
        setApproval(null);
      }
    },
    [pendingApproval, setApproval, respond]
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  if (!pendingApproval) return null;

  const risk = getRiskInfo(pendingApproval.decision);

  const argsString = pendingApproval.arguments
    ? JSON.stringify(pendingApproval.arguments, null, 2)
    : "";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg shadow-2xl w-full max-w-md mx-4 overflow-hidden">
        {/* Header */}
        <div className={`flex items-center gap-3 px-4 py-3 border-b ${risk.bg}`}>
          {risk.icon}
          <div>
            <div className="text-sm font-semibold text-text">
              Tool Approval
            </div>
            <div className={`text-xs ${risk.color}`}>
              {risk.label} — {pendingApproval.decision}
            </div>
          </div>
        </div>

        {/* Body */}
        <div className="px-4 py-3 space-y-3">
          <div>
            <div className="text-xs text-textMuted mb-1">Tool</div>
            <div className="text-sm text-text font-mono bg-bg rounded px-2 py-1">
              {pendingApproval.tool_name}
            </div>
          </div>

          {pendingApproval.message && (
            <div>
              <div className="text-xs text-textMuted mb-1">Message</div>
              <div className="text-sm text-text">{pendingApproval.message}</div>
            </div>
          )}

          {argsString && (
            <div>
              <div className="text-xs text-textMuted mb-1">Arguments</div>
              <pre className="text-xs text-textMuted font-mono bg-bg rounded p-2 whitespace-pre-wrap break-all max-h-32 overflow-y-auto">
                {argsString}
              </pre>
            </div>
          )}

          {/* Status indicator */}
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

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border bg-bg/50">
          <button
            onClick={() => respond(false)}
            disabled={status === "loading"}
            className="px-4 py-1.5 rounded-md text-xs font-medium text-textMuted hover:text-text border border-border hover:border-textMuted transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
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
              "Approve (Y)"
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
