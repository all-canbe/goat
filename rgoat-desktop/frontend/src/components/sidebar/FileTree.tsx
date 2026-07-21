import { useState, useEffect, useCallback } from "react";
import { ChevronRight, ChevronDown, File, Folder, FolderOpen, Loader2, FolderPlus } from "lucide-react";
import { tauriInvoke } from "../../lib/tauri-bridge";
import { useWorkspaceStore } from "../../stores/workspaceStore";

interface FileTreeNode {
  name: string;
  path: string;
  is_dir: boolean;
  children?: FileTreeNode[];
}

interface FileTreeProps {
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
}

function TreeNode({ node, depth, max_depth }: TreeNodeProps) {
  const [expanded, setExpanded] = useState(depth < 1);
  const hasChildren = node.is_dir && node.children && node.children.length > 0;
  const canExpand = node.is_dir && depth < max_depth - 1;

  const toggle = useCallback(() => {
    if (canExpand || hasChildren) {
      setExpanded((prev) => !prev);
    }
  }, [canExpand, hasChildren]);

  return (
    <div>
      <div
        className={`flex items-center gap-1 px-2 py-1 cursor-pointer rounded text-xs hover:bg-surface-hover transition-colors ${
          depth === 0 ? "" : ""
        }`}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
        onClick={toggle}
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

      {/* Children */}
      {expanded && hasChildren && (
        <div>
          {node.children!.map((child) => (
            <TreeNode
              key={child.path}
              node={child}
              depth={depth + 1}
              max_depth={max_depth}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export default function FileTree({ max_depth = 3, onAddWorkspace }: FileTreeProps) {
  const [tree, setTree] = useState<FileTreeNode | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const workspace = useWorkspaceStore((s) => s.workspace);

  useEffect(() => {
    let cancelled = false;
    async function fetchTree() {
      setLoading(true);
      setError(null);
      try {
        const data = await tauriInvoke<FileTreeNode>("list_workspace_files", {
          max_depth,
        });
        if (!cancelled) {
          setTree(data);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Failed to load files");
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }
    fetchTree();
    return () => {
      cancelled = true;
    };
  }, [max_depth]);

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
          />
        ))}
      </div>
    </div>
  );
}
