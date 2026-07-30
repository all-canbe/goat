import { useState, useEffect, useCallback } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { FolderTree, GitCompare, MonitorPlay, Loader2 } from "lucide-react";
import FileTree from "../sidebar/FileTree";
import ChangesPanel from "../sidebar/ChangesPanel";
import PreviewPanel from "../sidebar/PreviewPanel";
import { useWorkspaceStore } from "../../stores/workspaceStore";

const TABS = [
  { id: "files", label: "Files", icon: FolderTree },
  { id: "changes", label: "Changes", icon: GitCompare },
  { id: "preview", label: "Preview", icon: MonitorPlay },
] as const;

type TabId = (typeof TABS)[number]["id"];

interface RightPanelProps {
  onAddWorkspace?: () => void;
  width: number;
  onWidthChange: (w: number) => void;
  onCollapse: () => void;
}

const MIN_WIDTH = 240;
const MAX_WIDTH = 600;

export default function RightPanel({
  onAddWorkspace,
  width,
  onWidthChange,
  onCollapse,
}: RightPanelProps) {
  const [activeTab, setActiveTab] = useState<TabId>("files");
  const previewTarget = useWorkspaceStore((s) => s.previewTarget);
  const workspacePath = useWorkspaceStore((s) => s.workspace?.path);
  const pendingWorkspacePath = useWorkspaceStore((s) => s.pendingWorkspacePath);

  useEffect(() => {
    if (previewTarget) setActiveTab("preview");
  }, [previewTarget]);

  // 拖拽分隔条：mousedown 记录起点，mousemove 在 document 级监听并更新 width，mouseup 清理
  const startResize = useCallback(
    (e: ReactMouseEvent<HTMLDivElement>) => {
      e.preventDefault();
      const startX = e.clientX;
      const startWidth = width;

      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";

      const onMove = (ev: MouseEvent) => {
        // 分隔条在左侧：鼠标向左拖动（deltaX 负）应增大 width
        const delta = startX - ev.clientX;
        const next = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startWidth + delta));
        onWidthChange(next);
      };
      const onUp = () => {
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
        document.removeEventListener("mousemove", onMove);
        document.removeEventListener("mouseup", onUp);
      };
      document.addEventListener("mousemove", onMove);
      document.addEventListener("mouseup", onUp);
    },
    [width, onWidthChange],
  );

  return (
    <div className="flex shrink-0">
      <div
        onMouseDown={startResize}
        className="w-1 cursor-col-resize bg-transparent hover:bg-border transition-colors shrink-0"
      />
      <aside
        className="flex flex-col bg-surface border-l border-border shrink-0"
        style={{ width }}
      >
        <div className="flex items-center px-2 h-9 border-b border-border">
          <div className="flex items-center gap-1">
            {TABS.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`p-1.5 rounded transition-colors ${
                  activeTab === tab.id
                    ? "bg-surface-active text-text"
                    : "text-text-secondary hover:bg-surface-hover"
                }`}
                aria-label={tab.label}
                title={tab.label}
              >
                <tab.icon size={14} />
              </button>
            ))}
          </div>
          <button
            onClick={onCollapse}
            className="relative inline-flex items-center justify-center h-9 px-1.5 ml-1 rounded-lg border-0 bg-transparent shadow-none text-text hover:bg-surface-hover transition-colors"
            aria-label="收起侧边栏"
            title="收起侧边栏"
          >
            <svg
              viewBox="0 0 1024 1024"
              xmlns="http://www.w3.org/2000/svg"
              className="w-3.5 h-3.5"
              fill="currentColor"
            >
              <path d="M192 64c-35.328 0-65.536 12.48-90.496 37.504C76.544 126.464 64 156.672 64 192v640c0 35.328 12.48 65.536 37.504 90.496 24.96 24.96 55.168 37.504 90.496 37.504h640c35.328 0 65.536-12.48 90.496-37.504 24.96-24.96 37.504-55.168 37.504-90.496V192c0-35.328-12.48-65.536-37.504-90.496A123.328 123.328 0 0 0 832 64H192z m0 64h192v768H192a61.632 61.632 0 0 1-45.248-18.752A61.696 61.696 0 0 1 128 832V192c0-17.664 6.272-32.768 18.752-45.248A61.696 61.696 0 0 1 192 128z m640 768H448V128h384c17.664 0 32.768 6.272 45.248 18.752A61.632 61.632 0 0 1 896 192v640a61.632 61.632 0 0 1-18.752 45.248A61.632 61.632 0 0 1 832 896zM567.424 545.92l148.672 148.672 45.248-45.248L624 512l137.344-137.408-45.248-45.248L567.424 478.08q-14.08 14.08-14.08 33.92 0 19.84 14.08 33.92z" />
            </svg>
          </button>
        </div>
        <div className="flex-1 overflow-y-auto">
          {activeTab === "files" &&
            (pendingWorkspacePath ? (
              // 延迟切换期间：显示等待占位，避免快切闪烁
              <div className="flex items-center justify-center py-8 text-text-secondary">
                <Loader2 size={16} className="animate-spin mr-2" />
                <span className="text-xs">5 秒后加载工作区文件树...</span>
              </div>
            ) : !workspacePath ? (
              // 会话无关联工作区：清空文件树
              <div className="flex items-center justify-center py-8 text-text-secondary text-xs">
                该会话无关联工作区
              </div>
            ) : (
              // workspacePath 作为 key：workspace 切换时强制 FileTree 重新挂载并重新拉取
              <FileTree
                key={workspacePath}
                max_depth={1}
                onAddWorkspace={onAddWorkspace}
              />
            ))}
          {activeTab === "changes" && <ChangesPanel />}
          {activeTab === "preview" && <PreviewPanel />}
        </div>
      </aside>
    </div>
  );
}
