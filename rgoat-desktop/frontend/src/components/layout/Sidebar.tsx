import { useState, useMemo } from "react";
import {
  Plus,
  MessageSquare,
  Trash2,
  Pencil,
  Check,
  X,
  Search,
} from "lucide-react";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";
import type { Session } from "../../stores/sessionStore";

interface SidebarProps {
  // 折叠状态由 AppLayout 控制
  collapsed: boolean;
}

interface SessionItemProps {
  session: Session;
  isActive: boolean;
  editingId: string | null;
  editTitle: string;
  setEditTitle: (v: string) => void;
  onSelect: (s: Session) => void;
  onStartRename: (s: Session) => void;
  onConfirmRename: (id: string) => void;
  onCancelRename: () => void;
  onDelete: (id: string) => void;
}

function SessionItem({
  session,
  isActive,
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

  return (
    <div
      onClick={() => onSelect(session)}
      className={`group flex items-center gap-2 px-3 py-2 rounded-md cursor-pointer transition-colors relative ${
        isActive
          ? "bg-primary-subtle text-brand"
          : "hover:bg-surface-hover text-text-secondary hover:text-text"
      }`}
    >
      {isActive && (
        <span className="absolute left-0 top-2 bottom-2 w-0.5 rounded-full bg-primary" />
      )}
      <MessageSquare size={14} className="shrink-0" />

      <div className="flex-1 min-w-0">
        {isEditing ? (
          <div className="flex items-center gap-1">
            <input
              className="flex-1 bg-bg border border-border rounded px-1 py-0.5 text-xs text-text focus:border-primary focus-visible:ring-2 focus-visible:ring-ring"
              value={editTitle}
              onChange={(e) => setEditTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onConfirmRename(session.id);
                if (e.key === "Escape") onCancelRename();
              }}
              autoFocus
              onClick={(e) => e.stopPropagation()}
            />
            <button
              onClick={(e) => {
                e.stopPropagation();
                onConfirmRename(session.id);
              }}
              className="text-success hover:text-success/80"
              title="Confirm"
              aria-label="Confirm rename"
            >
              <Check size={12} />
            </button>
            <button
              onClick={(e) => {
                e.stopPropagation();
                onCancelRename();
              }}
              className="text-error hover:text-error/80"
              title="Cancel"
              aria-label="Cancel rename"
            >
              <X size={12} />
            </button>
          </div>
        ) : (
          <div className="text-xs truncate">{session.title}</div>
        )}
        {!isEditing && (
          <div className="text-[10px] text-text-secondary">
            {session.message_count} messages
          </div>
        )}
      </div>

      {!isEditing && (
        <div className="hidden group-hover:flex items-center gap-0.5">
          <button
            onClick={(e) => {
              e.stopPropagation();
              onStartRename(session);
            }}
            className="p-0.5 rounded hover:bg-surface-hover text-text-secondary hover:text-text"
            title="Rename"
            aria-label="Rename session"
          >
            <Pencil size={12} />
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              onDelete(session.id);
            }}
            className="p-0.5 rounded hover:bg-surface-hover text-text-secondary hover:text-error"
            title="Delete"
            aria-label="Delete session"
          >
            <Trash2 size={12} />
          </button>
        </div>
      )}
    </div>
  );
}

function startOfDay(date: Date): Date {
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  return d;
}

function diffDays(a: Date, b: Date): number {
  const msPerDay = 1000 * 60 * 60 * 24;
  return Math.floor(
    (startOfDay(a).getTime() - startOfDay(b).getTime()) / msPerDay
  );
}

type GroupKey = "today" | "yesterday" | "last7days" | "earlier";

interface Group {
  key: GroupKey;
  label: string;
  sessions: Session[];
}

const GROUP_ORDER: GroupKey[] = ["today", "yesterday", "last7days", "earlier"];

function groupSessions(sessions: Session[]): Group[] {
  const now = new Date();
  const map: Record<GroupKey, Session[]> = {
    today: [],
    yesterday: [],
    last7days: [],
    earlier: [],
  };

  for (const s of sessions) {
    const days = diffDays(now, new Date(s.created_at));
    if (days <= 0) {
      map.today.push(s);
    } else if (days === 1) {
      map.yesterday.push(s);
    } else if (days <= 7) {
      map.last7days.push(s);
    } else {
      map.earlier.push(s);
    }
  }

  const labels: Record<GroupKey, string> = {
    today: "今天",
    yesterday: "昨天",
    last7days: "最近 7 天",
    earlier: "更早",
  };

  return GROUP_ORDER.map((key) => ({
    key,
    label: labels[key],
    sessions: map[key].sort(
      (a, b) =>
        new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
    ),
  })).filter((g) => g.sessions.length > 0);
}

export default function Sidebar({ collapsed }: SidebarProps) {
  const {
    sessions,
    activeSessionId,
    selectSession,
    createSession,
    deleteSession,
    renameSession,
  } = useSessionStore();
  const { clearMessages } = useChatStore();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editTitle, setEditTitle] = useState("");
  // P1: 会话搜索
  const [searchQuery, setSearchQuery] = useState("");

  async function handleNewSession() {
    clearMessages();
    try {
      await createSession();
    } catch {
      // createSession 失败时不阻断 UI，会话列表保持原状
    }
  }

  function handleSelectSession(session: Session) {
    clearMessages();
    selectSession(session.id);
  }

  function startRename(session: Session) {
    setEditingId(session.id);
    setEditTitle(session.title);
  }

  function cancelRename() {
    setEditingId(null);
    setEditTitle("");
  }

  function confirmRename(id: string) {
    if (editTitle.trim()) {
      renameSession(id, editTitle.trim());
    }
    setEditingId(null);
    setEditTitle("");
  }

  function handleDelete(id: string) {
    deleteSession(id);
  }

  const filtered = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => s.title.toLowerCase().includes(q));
  }, [searchQuery, sessions]);

  const groups = useMemo(() => groupSessions(filtered), [filtered]);

  return (
    <aside
      className={`bg-surface flex flex-col shrink-0 transition-all duration-200 ${
        collapsed
          ? "w-0 overflow-hidden border-r-0"
          : "w-[260px] border-r border-border"
      }`}
    >
      <div className="flex flex-col h-full">
        {/* New Session button */}
        <div className="px-3 pt-3 pb-2">
          <button
            onClick={handleNewSession}
            className="flex items-center gap-2 w-full px-3 py-2 rounded-lg bg-surface-hover hover:bg-surface-active text-text-secondary hover:text-text text-sm font-medium transition-colors"
            aria-label="新建会话"
          >
            <Plus size={16} />
            <span>New Session</span>
          </button>
        </div>

        {/* P1: 会话搜索框 */}
        <div className="px-3 pb-2">
          <div className="relative">
            <Search
              size={12}
              className="absolute left-2 top-1/2 -translate-y-1/2 text-text-tertiary pointer-events-none"
            />
            <input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search sessions..."
              className="w-full pl-7 pr-2 py-1.5 rounded-lg bg-surface-hover border border-transparent text-xs text-text placeholder:text-text-tertiary focus:bg-bg focus:border-border focus-visible:ring-0 transition-colors"
              aria-label="搜索会话"
            />
          </div>
        </div>

        {/* Session list */}
        <div className="flex-1 overflow-y-auto px-2 pb-2">
          {sessions.length === 0 ? (
            <div className="text-center text-text-secondary text-xs mt-8">
              No sessions yet
            </div>
          ) : filtered.length === 0 ? (
            <div className="text-center text-text-secondary text-xs mt-8">
              No matching sessions
            </div>
          ) : (
            <div className="flex flex-col">
              {groups.map((group) => (
                <div key={group.key} className="flex flex-col">
                  <div className="text-xs font-medium text-text-tertiary px-3 py-1.5">
                    {group.label}
                  </div>
                  <div className="flex flex-col gap-0.5">
                    {group.sessions.map((session) => (
                      <SessionItem
                        key={session.id}
                        session={session}
                        isActive={activeSessionId === session.id}
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
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}
