import { useEffect, useRef } from "react";
import { useChatStore } from "../../stores/chatStore";
import MessageBubble from "./MessageBubble";
import StreamingThought from "./StreamingThought";

export default function MessageList() {
  const { messages, streamingContent, isStreaming } = useChatStore();
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingContent]);

  return (
    <div className="flex-1 overflow-y-auto px-4 py-3 flex flex-col gap-1">
      {messages.length === 0 && !isStreaming && (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center text-textMuted">
            <p className="text-lg font-semibold mb-1">RGoat Desktop</p>
            <p className="text-xs">Ask anything to get started</p>
          </div>
        </div>
      )}

      {messages.map((msg) => (
        <MessageBubble key={msg.id} message={msg} />
      ))}

      {streamingContent && <StreamingThought content={streamingContent} />}

      <div ref={bottomRef} />
    </div>
  );
}
