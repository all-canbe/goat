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

// D1-T07: 从工具名推断风险等级
function getRiskBadge(toolName: string): {
  label: string;
  cls: string;
} | null {
  const name = toolName.toLowerCase();
  // HIGH: shell/bash/执行类
  if (/^(shell|bash|exec|cmd|terminal)/.test(name)) {
    return { label: "HIGH", cls: "bg-error-subtle text-error border-error/40" };
  }
  // MEDIUM: 写文件/删除类
  if (/^(write_file|edit_file|delete_file|remove_file|create_file|move_file)/.test(name)) {
    return { label: "MED", cls: "bg-warning-subtle text-warning border-warning/40" };
  }
  // LOW: 只读类
  if (/^(read_file|list_files|grep|glob|find|search)/.test(name)) {
    return { label: "LOW", cls: "bg-success-subtle text-success border-success/40" };
  }
  // 默认无徽章
  return null;
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
    ? "border-warning/40 bg-warning-subtle border-l-2 border-l-warning"
    : isSuccess
      ? "border-success/40 bg-success-subtle border-l-2 border-l-success"
      : "border-error/40 bg-error-subtle border-l-2 border-l-error";

  const statusIcon = isPending ? (
    <Loader2 size={14} className="text-warning animate-spin shrink-0" />
  ) : isSuccess ? (
    <CheckCircle2 size={14} className="text-success shrink-0" />
  ) : (
    <XCircle size={14} className="text-error shrink-0" />
  );

  // D1-T07: 风险徽章
  const risk = getRiskBadge(toolName);

  return (
    <div
      className={`border rounded-md ${statusColor} mb-1 text-xs overflow-hidden`}
    >
      {/* Header */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 w-full px-2.5 py-1.5 text-left hover:bg-surface-hover transition-colors"
      >
        <Wrench size={12} className="text-text-secondary shrink-0" />
        {statusIcon}
        <span className="font-mono font-medium text-text truncate">
          {toolName}
        </span>
        {/* D1-T07: 风险徽章 */}
        {risk && (
          <span
            className={`px-1 py-0.5 rounded text-[9px] font-mono font-semibold border shrink-0 ${risk.cls}`}
            title={`Risk: ${risk.label}`}
          >
            {risk.label}
          </span>
        )}
        <span className="text-text-secondary truncate flex-1">
          {argsSummary || "no args"}
        </span>
        {expanded ? (
          <ChevronDown size={12} className="text-text-secondary shrink-0" />
        ) : (
          <ChevronRight size={12} className="text-text-secondary shrink-0" />
        )}
      </button>

      {/* Arguments */}
      {expanded && (
        <div className="px-2.5 pb-1.5 border-t border-border/50">
          <pre className="mt-1.5 text-xs text-text-secondary font-mono whitespace-pre-wrap break-all">
            {argsString}
          </pre>
        </div>
      )}

      {/* Result output */}
      {!isPending && output !== undefined && (
        <div className="border-t border-border/50">
          <button
            onClick={() => setResultExpanded(!resultExpanded)}
            className="flex items-center gap-1 w-full px-2.5 py-1 text-left hover:bg-surface-hover transition-colors"
          >
            {resultExpanded ? (
              <ChevronDown size={10} className="text-text-secondary shrink-0" />
            ) : (
              <ChevronRight size={10} className="text-text-secondary shrink-0" />
            )}
            <span
              className={isSuccess ? "text-success" : "text-error"}
            >
              {isSuccess ? "Result" : "Error"}
            </span>
          </button>
          {resultExpanded && (
            <pre className="px-2.5 pb-1.5 text-xs text-text-secondary font-mono whitespace-pre-wrap break-all max-h-40 overflow-y-auto">
              {output}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
