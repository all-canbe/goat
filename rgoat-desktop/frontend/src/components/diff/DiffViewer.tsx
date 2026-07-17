// D1-T04: 行级 diff 渲染主组件

import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { parseUnifiedDiff } from "../../lib/diff-utils";

interface DiffViewerProps {
  diff: string;
  /** 最大渲染行数（超出折叠为提示），默认 1000 */
  maxLines?: number;
  /** 是否默认展开所有上下文（默认 false，仅展示 3 行上下文） */
  defaultExpandAll?: boolean;
  /** 容器最大高度（Tailwind class），默认 max-h-96 */
  maxHeightClass?: string;
}

const CONTEXT_PADDING = 3;

export default function DiffViewer({
  diff,
  maxLines = 1000,
  defaultExpandAll = false,
  maxHeightClass = "max-h-96",
}: DiffViewerProps) {
  const [expandAll, setExpandAll] = useState(defaultExpandAll);

  const lines = useMemo(() => parseUnifiedDiff(diff), [diff]);

  if (lines.length === 0) {
    return (
      <div className="text-xs text-text-secondary italic px-3 py-2">
        无变更内容
      </div>
    );
  }

  const truncated = lines.length > maxLines;
  const visibleLines = truncated ? lines.slice(0, maxLines) : lines;

  // 折叠逻辑：若不展开全部，则只显示 hunk header 附近 CONTEXT_PADDING 行
  const renderedLines = expandAll
    ? visibleLines
    : visibleLines.filter((line, idx) => {
        if (line.type === 'hunk' || line.type === 'meta') return true;
        // 找到最近的 hunk header 距离
        let hunkIdx = -1;
        for (let i = idx; i >= 0; i--) {
          if (visibleLines[i].type === 'hunk') {
            hunkIdx = i;
            break;
          }
        }
        if (hunkIdx === -1) return true; // hunk 前的行全显示
        // 距离 hunk 起点的偏移
        const offsetFromHunk = idx - hunkIdx;
        // hunk header 后 CONTEXT_PADDING 行内的 add/del/context 全显示
        if (offsetFromHunk <= CONTEXT_PADDING) return true;
        // 之后只显示 add/del
        return line.type === 'add' || line.type === 'del';
      });

  return (
    <div className={`border border-border rounded-md overflow-hidden bg-bg ${maxHeightClass} overflow-y-auto`}>
      {/* 折叠控制条 */}
      <div className="sticky top-0 z-10 flex items-center justify-between px-3 py-1 bg-surface border-b border-border text-[10px] text-text-secondary">
        <span>{lines.length} 行</span>
        <button
          onClick={() => setExpandAll(!expandAll)}
          className="flex items-center gap-1 hover:text-text transition-colors"
        >
          {expandAll ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
          {expandAll ? "折叠上下文" : "展开全部"}
        </button>
      </div>

      {/* 行列表 */}
      <div className="font-mono text-[11px] leading-relaxed">
        {renderedLines.map((line, idx) => {
          const oldNo = line.oldLineNo ?? '';
          const newNo = line.newLineNo ?? '';

          if (line.type === 'meta') {
            return (
              <div key={idx} className="px-3 py-0.5 bg-surface-hover text-text-secondary border-l-2 border-border">
                {line.content}
              </div>
            );
          }

          if (line.type === 'hunk') {
            return (
              <div key={idx} className="px-3 py-0.5 bg-primary-subtle text-brand border-l-2 border-primary/40">
                {line.content}
              </div>
            );
          }

          // P2: 按行类型着色：add 绿色背景+左侧色条，del 红色背景+左侧色条
          const bgClass =
            line.type === 'add' ? 'bg-success-subtle' :
            line.type === 'del' ? 'bg-error-subtle' :
            'hover:bg-surface-hover/50';

          const borderClass =
            line.type === 'add' ? 'border-l-2 border-success' :
            line.type === 'del' ? 'border-l-2 border-error' :
            'border-l-2 border-transparent';

          const signClass =
            line.type === 'add' ? 'text-success' :
            line.type === 'del' ? 'text-error' :
            'text-text-secondary';

          const sign = line.type === 'add' ? '+' : line.type === 'del' ? '-' : ' ';

          return (
            <div key={idx} className={`flex ${bgClass} ${borderClass}`}>
              <span className="w-10 text-right pr-2 text-text-tertiary/50 select-none border-r border-border/40 shrink-0">
                {oldNo}
              </span>
              <span className="w-10 text-right pr-2 text-text-tertiary/50 select-none border-r border-border/40 shrink-0">
                {newNo}
              </span>
              <span className={`w-4 text-center select-none ${signClass} shrink-0`}>{sign}</span>
              <span className={`flex-1 pl-1 pr-2 whitespace-pre-wrap break-all ${
                line.type === 'add' ? 'text-success' :
                line.type === 'del' ? 'text-error' :
                'text-text'
              }`}>
                {line.content}
              </span>
            </div>
          );
        })}
      </div>

      {truncated && (
        <div className="px-3 py-2 bg-warning-subtle text-warning text-[10px] text-center">
          Diff 过大，已截断显示前 {maxLines} 行（共 {lines.length} 行）
        </div>
      )}
    </div>
  );
}
