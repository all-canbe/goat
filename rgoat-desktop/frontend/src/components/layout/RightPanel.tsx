import { useState } from "react";
import { FolderTree, GitCompare, MonitorPlay, Terminal } from "lucide-react";
import FileTree from "../sidebar/FileTree";
import ChangesPanel from "../sidebar/ChangesPanel";
import { useWorkspaceStore } from "../../stores/workspaceStore";

const TABS = [
  { id: "files", label: "Files", icon: FolderTree },
  { id: "changes", label: "Changes", icon: GitCompare },
  { id: "diff", label: "Diff", icon: GitCompare },
  { id: "preview", label: "Preview", icon: MonitorPlay },
  { id: "terminal", label: "Terminal", icon: Terminal },
] as const;

type TabId = (typeof TABS)[number]["id"];

interface RightPanelProps {
  onAddWorkspace?: () => void;
}

export default function RightPanel({ onAddWorkspace }: RightPanelProps) {
  const [activeTab, setActiveTab] = useState<TabId>("files");
  const workspacePath = useWorkspaceStore((s) => s.workspace?.path);

  return (
    <aside className="w-80 flex flex-col bg-surface border-l border-border shrink-0">
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
      </div>
      <div className="flex-1 overflow-y-auto">
        {activeTab === "files" && (
          // workspacePath 作为 key：workspace 切换时强制 FileTree 重新挂载并重新拉取
          <FileTree
            key={workspacePath ?? "no-workspace"}
            max_depth={3}
            onAddWorkspace={onAddWorkspace}
          />
        )}
        {activeTab === "changes" && <ChangesPanel />}
        {activeTab === "diff" && (
          <div className="flex items-center justify-center h-full text-text-secondary text-xs">
            Diff 未启用
          </div>
        )}
        {activeTab === "preview" && (
          <div className="flex items-center justify-center h-full text-text-secondary text-xs">
            Preview 未启用
          </div>
        )}
        {activeTab === "terminal" && (
          <div className="flex items-center justify-center h-full text-text-secondary text-xs">
            Terminal 未启用
          </div>
        )}
      </div>
    </aside>
  );
}
