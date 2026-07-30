import { useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Check, Copy } from "lucide-react";

function CodeBlock({ children, className }: { children?: React.ReactNode; className?: string }) {
  const [copied, setCopied] = useState(false);
  const code = String(children || "").replace(/\n$/, "");

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, [code]);

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
        {copied ? <Check size={14} className="text-success" /> : <Copy size={14} />}
      </button>
    </div>
  );
}

/** 轻度规范化：保证标题前有换行，避免流式粘连导致 GFM 不识别 */
export function normalizeMarkdown(src: string): string {
  if (!src) return src;
  return src.replace(/([^\n])(#{1,6}\s)/g, "$1\n\n$2");
}

interface MarkdownBodyProps {
  content: string;
  className?: string;
  /** 流式光标 */
  showCursor?: boolean;
}

export default function MarkdownBody({ content, className = "", showCursor = false }: MarkdownBodyProps) {
  const text = normalizeMarkdown(content || "");

  return (
    <div className={`prose prose-invert prose-sm max-w-none ${className}`}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          code({ className: codeClass, children }) {
            const hasLanguage = /language-(\w+)/.exec(codeClass || "");
            if (hasLanguage) {
              return <CodeBlock className={codeClass}>{children}</CodeBlock>;
            }
            const hasNewline = typeof children === "string" && children.includes("\n");
            if (hasNewline && codeClass) {
              return <CodeBlock className={codeClass}>{children}</CodeBlock>;
            }
            return (
              <code className="bg-surface-hover px-1.5 py-0.5 rounded text-xs font-mono text-text-secondary">
                {children}
              </code>
            );
          },
        }}
      >
        {text}
      </ReactMarkdown>
      {showCursor && (
        <span className="inline-block w-1.5 h-4 ml-0.5 align-text-bottom animate-pulse bg-accent-signal" />
      )}
    </div>
  );
}
