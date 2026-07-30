import { useState, useEffect } from "react";
import { Loader2, AlertCircle, X } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { tauriInvoke } from "../../lib/tauri-bridge";
import { useWorkspaceStore } from "../../stores/workspaceStore";

export default function PreviewPanel() {
  const previewTarget = useWorkspaceStore((s) => s.previewTarget);
  const closePreview = useWorkspaceStore((s) => s.closePreview);
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const isMd = previewTarget?.name.match(/\.(md|markdown)$/i);
  const isHtml = previewTarget?.name.match(/\.(html?)$/i);

  useEffect(() => {
    if (!previewTarget) {
      setContent(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    setContent(null);
    tauriInvoke<string>("read_workspace_file", { path: previewTarget.path })
      .then(setContent)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  }, [previewTarget]);

  // 无目标
  if (!previewTarget) {
    return (
      <div className="flex items-center justify-center h-full text-text-secondary text-xs">
        选择 HTML/MD 文件右键预览
      </div>
    );
  }

  // 加载中
  if (loading) {
    return (
      <div className="flex items-center justify-center h-full text-text-secondary">
        <Loader2 size={16} className="animate-spin mr-2" />
        <span className="text-xs">加载中...</span>
      </div>
    );
  }

  // 错误
  if (error) {
    return (
      <div className="flex items-center justify-center h-full p-4">
        <div className="flex items-center gap-2 text-error text-xs">
          <AlertCircle size={14} />
          <span>{error}</span>
        </div>
      </div>
    );
  }

  // 内容渲染
  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center justify-between px-3 py-2 border-b border-border">
        <span className="text-xs text-text-secondary truncate max-w-[200px]">
          {previewTarget.name}
        </span>
        <button
          onClick={closePreview}
          className="p-0.5 rounded hover:bg-surface-hover text-text-secondary hover:text-text transition-colors"
        >
          <X size={14} />
        </button>
      </div>
      <div className={isHtml ? "flex-1 overflow-hidden" : "flex-1 overflow-y-auto p-3"}>
        {isHtml && content ? (
          <iframe
            className="w-full h-full bg-white rounded"
            srcDoc={content}
            sandbox="allow-scripts allow-same-origin"
            title={previewTarget.name}
            style={{ minHeight: 0 }}
          />
        ) : isMd && content ? (
          <div className="prose prose-invert prose-sm max-w-none">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              rehypePlugins={[rehypeHighlight]}
            >
              {content}
            </ReactMarkdown>
          </div>
        ) : (
          <div className="text-text-secondary text-xs">
            不支持预览此文件类型
          </div>
        )}
      </div>
    </div>
  );
}