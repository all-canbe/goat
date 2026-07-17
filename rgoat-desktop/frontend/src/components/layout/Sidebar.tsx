import { useState } from "react";
import {
  Plus,
  MessageSquare,
  Trash2,
  Pencil,
  Check,
  X,
  PanelLeftClose,
  Search,
} from "lucide-react";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";
import type { Session } from "../../stores/sessionStore";
import FileTree from "../sidebar/FileTree";
import ChangesPanel from "../sidebar/ChangesPanel";

// D1-T06: 新增 Changes tab
const TABS = ["Sessions", "Files", "Changes", "Tasks"] as const;
type Tab = (typeof TABS)[number];

interface SidebarProps {
  // P1: 折叠状态控制
  collapsed: boolean;
  onToggleCollapse: () => void;
}

export default function Sidebar({ collapsed, onToggleCollapse }: SidebarProps) {
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
  const [activeTab, setActiveTab] = useState<Tab>("Sessions");
  // P1: 会话搜索
  const [searchQuery, setSearchQuery] = useState("");

  function handleNewSession() {
    clearMessages();
    createSession();
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

  return (
    <aside
      className={`bg-surface flex flex-col shrink-0 transition-all duration-200 ${
        collapsed
          ? "w-0 overflow-hidden border-r-0"
          : "w-[260px] border-r border-border"
      }`}
    >
      {/* Logo */}
      <div className="px-3 pt-3 pb-2">
        <div className="flex items-center gap-2 mb-2">
          <div className="w-6 h-6 rounded bg-primary flex items-center justify-center">
            <span className="text-white text-xs font-bold">R</span>
          </div>
          <span className="font-bold text-sm text-text flex-1">RGoat</span>
          {/* P1: 折叠按钮 */}
          <button
            onClick={onToggleCollapse}
            title="折叠侧边栏"
            className="p-1 rounded text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
          >
            <PanelLeftClose size={16} />
          </button>
        </div>
      </div>

      {/* Tab bar */}
      <div className="flex items-center bg-bg border-b border-border mx-2 rounded-t-md overflow-hidden">
        {TABS.map((tab) => (
          <button
            key={tab}
            onClick={() => setActiveTab(tab)}
            className={`flex-1 px-2 py-1.5 text-[11px] font-medium transition-colors ${
              activeTab === tab
                ? "bg-primary text-white"
                : "text-text-secondary hover:text-text"
            }`}
          >
            {tab}
          </button>
        ))}
      </div>

      {/* Tab content */}
      <div className="flex-1 overflow-y-auto">
        {activeTab === "Sessions" && (
          <div className="flex flex-col h-full">
            {/* New Session button */}
            <div className="px-3 pt-2 pb-1">
              <button
                onClick={handleNewSession}
                className="flex items-center gap-2 w-full px-3 py-2 rounded-md bg-primary-subtle hover:bg-primary/20 text-brand text-sm font-medium transition-colors"
              >
                <Plus size={16} />
                <span>New Session</span>
              </button>
            </div>

            {/* P1: 会话搜索框 */}
            <div className="px-3 pb-1">
              <div className="relative">
                <Search
                  size={12}
                  className="absolute left-2 top-1/2 -translate-y-1/2 text-text-secondary pointer-events-none"
                />
                <input
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  placeholder="Search sessions..."
                  className="w-full pl-7 pr-2 py-1.5 rounded-md bg-bg border border-border text-xs text-text placeholder:text-text-tertiary outline-none focus:border-primary/50"
                />
              </div>
            </div>

            {/* Session list */}
            <div className="flex-1 overflow-y-auto px-2 pb-2">
              {sessions.length === 0 ? (
                <div className="text-center text-text-secondary text-xs mt-8">
                  No sessions yet
                </div>
              ) : (
                (() => {
                  // P1: 按 title 模糊匹配（不区分大小写）
                  const q = searchQuery.trim().toLowerCase();
                  const filtered = q
                    ? sessions.filter((s) =>
                        s.title.toLowerCase().includes(q)
                      )
                    : sessions;
                  if (filtered.length === 0) {
                    return (
                      <div className="text-center text-text-secondary text-xs mt-8">
                        No matching sessions
                      </div>
                    );
                  }
                  return (
                    <div className="flex flex-col gap-0.5">
                      {filtered.map((session) => (
                        <div
                          key={session.id}
                          onClick={() => handleSelectSession(session)}
                          className={`group flex items-center gap-2 px-3 py-2 rounded-md cursor-pointer transition-colors ${
                            activeSessionId === session.id
                              ? "bg-primary-subtle text-brand"
                              : "hover:bg-surface-hover text-text-secondary hover:text-text"
                          }`}
                        >
                          <MessageSquare size={14} className="shrink-0" />

                          <div className="flex-1 min-w-0">
                            {editingId === session.id ? (
                              <div className="flex items-center gap-1">
                                <input
                                  className="flex-1 bg-bg border border-border rounded px-1 py-0.5 text-xs text-text outline-none focus:border-primary"
                                  value={editTitle}
                                  onChange={(e) => setEditTitle(e.target.value)}
                                  onKeyDown={(e) => {
                                    if (e.key === "Enter") confirmRename(session.id);
                                    if (e.key === "Escape") cancelRename();
                                  }}
                                  autoFocus
                                  onClick={(e) => e.stopPropagation()}
                                />
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    confirmRename(session.id);
                                  }}
                                  className="text-success hover:text-success/80"
                                >
                                  <Check size={12} />
                                </button>
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    cancelRename();
                                  }}
                                  className="text-error hover:text-error/80"
                                >
                                  <X size={12} />
                                </button>
                              </div>
                            ) : (
                              <div className="text-xs truncate">{session.title}</div>
                            )}
                            {editingId !== session.id && (
                              <div className="text-[10px] text-text-secondary">
                                {session.message_count} messages
                              </div>
                            )}
                          </div>

                          {editingId !== session.id && (
                            <div className="hidden group-hover:flex items-center gap-0.5">
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  startRename(session);
                                }}
                                className="p-0.5 rounded hover:bg-surface-hover text-text-secondary hover:text-text"
                                title="Rename"
                              >
                                <Pencil size={12} />
                              </button>
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleDelete(session.id);
                                }}
                                className="p-0.5 rounded hover:bg-surface-hover text-text-secondary hover:text-error"
                                title="Delete"
                              >
                                <Trash2 size={12} />
                              </button>
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  );
                })()
              )}
            </div>
          </div>
        )}

        {activeTab === "Files" && <FileTree max_depth={3} />}

        {/* D1-T06: Changes 面板 */}
        {activeTab === "Changes" && <ChangesPanel />}

        {activeTab === "Tasks" && (
          <div className="flex items-center justify-center py-8 text-text-secondary">
            <span className="text-xs">No active tasks</span>
          </div>
        )}
      </div>
    </aside>
  );
}
