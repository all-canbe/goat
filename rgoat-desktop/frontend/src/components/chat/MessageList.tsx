import { useEffect, useRef } from "react";
import { Sparkles, Code2, Bug, ListChecks, Slash, Layers, X } from "lucide-react";
import { useChatStore } from "../../stores/chatStore";
import MessageBubble from "./MessageBubble";
import StreamingThought from "./StreamingThought";

// P1: Empty State 示例提问
const EXAMPLE_PROMPTS = [
  { icon: Sparkles, text: "解释这个项目的架构" },
  { icon: Code2, text: "帮我审查代码" },
  { icon: Bug, text: "修复这个 bug" },
  { icon: ListChecks, text: "列出待办事项" },
];

const AGENT_MESSAGE_TYPES = new Set(["assistant", "tool_call", "tool_result"]);

function isAgentMessage(type: string): boolean {
  return AGENT_MESSAGE_TYPES.has(type);
}

export default function MessageList() {
  const { messages, streamingContent, isStreaming, setDraft, compactionNotices, dismissCompactionNotice } = useChatStore();
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingContent]);

  // P1: 取最新一条压缩通知展示
  const latestNotice = compactionNotices[compactionNotices.length - 1];

  return (
    <div className="flex-1 overflow-y-auto px-4 py-3 pb-32 flex flex-col gap-1">
      {/* P1: 上下文压缩通知条（醒目展示，可关闭） */}
      {latestNotice && (
        <div className="flex items-center gap-2 px-3 py-2 mb-2 rounded-md bg-primary-subtle border border-primary/30 text-brand text-xs">
          <Layers size={14} className="shrink-0" />
          <span className="flex-1">
            上下文已压缩：{latestNotice.before} → {latestNotice.after} 条消息
          </span>
          <button
            onClick={() => dismissCompactionNotice(latestNotice.id)}
            className="p-0.5 rounded hover:bg-primary/20 transition-colors"
            title="关闭"
          >
            <X size={12} />
          </button>
        </div>
      )}

      {messages.length === 0 && !isStreaming && (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center max-w-md w-full">
            <div className="w-12 h-12 rounded-lg bg-primary-subtle flex items-center justify-center mx-auto mb-4">
              <Sparkles size={24} className="text-brand" />
            </div>
            <h2 className="text-lg font-semibold text-text mb-1">开始对话</h2>
            <p className="text-xs text-text-secondary mb-5">输入问题或使用 Slash 命令</p>
            <div className="grid grid-cols-2 gap-2 mb-5">
              {EXAMPLE_PROMPTS.map(({ icon: Icon, text }) => (
                <button
                  key={text}
                  onClick={() => setDraft(text)}
                  className="flex items-center gap-2 px-3 py-2.5 rounded-md bg-surface border border-border text-left text-xs text-text hover:border-primary/50 hover:bg-surface-hover hover:ring-2 hover:ring-ring focus-visible:ring-2 focus-visible:ring-ring transition-colors"
                >
                  <Icon size={14} className="text-brand shrink-0" />
                  <span className="truncate">{text}</span>
                </button>
              ))}
            </div>
            <div className="flex items-center justify-center gap-1.5 text-[11px] text-text-secondary">
              <Slash size={12} />
              <span>输入 / 查看 Slash 命令</span>
            </div>
          </div>
        </div>
      )}

      {messages.map((msg, index) => {
        const prevMsg = messages[index - 1];
        const showDivider = prevMsg && isAgentMessage(prevMsg.type) && isAgentMessage(msg.type);
        return (
          <div key={msg.id} className={showDivider ? "border-t border-divider" : undefined}>
            <MessageBubble message={msg} />
          </div>
        );
      })}

      {streamingContent && <StreamingThought content={streamingContent} />}

      <div ref={bottomRef} />
    </div>
  );
}
