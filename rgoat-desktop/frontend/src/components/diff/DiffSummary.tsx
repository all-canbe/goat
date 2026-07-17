// D1-T04: 变更汇总（+X -Y 行数）

import { computeDiffStats } from "../../lib/diff-utils";

interface DiffSummaryProps {
  diff: string;
  /** 是否显示标签文字（默认 true） */
  showLabels?: boolean;
  className?: string;
}

export default function DiffSummary({
  diff,
  showLabels = true,
  className = "",
}: DiffSummaryProps) {
  const stats = computeDiffStats(diff);

  if (stats.additions === 0 && stats.deletions === 0) {
    return <span className={`text-text-secondary text-xs ${className}`}>无变更</span>;
  }

  return (
    <span className={`inline-flex items-center gap-2 text-xs font-mono ${className}`}>
      {stats.additions > 0 && (
        <span className="text-success">
          {showLabels ? `+${stats.additions}` : `+${stats.additions}`}
        </span>
      )}
      {stats.deletions > 0 && (
        <span className="text-error">
          {showLabels ? `-${stats.deletions}` : `-${stats.deletions}`}
        </span>
      )}
    </span>
  );
}
