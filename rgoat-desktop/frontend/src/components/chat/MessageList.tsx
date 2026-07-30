import { useEffect, useRef } from "react";
import { Sparkles, Code2, Bug, ListChecks, Slash, Layers, X } from "lucide-react";
import { useChatStore, useActiveSessionState, type ChatMessage } from "../../stores/chatStore";
import MessageBubble from "./MessageBubble";
import StreamingThought from "./StreamingThought";

// P1: Empty State 示例提问
const EXAMPLE_PROMPTS = [
  { icon: Sparkles, text: "解释这个项目的架构" },
  { icon: Code2, text: "帮我审查代码" },
  { icon: Bug, text: "修复这个 bug" },
  { icon: ListChecks, text: "列出待办事项" },
];

/** 连续 agent 段内仅第一条 assistant 显示头像（中间的 tool 卡不打断段） */
export function shouldShowAvatar(messages: ChatMessage[], index: number): boolean {
  const msg = messages[index];
  if (!msg || msg.type !== "assistant") return false;
  for (let i = index - 1; i >= 0; i--) {
    const t = messages[i].type;
    if (t === "assistant") return false;
    if (t === "tool_call" || t === "tool_result") continue;
    return true;
  }
  return true;
}

export default function MessageList() {
  const { messages, streamingContent, isStreaming, compactionNotices } = useActiveSessionState();
  const { setDraft, dismissCompactionNotice } = useChatStore();
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingContent]);

  // P1: 取最新一条压缩通知展示
  const latestNotice = compactionNotices[compactionNotices.length - 1];

  return (
    <div
      data-testid="message-list-scroll"
      className="flex-1 overflow-y-auto px-4 py-3 pb-56"
    >
      <div
        data-testid="chat-content-column"
        className="w-full max-w-[840px] mx-auto flex min-h-full flex-col gap-1"
      >
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
                  className="flex items-center gap-2 px-3 py-2.5 rounded-md bg-surface border border-border text-left text-xs text-text hover:border-primary/50 hover:bg-surface-hover hover:ring-2 hover:ring-ring focus-visible:ring-2 focus-visible:ring-ring transition"
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

      {/* 时间线：文字与工具卡按 messages 顺序交错渲染；连续 agent 段仅首条头像 */}
      {messages.map((msg, index) => (
        <MessageBubble
          key={msg.id}
          message={msg}
          showAvatar={shouldShowAvatar(messages, index)}
        />
      ))}

      {(streamingContent || isStreaming) && (
        <StreamingThought content={streamingContent} waiting={isStreaming && !streamingContent} />
      )}

      <div ref={bottomRef} />
      </div>
    </div>
  );
}
