import { useState, useMemo } from "react";
import {
  Plus,
  MessageSquare,
  Trash2,
  Pencil,
  Check,
  X,
  Search,
  GitBranch,
} from "lucide-react";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";
import { useToastStore } from "../../stores/toastStore";
import type { Session } from "../../stores/sessionStore";

interface SidebarProps {
  // 折叠状态由 AppLayout 控制
  collapsed: boolean;
}

// 会话树节点
interface SessionNode {
  session: Session;
  children: SessionNode[];
  depth: number;
}

export default function Sidebar({ collapsed }: SidebarProps) {
  const {
    sessions,
    activeSessionId,
    selectSession,
    createSession,
    deleteSession,
    renameSession,
    forkSession,
  } = useSessionStore();
  const { clearMessages } = useChatStore();
  const { addToast } = useToastStore();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editTitle, setEditTitle] = useState("");
  // P1: 会话搜索
  const [searchQuery, setSearchQuery] = useState("");
  // 右键菜单
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    sessionId: string;
  } | null>(null);

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

  function handleContextMenu(e: React.MouseEvent, session: Session) {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, sessionId: session.id });
  }

  async function handleForkFromMenu() {
    if (!contextMenu) return;
    try {
      await forkSession(contextMenu.sessionId);
      clearMessages();
      addToast("会话已 Fork 并切换", "success");
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      addToast(`Fork 失败：${errorMsg}`, "error");
    }
    setContextMenu(null);
  }

  // 构建会话树：用 Map 索引，root 节点 parent_session_id 为空或指向不存在的会话
  const sessionTree = useMemo(() => {
    const map = new Map<string, SessionNode>();
    sessions.forEach((s) => {
      map.set(s.id, { session: s, children: [], depth: 0 });
    });
    const roots: SessionNode[] = [];
    sessions.forEach((s) => {
      const node = map.get(s.id)!;
      if (s.parent_session_id && map.has(s.parent_session_id)) {
        map.get(s.parent_session_id)!.children.push(node);
      } else {
        roots.push(node);
      }
    });
    // 设置 depth
    const setDepth = (nodes: SessionNode[], depth: number) => {
      nodes.forEach((n) => {
        n.depth = depth;
        setDepth(n.children, depth + 1);
      });
    };
    setDepth(roots, 0);
    return roots;
  }, [sessions]);

  // 搜索时保留匹配节点的祖先路径（让用户能看到分支上下文）
  const filteredTree = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return sessionTree;

    // 收集所有匹配节点 id
    const matchedIds = new Set<string>();
    sessions.forEach((s) => {
      if (s.title.toLowerCase().includes(q)) {
        matchedIds.add(s.id);
      }
    });

    // 对每个匹配节点，把其所有祖先也加入结果
    const ancestorIds = new Set<string>();
    const collectAncestors = (id: string) => {
      const s = sessions.find((x) => x.id === id);
      if (s?.parent_session_id) {
        ancestorIds.add(s.parent_session_id);
        collectAncestors(s.parent_session_id);
      }
    };
    matchedIds.forEach(collectAncestors);

    // 重建树，只保留 matchedIds + ancestorIds
    const visibleIds = new Set([...matchedIds, ...ancestorIds]);
    const filterNodes = (nodes: SessionNode[]): SessionNode[] => {
      return nodes
        .filter((n) => visibleIds.has(n.session.id))
        .map((n) => ({ ...n, children: filterNodes(n.children) }));
    };
    return filterNodes(sessionTree);
  }, [sessionTree, searchQuery, sessions]);

  // 扁平化树为数组用于渲染（避免递归组件）
  const flattenedNodes = useMemo(() => {
    const result: SessionNode[] = [];
    const walk = (nodes: SessionNode[]) => {
      nodes.forEach((n) => {
        result.push(n);
        walk(n.children);
      });
    };
    walk(filteredTree);
    return result;
  }, [filteredTree]);

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
              className="absolute left-2 top-1/2 -translate-y-1/2 text-text-secondary pointer-events-none"
            />
            <input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search sessions..."
              className="w-full pl-7 pr-2 py-1.5 rounded-lg bg-bg border border-border text-xs text-text placeholder:text-text-tertiary focus:border-primary focus-visible:ring-2 focus-visible:ring-ring"
            />
          </div>
        </div>

        {/* Session list（树形渲染） */}
        <div className="flex-1 overflow-y-auto px-2 pb-2">
          {sessions.length === 0 ? (
            <div className="text-center text-text-secondary text-xs mt-8">
              No sessions yet
            </div>
          ) : flattenedNodes.length === 0 ? (
            <div className="text-center text-text-secondary text-xs mt-8">
              No matching sessions
            </div>
          ) : (
            <div className="flex flex-col gap-0.5">
              {flattenedNodes.map((node) => {
                const session = node.session;
                const isActive = activeSessionId === session.id;
                return (
                  <div
                    key={session.id}
                    onClick={() => handleSelectSession(session)}
                    onContextMenu={(e) => handleContextMenu(e, session)}
                    style={{ paddingLeft: `${12 + node.depth * 20}px` }}
                    className={`group flex items-center gap-2 py-2 pr-3 rounded-md cursor-pointer transition-colors relative ${
                      isActive
                        ? "bg-primary-subtle text-brand"
                        : "hover:bg-surface-hover text-text-secondary hover:text-text"
                    }`}
                  >
                    {isActive && (
                      <span className="absolute left-0 top-2 bottom-2 w-0.5 rounded-full bg-primary" />
                    )}
                    {/* depth > 0 时左侧画竖线表示分支 */}
                    {node.depth > 0 && (
                      <span className="absolute left-3 top-0 bottom-0 w-px bg-border" />
                    )}
                    <MessageSquare size={14} className="shrink-0" />

                    <div className="flex-1 min-w-0">
                      {editingId === session.id ? (
                        <div className="flex items-center gap-1">
                          <input
                            className="flex-1 bg-bg border border-border rounded px-1 py-0.5 text-xs text-text focus:border-primary focus-visible:ring-2 focus-visible:ring-ring"
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
                            title="Confirm"
                          >
                            <Check size={12} />
                          </button>
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              cancelRename();
                            }}
                            className="text-error hover:text-error/80"
                            title="Cancel"
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
                          aria-label="Rename session"
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
                          aria-label="Delete session"
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* 右键菜单：Fork session */}
      {contextMenu && (
        <>
          <div
            className="fixed inset-0 z-40"
            onClick={() => setContextMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setContextMenu(null);
            }}
          />
          <div
            className="fixed z-50 bg-surface border border-border rounded-md shadow-lg py-1 min-w-[160px] outline-none"
            style={{ left: contextMenu.x, top: contextMenu.y }}
            tabIndex={-1}
            ref={(el) => el?.focus()}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                setContextMenu(null);
              }
            }}
          >
            <button
              onClick={handleForkFromMenu}
              className="w-full text-left px-3 py-1.5 text-xs hover:bg-surface-hover text-text flex items-center gap-2"
            >
              <GitBranch size={12} />
              Fork session
            </button>
          </div>
        </>
      )}
    </aside>
  );
}
