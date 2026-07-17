interface StreamingThoughtProps {
  content: string;
}

export default function StreamingThought({ content }: StreamingThoughtProps) {
  if (!content) return null;

  return (
    <div className="flex justify-start mb-4 gap-3">
      <div className="w-7 h-7 rounded-full bg-accent-signal-subtle flex items-center justify-center shrink-0">
        <span className="text-xs font-semibold text-accent-signal">R</span>
      </div>
      <div className="flex-1 max-w-[85%] text-sm text-text-secondary italic">
        {content}
        <span className="inline-block w-1.5 h-4 ml-0.5 align-text-bottom animate-pulse bg-accent-signal" />
      </div>
    </div>
  );
}
