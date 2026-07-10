import { useState } from "react";
import {
  Loader2,
  CheckCircle2,
  XCircle,
  ChevronDown,
  ChevronRight,
  Wrench,
} from "lucide-react";

interface ToolCallCardProps {
  toolName: string;
  arguments: Record<string, unknown>;
  success?: boolean;
  output?: string;
}

export default function ToolCallCard({
  toolName,
  arguments: args,
  success,
  output,
}: ToolCallCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [resultExpanded, setResultExpanded] = useState(false);

  const argsString = JSON.stringify(args, null, 2);
  const argsSummary = Object.entries(args)
    .map(([k, v]) => {
      const sv = String(v);
      return `${k}=${sv.length > 30 ? sv.slice(0, 30) + "..." : sv}`;
    })
    .join(", ");

  const isPending = success === undefined;
  const isSuccess = success === true;

  const statusColor = isPending
    ? "border-warning/40 bg-warning/5"
    : isSuccess
      ? "border-success/40 bg-success/5"
      : "border-error/40 bg-error/5";

  const statusIcon = isPending ? (
    <Loader2 size={14} className="text-warning animate-spin shrink-0" />
  ) : isSuccess ? (
    <CheckCircle2 size={14} className="text-success shrink-0" />
  ) : (
    <XCircle size={14} className="text-error shrink-0" />
  );

  return (
    <div
      className={`border rounded-md ${statusColor} mb-1 text-xs overflow-hidden`}
    >
      {/* Header */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 w-full px-2.5 py-1.5 text-left hover:bg-white/5 transition-colors"
      >
        <Wrench size={12} className="text-textMuted shrink-0" />
        {statusIcon}
        <span className="font-mono font-medium text-text truncate">
          {toolName}
        </span>
        <span className="text-textMuted truncate flex-1">
          {argsSummary || "no args"}
        </span>
        {expanded ? (
          <ChevronDown size={12} className="text-textMuted shrink-0" />
        ) : (
          <ChevronRight size={12} className="text-textMuted shrink-0" />
        )}
      </button>

      {/* Arguments */}
      {expanded && (
        <div className="px-2.5 pb-1.5 border-t border-border/50">
          <pre className="mt-1.5 text-xs text-textMuted font-mono whitespace-pre-wrap break-all">
            {argsString}
          </pre>
        </div>
      )}

      {/* Result output */}
      {!isPending && output !== undefined && (
        <div className="border-t border-border/50">
          <button
            onClick={() => setResultExpanded(!resultExpanded)}
            className="flex items-center gap-1 w-full px-2.5 py-1 text-left hover:bg-white/5 transition-colors"
          >
            {resultExpanded ? (
              <ChevronDown size={10} className="text-textMuted shrink-0" />
            ) : (
              <ChevronRight size={10} className="text-textMuted shrink-0" />
            )}
            <span
              className={isSuccess ? "text-success" : "text-error"}
            >
              {isSuccess ? "Result" : "Error"}
            </span>
          </button>
          {resultExpanded && (
            <pre className="px-2.5 pb-1.5 text-xs text-textMuted font-mono whitespace-pre-wrap break-all max-h-40 overflow-y-auto">
              {output}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
