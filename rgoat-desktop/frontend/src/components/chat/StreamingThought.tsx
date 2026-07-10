interface StreamingThoughtProps {
  content: string;
}

export default function StreamingThought({ content }: StreamingThoughtProps) {
  if (!content) return null;

  return (
    <div className="flex justify-start mb-1">
      <div className="max-w-[85%] px-3 py-2 rounded-lg text-sm text-textMuted italic">
        {content}
        <span className="inline-block w-1.5 h-4 bg-primary ml-0.5 align-text-bottom animate-pulse" />
      </div>
    </div>
  );
}
