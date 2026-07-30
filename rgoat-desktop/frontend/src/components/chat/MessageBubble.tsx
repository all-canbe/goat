import { AlertCircle } from "lucide-react";
import type { ChatMessage } from "../../stores/chatStore";
import ToolCallCard from "./ToolCallCard";
import MarkdownBody from "./MarkdownBody";

interface MessageBubbleProps {
  message: ChatMessage;
  /** 连续 agent 段仅首条显示头像；默认 true 兼容旧调用 */
  showAvatar?: boolean;
}

export default function MessageBubble({ message, showAvatar = true }: MessageBubbleProps) {
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
          {/* 保留头像占位宽度，避免同段后续气泡左右跳动 */}
          <div className="w-7 h-7 shrink-0 flex items-center justify-center">
            {showAvatar ? (
              <div className="w-7 h-7 rounded-full bg-primary-subtle flex items-center justify-center">
                <span className="text-xs font-semibold text-brand">G</span>
              </div>
            ) : null}
          </div>
          <div className="max-w-[85%] text-sm text-text">
            <MarkdownBody content={message.content || ""} />
          </div>
        </div>
      );

    case "tool_call":
    case "tool_result":
      return (
        <div className="flex justify-start mb-1 gap-3">
          {/* 与 assistant 左缘对齐（头像列占位） */}
          <div className="w-7 shrink-0" />
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
