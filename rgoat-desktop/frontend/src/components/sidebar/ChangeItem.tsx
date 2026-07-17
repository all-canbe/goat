// D1-T06: 单条文件变更项 — 图标 + 路径 + 行数统计 + 展开查看 diff

import { FilePlus, FileEdit, FileX, ChevronDown, ChevronRight } from "lucide-react";
import type { FileChangeRecord } from "../../stores/changesStore";
import DiffViewer from "../diff/DiffViewer";

interface ChangeItemProps {
  change: FileChangeRecord;
  index: number;
  expanded: boolean;
  onToggle: (idx: number) => void;
}

function getChangeIcon(type: string) {
  switch (type) {
    case "create":
      return <FilePlus size={12} className="text-success shrink-0" />;
    case "delete":
      return <FileX size={12} className="text-error shrink-0" />;
    case "edit":
    default:
      return <FileEdit size={12} className="text-warning shrink-0" />;
  }
}

function getChangeBadge(type: string) {
  const cls =
    type === "create"
      ? "bg-success-subtle text-success border-success/40"
      : type === "delete"
        ? "bg-error-subtle text-error border-error/40"
        : "bg-warning-subtle text-warning border-warning/40";
  return (
    <span
      className={`px-1 py-0.5 rounded text-[9px] font-mono uppercase border ${cls}`}
    >
      {type}
    </span>
  );
}

export default function ChangeItem({ change, index, expanded, onToggle }: ChangeItemProps) {
  const fileName = change.file_path.split(/[\\/]/).pop() || change.file_path;

  return (
    <div className="border-b border-border/40 last:border-b-0">
      <button
        onClick={() => onToggle(index)}
        className="flex items-center gap-2 w-full px-2 py-1.5 text-left hover:bg-surface-hover transition-colors"
      >
        {getChangeIcon(change.change_type)}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5">
            <span className="text-xs text-text font-mono truncate">{fileName}</span>
            {getChangeBadge(change.change_type)}
          </div>
          <div className="text-[10px] text-text-secondary truncate font-mono">
            {change.file_path}
          </div>
        </div>
        <div className="shrink-0 flex items-center gap-1.5 text-[10px] font-mono">
          <span className="text-success">+{change.additions}</span>
          <span className="text-error">-{change.deletions}</span>
        </div>
        {expanded ? (
          <ChevronDown size={12} className="text-text-secondary shrink-0" />
        ) : (
          <ChevronRight size={12} className="text-text-secondary shrink-0" />
        )}
      </button>
      {expanded && change.diff && (
        <div className="px-2 pb-2">
          <DiffViewer diff={change.diff} maxHeightClass="max-h-60" />
        </div>
      )}
    </div>
  );
}
