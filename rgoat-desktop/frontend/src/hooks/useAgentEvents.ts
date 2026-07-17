import { useEffect } from "react";
import { tauriListen, tauriInvoke, type AgentEvent } from "../lib/tauri-bridge";
import { useChatStore } from "../stores/chatStore";
import { useSessionStore } from "../stores/sessionStore";
import { useChangesStore, type FileChangeRecord } from "../stores/changesStore";
import { useToastStore } from "../stores/toastStore";

// D1-T05: 从 unified diff 文本估算 +/- 行数（用于前端展示，权威值在后端）
function estimateDiffStats(diff: string): { additions: number; deletions: number } {
  if (!diff) return { additions: 0, deletions: 0 };
  let additions = 0;
  let deletions = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+") && !line.startsWith("+++")) additions++;
    else if (line.startsWith("-") && !line.startsWith("---")) deletions++;
  }
  return { additions, deletions };
}

export function useAgentEvents() {
  const {
    addSystemMessage,
    addErrorMessage,
    addAssistantMessage,
    appendThought,
    addToolCall,
    updateToolResult,
    commitStream,
    setStreaming,
    clearMessages,
    setApproval,
    addUsage,
    addCompactionNotice,
  } = useChatStore();

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    async function setup() {
      try {
        unlisten = await tauriListen<AgentEvent>("agent-event", (event) => {
          const payload = event.payload;
          switch (payload.type) {
            case "started": {
              const mode = (payload.mode as string) || "agent";
              clearMessages();
              setStreaming(true);
              addSystemMessage(`Agent started in ${mode} mode`);
              break;
            }

            case "thought": {
              const content = (payload.content as string) || "";
              appendThought(content);
              // P1: Plan Mode 计划完成时弹出 toast 提示（PlanRunner 通过 emit_system 发出）
              if (content.includes("计划已保存")) {
                useToastStore.getState().addToast(
                  "计划已生成，已保存到 .goat/doc/",
                  "success"
                );
              }
              break;
            }

            case "tool_call": {
              const toolName = (payload.tool_name as string) || "unknown";
              const args = (payload.arguments as Record<string, unknown>) || {};
              addToolCall(toolName, args);
              break;
            }

            case "tool_result": {
              const toolName = (payload.tool_name as string) || "unknown";
              const success = !!payload.success;
              const output = (payload.output as string) || "";
              updateToolResult(toolName, success, output);
              break;
            }

            // D1-T05: 修复事件名（旧 "approval" → "approval_required"）
            case "approval_required": {
              const toolName = (payload.tool_name as string) || "unknown";
              const toolType = (payload.tool_type as string) || "other";
              const summary = (payload.summary as string) || "";
              const riskLevel = (payload.risk_level as string) || "LOW";
              const dangerScore = (payload.danger_score as number) || 0;
              const command = payload.command as string | undefined;
              const path = payload.path as string | undefined;
              const url = payload.url as string | undefined;
              const diff = payload.diff as string | undefined;
              const affectedFiles = (payload.affected_files as string[]) || [];
              const allowOptions = (payload.allow_options as string[]) || ["once"];
              const args = (payload.arguments as Record<string, unknown>) || {};
              setApproval({
                tool_name: toolName,
                tool_type: toolType,
                summary,
                risk_level: riskLevel,
                danger_score: dangerScore,
                command,
                path,
                url,
                diff,
                affected_files: affectedFiles,
                allow_options: allowOptions,
                arguments: args,
              });
              break;
            }

            // D1-T05: 文件变更 → 写入后端 session_changes + 同步 changesStore
            case "file_changed": {
              const sessionId = useSessionStore.getState().activeSessionId;
              if (!sessionId) break;
              const diffText = (payload.diff as string) || "";
              const { additions, deletions } = estimateDiffStats(diffText);
              const change: FileChangeRecord = {
                file_path: (payload.file_path as string) || "",
                change_type: (payload.change_type as string) || "edit",
                diff: diffText,
                tool_name: (payload.tool_name as string) || "",
                timestamp: new Date().toISOString(),
                additions,
                deletions,
              };
              // 先更新前端 store（即时反馈），再异步同步到后端
              useChangesStore.getState().addChange(sessionId, change);
              tauriInvoke("add_session_change", { sessionId, change }).catch(() => {});
              break;
            }

            case "message": {
              const content = (payload.content as string) || "";
              if (content) {
                addAssistantMessage(content);
              }
              setStreaming(false);
              break;
            }

            case "finished": {
              const answer = (payload.answer as string) || "";
              const { streamingContent } = useChatStore.getState();
              commitStream();
              if (answer && answer !== "Done" && answer !== streamingContent) {
                addAssistantMessage(answer);
              }
              setStreaming(false);
              addSystemMessage("Agent finished");
              break;
            }

            case "error": {
              const message = (payload.message as string) || "Unknown error";
              commitStream();
              addErrorMessage(message);
              setStreaming(false);
              // P0-4: 同时弹出 Toast 即时反馈
              useToastStore.getState().addToast(message, "error");
              break;
            }

            case "step_completed": {
              // 可选：更新进度指示器
              break;
            }

            // P0-2: Token 用量累积到 store（StatusBar 展示），不再刷屏系统消息
            case "usage": {
              const inputTokens = (payload.input_tokens as number) || 0;
              const outputTokens = (payload.output_tokens as number) || 0;
              if (inputTokens || outputTokens) {
                addUsage(inputTokens, outputTokens);
              }
              break;
            }

            case "cancelled": {
              const partial = (payload.partial_answer as string) || "";
              commitStream();
              if (partial) {
                addAssistantMessage(partial);
              }
              setStreaming(false);
              addSystemMessage("Agent cancelled");
              break;
            }

            case "context_compacted": {
              const before = (payload.messages_before as number) || 0;
              const after = (payload.messages_after as number) || 0;
              // P1: 改用更醒目的压缩通知条（MessageList 顶部展示）
              addCompactionNotice(before, after);
              break;
            }

            case "tool_failed": {
              const toolName = (payload.tool_name as string) || "unknown";
              const error = (payload.error as string) || "Tool failed";
              updateToolResult(toolName, false, error);
              break;
            }

            case "message_delta": {
              // 流式增量暂不处理（保留 hook 兼容）
              break;
            }

            default:
              // 未知事件类型静默忽略
              break;
          }
        });
      } catch {
        // Tauri listener not available (e.g., running in browser)
      }
    }

    setup();

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const isStreaming = useChatStore((s) => s.isStreaming);

  return { isStreaming };
}
