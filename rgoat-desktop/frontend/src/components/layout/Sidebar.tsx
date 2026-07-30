import { useState, useMemo } from "react";
import {
  Plus,
  MessageSquare,
  Trash2,
  Pencil,
  Check,
  X,
  Search,
  Folder,
  ChevronRight,
  ChevronDown,
  Loader2,
} from "lucide-react";
import { useSessionStore } from "../../stores/sessionStore";
import type { Session } from "../../stores/sessionStore";
import { useToastStore } from "../../stores/toastStore";

interface SidebarProps {
  // 折叠状态由 AppLayout 控制
  collapsed: boolean;
  onCreateSessionForWorkspace: (workspace: string) => Promise<void>;
  onNewSession: () => void;
}

interface SessionItemProps {
  session: Session;
  isActive: boolean;
  isSelecting: boolean;
  isDeleting: boolean;
  editingId: string | null;
  editTitle: string;
  setEditTitle: (v: string) => void;
  onSelect: (s: Session) => void;
  onStartRename: (s: Session) => void;
  onConfirmRename: (id: string) => void;
  onCancelRename: () => void;
  onDelete: (id: string) => void;
}

function formatRelativeTime(dateStr: string): string {
  if (!dateStr) return "";
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / (1000 * 60));
  const diffHours = Math.floor(diffMs / (1000 * 60 * 60));
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffMins < 1) return "刚刚";
  if (diffMins < 60) return `${diffMins}m`;
  if (diffHours < 24) return `${diffHours}h`;
  if (diffDays === 1) return "昨天";
  if (diffDays < 7) return `${diffDays}d`;
  return `${date.getMonth() + 1}/${date.getDate()}`;
}

function cleanWorkspacePath(path: string): { basename: string; fullPath: string; isTemp: boolean } {
  if (!path || path === "临时工作空间") {
    return { basename: "临时工作空间", fullPath: "临时工作空间", isTemp: true };
  }
  const cleaned = path.replace(/^\\\\\?\\/, "");
  const isTemp = cleaned.includes("temporary-workspace");
  if (isTemp) {
    return { basename: "临时工作空间", fullPath: cleaned, isTemp: true };
  }
  const parts = cleaned.split(/[/\\]/).filter(Boolean);
  const basename = parts[parts.length - 1] || cleaned;
  return {
    basename,
    fullPath: cleaned,
    isTemp: false,
  };
}

function SessionItem({
  session,
  isActive,
  isSelecting,
  isDeleting,
  editingId,
  editTitle,
  setEditTitle,
  onSelect,
  onStartRename,
  onConfirmRename,
  onCancelRename,
  onDelete,
}: SessionItemProps) {
  const isEditing = editingId === session.id;
  const timeLabel = useMemo(() => formatRelativeTime(session.created_at), [session.created_at]);
  const disabled = isSelecting || isDeleting;

  return (
    <div
      onClick={() => !disabled && onSelect(session)}
      className={`group flex items-center gap-2 pl-3 pr-2.5 py-1.5 rounded-lg relative ${
        isActive ? "bg-primary-subtle/80 text-brand" : "text-text-secondary"
      } ${disabled ? "opacity-60 cursor-default" : "cursor-pointer"}`}
    >
      {/* 左侧蓝竖条：常驻占位，hover/active 显色，避免布局抖动 */}
      <span
        className={`absolute left-0 top-1.5 bottom-1.5 w-0.5 rounded-full bg-primary ${
          isActive ? "opacity-100" : "opacity-0 group-hover:opacity-100"
        }`}
      />
      {isSelecting ? (
        <Loader2 size={13} className="shrink-0 animate-spin text-text-tertiary" />
      ) : (
        <MessageSquare size={13} className="shrink-0 opacity-70" />
      )}

      <div className="flex-1 min-w-0">
        {isEditing ? (
          <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
            <input
              className="flex-1 bg-bg border border-border rounded px-1.5 py-0.5 text-xs text-text focus:border-primary focus-visible:ring-1 focus-visible:ring-ring outline-none"
              value={editTitle}
              onChange={(e) => setEditTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onConfirmRename(session.id);
                if (e.key === "Escape") onCancelRename();
              }}
              autoFocus
            />
            <button
              onClick={() => onConfirmRename(session.id)}
              className="text-success hover:text-success/80 p-0.5"
              title="Confirm"
              aria-label="Confirm rename"
            >
              <Check size={12} />
            </button>
            <button
              onClick={onCancelRename}
              className="text-error hover:text-error/80 p-0.5"
              title="Cancel"
              aria-label="Cancel rename"
            >
              <X size={12} />
            </button>
          </div>
        ) : (
          <div className="flex items-center justify-between gap-1 min-h-[18px]">
            <span className="text-xs truncate">{session.title}</span>
            <span className="text-[10px] text-text-tertiary shrink-0 w-8 text-right tabular-nums">
              {timeLabel}
            </span>
          </div>
        )}
      </div>

      {/* 操作按钮绝对定位，不挤占行高/宽度，避免 hover 抖动 */}
      {!isEditing && (
        <div className="absolute right-1 top-1/2 -translate-y-1/2 flex items-center gap-0.5 opacity-0 pointer-events-none group-hover:opacity-100 group-hover:pointer-events-auto bg-surface/95 rounded">
          <button
            onClick={(e) => {
              e.stopPropagation();
              onStartRename(session);
            }}
            disabled={disabled}
            className="p-1 rounded text-text-tertiary hover:text-text disabled:opacity-50 disabled:cursor-not-allowed"
            title="Rename"
            aria-label="Rename session"
          >
            <Pencil size={11} />
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              onDelete(session.id);
            }}
            disabled={disabled}
            className="p-1 rounded text-text-tertiary hover:text-error disabled:opacity-50 disabled:cursor-not-allowed"
            title="Delete"
            aria-label="Delete session"
          >
            {isDeleting ? (
              <Loader2 size={11} className="animate-spin" />
            ) : (
              <Trash2 size={11} />
            )}
          </button>
        </div>
      )}
    </div>
  );
}

const COLLAPSED_WORKSPACES_KEY = "collapsed_workspaces";

function readCollapsedWorkspaces(): Set<string> {
  try {
    const raw = localStorage.getItem(COLLAPSED_WORKSPACES_KEY);
    if (!raw) return new Set();
    return new Set(JSON.parse(raw) as string[]);
  } catch {
    return new Set();
  }
}

function saveCollapsedWorkspaces(set: Set<string>) {
  try {
    localStorage.setItem(COLLAPSED_WORKSPACES_KEY, JSON.stringify(Array.from(set)));
  } catch {
    // localStorage 不可用时忽略
  }
}

export default function Sidebar({ collapsed, onCreateSessionForWorkspace, onNewSession }: SidebarProps) {
  const {
    sessions,
    activeSessionId,
    selectingId,
    deletingId,
    selectSession,
    deleteSession,
    renameSession,
  } = useSessionStore();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editTitle, setEditTitle] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<Set<string>>(readCollapsedWorkspaces);
  const addToast = useToastStore((s) => s.addToast);

  async function handleSelectSession(session: Session) {
    if (selectingId || deletingId) return;
    try {
      await selectSession(session.id);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      addToast(`加载会话历史失败：${msg}`, "error");
    }
  }

  function startRename(session: Session) {
    setEditingId(session.id);
    setEditTitle(session.title);
  }

  function cancelRename() {
    setEditingId(null);
    setEditTitle("");
  }

  async function confirmRename(id: string) {
    if (!editTitle.trim()) {
      cancelRename();
      return;
    }
    try {
      await renameSession(id, editTitle.trim());
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      addToast(`重命名会话失败：${msg}`, "error");
    }
    setEditingId(null);
    setEditTitle("");
  }

  async function handleDelete(id: string) {
    if (selectingId || deletingId) return;
    try {
      await deleteSession(id);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      addToast(`删除会话失败：${msg}`, "error");
    }
  }

  function toggleWorkspace(key: string) {
    setCollapsedWorkspaces((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      saveCollapsedWorkspaces(next);
      return next;
    });
  }

  const filtered = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => s.title.toLowerCase().includes(q));
  }, [searchQuery, sessions]);

  // 按工作空间分组，并应用 cleanWorkspacePath 进行路径包装
  const workspaceGroups = useMemo(() => {
    const map = new Map<string, { info: ReturnType<typeof cleanWorkspacePath>; items: Session[] }>();
    for (const session of filtered) {
      const rawWs = session.workspace || "临时工作空间";
      const info = cleanWorkspacePath(rawWs);
      const key = info.fullPath;
      if (!map.has(key)) {
        map.set(key, { info, items: [] });
      }
      map.get(key)!.items.push(session);
    }
    return Array.from(map.values());
  }, [filtered]);

  return (
    <aside
      className={`bg-surface flex flex-col shrink-0 transition-all duration-200 select-none ${
        collapsed
          ? "w-0 overflow-hidden border-r-0"
          : "w-[260px] border-r border-border"
      }`}
    >
      <div className="flex flex-col h-full">
        {/* New Session button */}
        <div className="px-3 pt-3 pb-2">
          <button
            onClick={onNewSession}
            className="flex items-center gap-2 w-full px-3 py-2 rounded-lg bg-surface-hover hover:bg-surface-active text-text-secondary hover:text-text text-xs font-medium transition-colors border border-border/50"
            aria-label="新建会话"
          >
            <Plus size={15} />
            <span>New Session</span>
          </button>
        </div>

        {/* 会话搜索框 */}
        <div className="px-3 pb-2">
          <div className="relative">
            <Search
              size={12}
              className="absolute left-2.5 top-1/2 -translate-y-1/2 text-text-tertiary pointer-events-none"
            />
            <input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search sessions..."
              className="w-full pl-8 pr-2 py-1.5 rounded-lg bg-surface-hover border border-transparent text-xs text-text placeholder:text-text-tertiary focus:bg-bg focus:border-border focus-visible:ring-0 transition-colors"
              aria-label="搜索会话"
            />
          </div>
        </div>

        {/* Session list (参考图 2 优雅展示) */}
        <div className="flex-1 overflow-y-auto px-2 pb-2">
          {sessions.length === 0 ? (
            <div className="text-center text-text-tertiary text-xs mt-8">
              No sessions yet
            </div>
          ) : filtered.length === 0 ? (
            <div className="text-center text-text-tertiary text-xs mt-8">
              No matching sessions
            </div>
          ) : (
            <div className="flex flex-col gap-3 pt-1">
              {workspaceGroups.map(({ info, items }) => {
                const isCollapsed = collapsedWorkspaces.has(info.fullPath);
                return (
                  <div key={info.fullPath} className="flex flex-col gap-1">
                    {/* 工作空间 Header (参考图 2) */}
                    <div
                      className="flex items-center justify-between px-2 py-1 text-xs font-medium text-text-secondary group hover:text-text hover:bg-surface-hover cursor-pointer rounded transition-colors"
                      title={info.fullPath}
                      onClick={() => toggleWorkspace(info.fullPath)}
                    >
                      <div className="flex items-center gap-1.5 min-w-0">
                        {isCollapsed ? (
                          <ChevronRight size={14} className="shrink-0 text-text-tertiary group-hover:text-text transition-colors" />
                        ) : (
                          <ChevronDown size={14} className="shrink-0 text-text-tertiary group-hover:text-text transition-colors" />
                        )}
                        <Folder size={14} className="shrink-0 text-text-tertiary group-hover:text-primary transition-colors" />
                        <span className="truncate text-xs font-semibold">{info.basename}</span>
                      </div>
                      {!info.isTemp && (
                        <button
                          type="button"
                          aria-label={`在 ${info.basename} 新建会话`}
                          onClick={(e) => {
                            e.stopPropagation();
                            void onCreateSessionForWorkspace(info.fullPath);
                          }}
                          className="p-0.5 rounded hover:bg-surface-active text-text-tertiary hover:text-text transition-colors"
                          title="新建此工作空间会话"
                        >
                          <Plus size={13} />
                        </button>
                      )}
                    </div>

                    {/* 属于该工作区的会话列表 */}
                    {!isCollapsed && (
                      <div className="flex flex-col gap-0.5 pl-2">
                        {items.map((session) => (
                          <SessionItem
                            key={session.id}
                            session={session}
                            isActive={activeSessionId === session.id}
                            isSelecting={selectingId === session.id}
                            isDeleting={deletingId === session.id}
                            editingId={editingId}
                            editTitle={editTitle}
                            setEditTitle={setEditTitle}
                            onSelect={handleSelectSession}
                            onStartRename={startRename}
                            onConfirmRename={confirmRename}
                            onCancelRename={cancelRename}
                            onDelete={handleDelete}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}
