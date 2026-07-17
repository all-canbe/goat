import { useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Check, Copy, AlertCircle } from "lucide-react";
import type { ChatMessage } from "../../stores/chatStore";
import ToolCallCard from "./ToolCallCard";

interface MessageBubbleProps {
  message: ChatMessage;
}

function CodeBlock({ children, className }: { children?: React.ReactNode; className?: string }) {
  const [copied, setCopied] = useState(false);
  const code = String(children || "").replace(/\n$/, "");

  // P1: 代码块复制按钮 — 1.5s 后恢复 Copy 图标
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, [code]);

  // P1: 右上角悬浮复制按钮（hover 显示），点击复制到剪贴板
  return (
    <div className="relative group my-2">
      <pre className="bg-bg-elevated border border-border rounded-lg overflow-x-auto p-3 pr-10">
        <code className={className}>{children}</code>
      </pre>
      <button
        onClick={handleCopy}
        title="Copy code"
        className="absolute top-1.5 right-1.5 p-1 rounded text-text-secondary hover:text-text hover:bg-surface-hover opacity-0 group-hover:opacity-100 transition-opacity"
      >
        {copied ? (
          <Check size={14} className="text-success" />
        ) : (
          <Copy size={14} />
        )}
      </button>
    </div>
  );
}

export default function MessageBubble({ message }: MessageBubbleProps) {
  switch (message.type) {
    case "user":
      return (
        <div className="flex justify-end mb-1">
          <div className="max-w-[80%] px-4 py-2.5 rounded-xl rounded-tr-sm bg-surface-active text-text text-sm">
            {message.content}
          </div>
        </div>
      );

    case "assistant":
      return (
        <div className="flex justify-start mb-1 gap-3">
          <div className="w-7 h-7 rounded-full bg-primary-subtle flex items-center justify-center shrink-0">
            <span className="text-xs font-semibold text-brand">R</span>
          </div>
          <div className="max-w-[85%] text-sm text-text prose prose-invert prose-sm max-w-none">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              rehypePlugins={[rehypeHighlight]}
              components={{
                code({ className, children }) {
                  const hasLanguage = /language-(\w+)/.exec(className || "");
                  if (hasLanguage) {
                    return <CodeBlock className={className}>{children}</CodeBlock>;
                  }
                  // Inline code or code without language
                  const hasNewline = typeof children === "string" && children.includes("\n");
                  if (hasNewline && className) {
                    return <CodeBlock className={className}>{children}</CodeBlock>;
                  }
                  return (
                    <code className="bg-surface-hover px-1.5 py-0.5 rounded text-xs font-mono text-text-secondary">
                      {children}
                    </code>
                  );
                },
              }}
            >
              {message.content || ""}
            </ReactMarkdown>
          </div>
        </div>
      );

    case "tool_call":
    case "tool_result":
      return (
        <div className="flex justify-start mb-1">
          <div className="max-w-[85%] w-full">
            <ToolCallCard
              toolName={message.toolName || "unknown"}
              arguments={message.arguments || {}}
              success={message.success}
              output={message.output}
            />
          </div>
        </div>
      );

    case "system":
      return (
        <div className="flex justify-center mb-1">
          <span className="text-xs text-text-tertiary px-2 py-0.5">
            {message.content}
          </span>
        </div>
      );

    case "error":
      return (
        <div className="flex justify-start mb-1">
          <div className="max-w-[85%] px-3 py-2 rounded-md border border-error/40 bg-error-subtle text-sm flex items-start gap-2">
            <AlertCircle size={16} className="text-error shrink-0 mt-0.5" />
            <span className="text-error">{message.content}</span>
          </div>
        </div>
      );

    default:
      return null;
  }
}
