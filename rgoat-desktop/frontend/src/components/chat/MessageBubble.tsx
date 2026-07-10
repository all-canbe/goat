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

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [code]);

  return (
    <div className="relative group my-2">
      <div className="flex items-center justify-between px-3 py-1.5 bg-[#161b22] rounded-t-md border-b border-border/30">
        <span className="text-xs text-textMuted font-mono">
          {className ? className.replace("language-", "") : "code"}
        </span>
        <button
          onClick={handleCopy}
          className="text-textMuted hover:text-text transition-colors"
          title="Copy code"
        >
          {copied ? (
            <Check size={14} className="text-success" />
          ) : (
            <Copy size={14} />
          )}
        </button>
      </div>
      <pre className="bg-[#0d1117] rounded-b-md overflow-x-auto p-3">
        <code className={className}>{children}</code>
      </pre>
    </div>
  );
}

export default function MessageBubble({ message }: MessageBubbleProps) {
  switch (message.type) {
    case "user":
      return (
        <div className="flex justify-end mb-1">
          <div className="max-w-[80%] px-4 py-2.5 rounded-lg bg-primary text-white text-sm">
            {message.content}
          </div>
        </div>
      );

    case "assistant":
      return (
        <div className="flex justify-start mb-1">
          <div className="max-w-[85%] px-4 py-2.5 rounded-lg bg-surface border border-border text-sm prose prose-invert prose-sm max-w-none">
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
                    <code className="bg-surfaceLight px-1.5 py-0.5 rounded text-xs font-mono text-warning">
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
          <span className="text-xs text-textMuted px-2 py-0.5">
            {message.content}
          </span>
        </div>
      );

    case "error":
      return (
        <div className="flex justify-start mb-1">
          <div className="max-w-[85%] px-3 py-2 rounded-md border border-error/40 bg-error/5 text-sm flex items-start gap-2">
            <AlertCircle size={16} className="text-error shrink-0 mt-0.5" />
            <span className="text-error/90">{message.content}</span>
          </div>
        </div>
      );

    default:
      return null;
  }
}
