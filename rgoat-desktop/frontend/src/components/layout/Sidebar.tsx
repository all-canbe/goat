import { useState } from "react";
import {
  Plus,
  MessageSquare,
  Trash2,
  Pencil,
  Check,
  X,
} from "lucide-react";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";
import type { Session } from "../../stores/sessionStore";
import FileTree from "../sidebar/FileTree";

const TABS = ["Sessions", "Files", "Tasks"] as const;
type Tab = (typeof TABS)[number];

export default function Sidebar() {
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
    <aside className="w-[260px] bg-surface border-r border-border flex flex-col shrink-0">
      {/* Logo */}
      <div className="px-3 pt-3 pb-2">
        <div className="flex items-center gap-2 mb-2">
          <div className="w-6 h-6 rounded bg-primary flex items-center justify-center">
            <span className="text-white text-xs font-bold">R</span>
          </div>
          <span className="font-bold text-sm text-text">RGoat</span>
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
                : "text-textMuted hover:text-text"
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
                className="flex items-center gap-2 w-full px-3 py-2 rounded-md bg-primary/10 hover:bg-primary/20 text-primary text-sm font-medium transition-colors"
              >
                <Plus size={16} />
                <span>New Session</span>
              </button>
            </div>

            {/* Session list */}
            <div className="flex-1 overflow-y-auto px-2 pb-2">
              {sessions.length === 0 ? (
                <div className="text-center text-textMuted text-xs mt-8">
                  No sessions yet
                </div>
              ) : (
                <div className="flex flex-col gap-0.5">
                  {sessions.map((session) => (
                    <div
                      key={session.id}
                      onClick={() => handleSelectSession(session)}
                      className={`group flex items-center gap-2 px-3 py-2 rounded-md cursor-pointer transition-colors ${
                        activeSessionId === session.id
                          ? "bg-primary/10 text-primary"
                          : "hover:bg-surfaceLight text-textMuted hover:text-text"
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
                          <div className="text-[10px] text-textMuted">
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
                            className="p-0.5 rounded hover:bg-surfaceLight text-textMuted hover:text-text"
                            title="Rename"
                          >
                            <Pencil size={12} />
                          </button>
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              handleDelete(session.id);
                            }}
                            className="p-0.5 rounded hover:bg-surfaceLight text-textMuted hover:text-error"
                            title="Delete"
                          >
                            <Trash2 size={12} />
                          </button>
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}

        {activeTab === "Files" && <FileTree max_depth={3} />}

        {activeTab === "Tasks" && (
          <div className="flex items-center justify-center py-8 text-textMuted">
            <span className="text-xs">No active tasks</span>
          </div>
        )}
      </div>
    </aside>
  );
}
