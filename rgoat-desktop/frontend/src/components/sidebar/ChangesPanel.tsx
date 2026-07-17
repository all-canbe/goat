// D1-T06: Changes 面板 — 列出会话内所有文件变更 + Clear All + 展开查看 diff

import { useState, useEffect } from "react";
import { Trash2 } from "lucide-react";
import { useChangesStore } from "../../stores/changesStore";
import { useSessionStore } from "../../stores/sessionStore";
import ChangeItem from "./ChangeItem";

export default function ChangesPanel() {
  const { activeSessionId } = useSessionStore();
  const { changesBySession, clearChanges, loadChanges } = useChangesStore();
  const [expandedIdx, setExpandedIdx] = useState<number | null>(null);

  const changes = activeSessionId ? changesBySession[activeSessionId] || [] : [];

  // 切换会话时从后端重新加载
  useEffect(() => {
    if (activeSessionId) {
      loadChanges(activeSessionId);
      setExpandedIdx(null);
    }
  }, [activeSessionId, loadChanges]);

  function toggle(idx: number) {
    setExpandedIdx((prev) => (prev === idx ? null : idx));
  }

  function handleClear() {
    if (activeSessionId) {
      clearChanges(activeSessionId);
      setExpandedIdx(null);
    }
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-3 pt-2 pb-1">
        <span className="text-[11px] text-text-secondary">
          Changes ({changes.length})
        </span>
        {changes.length > 0 && (
          <button
            onClick={handleClear}
            className="p-1 rounded hover:bg-surface-hover text-text-secondary hover:text-error transition-colors"
            title="Clear all changes"
          >
            <Trash2 size={12} />
          </button>
        )}
      </div>

      {/* 列表 / 空状态 */}
      <div className="flex-1 overflow-y-auto">
        {!activeSessionId ? (
          <div className="text-center text-text-secondary text-xs mt-8 px-3">
            No active session
          </div>
        ) : changes.length === 0 ? (
          <div className="text-center text-text-secondary text-xs mt-8 px-3">
            No changes yet
          </div>
        ) : (
          <div>
            {changes.map((change, idx) => (
              <ChangeItem
                key={`${change.file_path}-${change.timestamp}-${idx}`}
                change={change}
                index={idx}
                expanded={expandedIdx === idx}
                onToggle={toggle}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
