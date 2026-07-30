import MarkdownBody from "./MarkdownBody";

interface StreamingThoughtProps {
  content: string;
  waiting?: boolean;
}

export default function StreamingThought({ content, waiting = false }: StreamingThoughtProps) {
  if (!content && !waiting) return null;

  return (
    <div className="flex justify-start mb-4 gap-3">
      <div className="w-7 h-7 rounded-full bg-accent-signal-subtle flex items-center justify-center shrink-0">
        <span className="text-xs font-semibold text-accent-signal">G</span>
      </div>
      <div className="flex-1 max-w-[85%] text-sm text-text">
        {content ? (
          <MarkdownBody content={content} showCursor />
        ) : (
          <span className="text-text-secondary italic">
            正在生成回复…
            <span className="inline-block w-1.5 h-4 ml-0.5 align-text-bottom animate-pulse bg-accent-signal" />
          </span>
        )}
      </div>
    </div>
  );
}
