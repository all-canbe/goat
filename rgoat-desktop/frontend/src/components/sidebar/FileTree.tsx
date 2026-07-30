import { useState, useEffect, useCallback, useRef } from "react";
import { ChevronRight, ChevronDown, File, Folder, FolderOpen, Loader2, FolderPlus, FilePlus, Eye } from "lucide-react";
import { tauriInvoke } from "../../lib/tauri-bridge";
import { useChatStore } from "../../stores/chatStore";
import { useWorkspaceStore } from "../../stores/workspaceStore";

interface FileTreeNode {
  name: string;
  path: string;
  is_dir: boolean;
  children?: FileTreeNode[];
}

interface FileTreeProps {
  /** 初始预加载深度（懒加载模式下默认只加载根目录一层） */
  max_depth?: number;
  onAddWorkspace?: () => void;
}

const EXTENSION_COLORS: Record<string, string> = {
  tsx: "text-blue-400",
  ts: "text-blue-400",
  jsx: "text-blue-300",
  js: "text-yellow-300",
  rs: "text-orange-400",
  py: "text-green-400",
  json: "text-yellow-400",
  toml: "text-orange-300",
  css: "text-cyan-400",
  html: "text-red-400",
  md: "text-purple-400",
  svg: "text-pink-400",
  lock: "text-text-secondary",
  gitignore: "text-text-secondary",
  yml: "text-yellow-300",
  yaml: "text-yellow-300",
};

function getExtensionColor(filename: string): string {
  const parts = filename.split(".");
  if (parts.length < 2) return "text-text-secondary";

  // Check special filenames
  const fullLower = filename.toLowerCase();
  if (fullLower === "package-lock.json" || fullLower === "yarn.lock" || fullLower === "cargo.lock") {
    return EXTENSION_COLORS["lock"] || "text-text-secondary";
  }
  if (fullLower === ".gitignore") {
    return EXTENSION_COLORS["gitignore"] || "text-text-secondary";
  }

  const ext = parts[parts.length - 1].toLowerCase();
  return EXTENSION_COLORS[ext] || "text-text-secondary";
}

interface TreeNodeProps {
  node: FileTreeNode;
  depth: number;
  max_depth: number;
  onContextMenu?: (e: React.MouseEvent, node: FileTreeNode) => void;
}

function TreeNode({ node, depth, max_depth, onContextMenu }: TreeNodeProps) {
  // 默认折叠：懒加载模式下用户点击展开才触发 list_directory
  const [expanded, setExpanded] = useState(false);
  // 懒加载子节点：undefined=未加载，[]=已加载但为空，FileTreeNode[]=已加载有内容
  const [localChildren, setLocalChildren] = useState<FileTreeNode[] | undefined>(undefined);
  const [loadingChildren, setLoadingChildren] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  // 优先用懒加载结果，其次用预加载的 node.children
  const effectiveChildren = localChildren ?? node.children;
  const hasChildren = node.is_dir && effectiveChildren && effectiveChildren.length > 0;
  // 是否已懒加载过（避免重复请求）
  const lazyLoaded = localChildren !== undefined;

  // 用 ref 存最新值，避免轮询导致 node prop 变化时 toggle 闭包过期
  const expandedRef = useRef(expanded);
  const lazyLoadedRef = useRef(lazyLoaded);
  const nodeRef = useRef(node);
  const localChildrenRef = useRef(localChildren);
  useEffect(() => {
    expandedRef.current = expanded;
  }, [expanded]);
  useEffect(() => {
    lazyLoadedRef.current = lazyLoaded;
  }, [lazyLoaded]);
  useEffect(() => {
    nodeRef.current = node;
  }, [node]);
  useEffect(() => {
    localChildrenRef.current = localChildren;
  }, [localChildren]);

  const toggle = useCallback(async () => {
    const currentNode = nodeRef.current;
    const currentExpanded = expandedRef.current;
    const currentLazyLoaded = lazyLoadedRef.current;

    if (!currentNode.is_dir) return;

    // 展开且未懒加载过且预加载 children 为空/undefined：触发懒加载
    const preloadedEmpty = !currentNode.children || currentNode.children.length === 0;
    if (!currentExpanded && !currentLazyLoaded && preloadedEmpty) {
      setLoadingChildren(true);
      setLoadError(null);
      try {
        const children = await tauriInvoke<FileTreeNode[]>("list_directory", {
          path: currentNode.path,
        });
        setLocalChildren(children);
        setExpanded(true);
      } catch (err) {
        setLoadError(err instanceof Error ? err.message : "Failed to load");
      } finally {
        setLoadingChildren(false);
      }
      return;
    }
    setExpanded((prev) => !prev);
  }, []);

  return (
    <div>
      <div
        className={`flex items-center gap-1 px-2 py-1 cursor-pointer rounded text-xs hover:bg-surface-hover transition-colors ${
          depth === 0 ? "" : ""
        }`}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
        onClick={toggle}
        onContextMenu={(e) => onContextMenu?.(e, node)}
      >
        {/* Expand/collapse icon */}
        {node.is_dir ? (
          <>
            <span className="shrink-0 text-text-secondary">
              {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            </span>
            <span className="shrink-0 text-yellow-300">
              {expanded ? <FolderOpen size={14} /> : <Folder size={14} />}
            </span>
          </>
        ) : (
          <>
            <span className="w-3 shrink-0" />
            <span className={`shrink-0 ${getExtensionColor(node.name)}`}>
              <File size={14} />
            </span>
          </>
        )}

        <span className="truncate text-text select-none">
          {node.name}
        </span>
      </div>

      {/* 加载中提示 */}
      {loadingChildren && (
        <div
          className="flex items-center gap-1 px-2 py-1 text-text-secondary"
          style={{ paddingLeft: `${(depth + 1) * 16 + 8}px` }}
        >
          <Loader2 size={12} className="animate-spin" />
          <span className="text-xs">Loading...</span>
        </div>
      )}

      {/* 加载失败提示 */}
      {loadError && (
        <div
          className="px-2 py-1 text-error text-xs"
          style={{ paddingLeft: `${(depth + 1) * 16 + 8}px` }}
        >
          {loadError}
        </div>
      )}

      {/* Children */}
      {expanded && hasChildren && !loadingChildren && (
        <div>
          {effectiveChildren!.map((child) => (
            <TreeNode
              key={child.path}
              node={child}
              depth={depth + 1}
              max_depth={max_depth}
              onContextMenu={onContextMenu}
            />
          ))}
        </div>
      )}

      {/* 空目录提示 */}
      {expanded && lazyLoaded && !hasChildren && !loadingChildren && (
        <div
          className="px-2 py-1 text-text-secondary text-xs"
          style={{ paddingLeft: `${(depth + 1) * 16 + 8}px` }}
        >
          空目录
        </div>
      )}
    </div>
  );
}

export default function FileTree({ max_depth = 1, onAddWorkspace }: FileTreeProps) {
  const [tree, setTree] = useState<FileTreeNode | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    node: FileTreeNode;
  } | null>(null);
  const workspace = useWorkspaceStore((s) => s.workspace);
  const addFileRef = useChatStore((s) => s.addFileRef);
  const openPreview = useWorkspaceStore((s) => s.openPreview);

  const closeContextMenu = useCallback(() => setContextMenu(null), []);

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, node: FileTreeNode) => {
      e.preventDefault();
      e.stopPropagation();
      setContextMenu({ x: e.clientX, y: e.clientY, node });
    },
    []
  );

  // 首次加载 + 5s 静默轮询（准实时，避免 FS watch 吃性能）
  useEffect(() => {
    let cancelled = false;

    async function fetchTree(silent: boolean) {
      if (!silent) {
        setLoading(true);
        setError(null);
      }
      try {
        const data = await tauriInvoke<FileTreeNode>("list_workspace_files", {
          max_depth,
        });
        if (!cancelled) {
          setTree(data);
          setError(null);
        }
      } catch (err) {
        if (!cancelled && !silent) {
          setError(err instanceof Error ? err.message : "Failed to load files");
        }
      } finally {
        if (!cancelled && !silent) {
          setLoading(false);
        }
      }
    }

    fetchTree(false);

    const POLL_MS = 5000;
    const timer = window.setInterval(() => {
      if (document.visibilityState === "hidden") return;
      fetchTree(true);
    }, POLL_MS);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [max_depth, workspace?.path]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8 text-text-secondary">
        <Loader2 size={16} className="animate-spin mr-2" />
        <span className="text-xs">Loading files...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center justify-center py-8 text-error">
        <span className="text-xs">{error}</span>
      </div>
    );
  }

  // 临时工作空间提示 + 添加工作空间按钮
  const temporaryBanner = workspace?.is_temporary ? (
    <div className="px-3 py-2 border-b border-border bg-warning-subtle">
      <div className="text-xs text-warning mb-1">临时工作空间</div>
      <button
        onClick={() => onAddWorkspace?.()}
        className="flex items-center gap-1 w-full px-2 py-1 rounded text-xs bg-surface-hover hover:bg-surface-active text-text transition-colors"
        aria-label="添加工作空间"
      >
        <FolderPlus size={12} />
        <span>添加工作空间</span>
      </button>
    </div>
  ) : null;

  const children = tree?.children ?? [];

  if (children.length === 0) {
    return (
      <div>
        {temporaryBanner}
        <div className="flex items-center justify-center py-8 text-text-secondary">
          <span className="text-xs">No files</span>
        </div>
      </div>
    );
  }

  return (
    <div>
      {temporaryBanner}
      <div className="py-1">
        {children.map((node) => (
          <TreeNode
            key={node.path}
            node={node}
            depth={0}
            max_depth={max_depth}
            onContextMenu={handleContextMenu}
          />
        ))}
      </div>
      {/* 右键菜单遮罩 + 面板 */}
      {contextMenu && (
        <>
          <div className="fixed inset-0 z-40" onClick={closeContextMenu} />
          <div
            className="fixed z-50 bg-surface border border-border rounded-lg shadow-md py-1 min-w-[160px]"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            <button
              onClick={() => {
                addFileRef({ name: contextMenu.node.name, path: contextMenu.node.path });
                closeContextMenu();
              }}
              className="w-full flex items-center gap-2 px-3 py-1.5 text-left text-xs text-text hover:bg-surface-hover transition-colors"
            >
              <FilePlus size={14} />
              <span>添加到对话</span>
            </button>
            {/\.(html?|md|markdown)$/i.test(contextMenu.node.name) && (
              <button
                onClick={() => {
                  openPreview(contextMenu.node.path, contextMenu.node.name);
                  closeContextMenu();
                }}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-left text-xs text-text hover:bg-surface-hover transition-colors"
              >
                <Eye size={14} />
                <span>预览</span>
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}
