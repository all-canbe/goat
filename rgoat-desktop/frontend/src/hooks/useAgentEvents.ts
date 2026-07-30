import { useEffect, useRef } from "react";
import { tauriListen, tauriInvoke, type AgentEvent } from "../lib/tauri-bridge";
import { useChatStore, useActiveSessionState } from "../stores/chatStore";
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
  // P1 修复：React StrictMode 会双重调用 useEffect；用 ref 保证同一时刻只有一个 listener
  const unlistenRef = useRef<(() => void) | null>(null);

  const {
    addSystemMessage,
    addErrorMessage,
    addAssistantMessage,
    appendThought,
    addToolCall,
    updateToolResult,
    commitStream,
    setStreaming,
    setApproval,
    addUsage,
    addCompactionNotice,
  } = useChatStore();

  // isStreaming 来自当前激活会话（多会话隔离）
  const { isStreaming } = useActiveSessionState();

  useEffect(() => {
    async function setup() {
      // 如果已有 listener，先清理（StrictMode 下避免重复注册）
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
      try {
        unlistenRef.current = await tauriListen<AgentEvent>("agent-event", (event) => {
          const payload = event.payload;
          // 多会话：后端在 emit() 注入 session_id，优先用它路由；缺失时 fallback 激活会话
          const sid = (payload.session_id as string) || undefined;
          switch (payload.type) {
            case "started": {
              const mode = (payload.mode as string) || "agent";
              setStreaming(true, sid);
              addSystemMessage(`Agent started in ${mode} mode`, sid);
              break;
            }

            case "thought": {
              const content = (payload.content as string) || "";
              // 流式路径已通过 message_delta 累积正文；Thought 再 append 会导致全文重复、MD 糊成一团。
              // 仅在尚无流式内容时作为非流式 fallback 写入。
              const sessId = sid ?? useSessionStore.getState().activeSessionId;
              const current =
                (sessId && useChatStore.getState().sessions[sessId]?.streamingContent) || "";
              if (content && !current) {
                appendThought(content, sid);
              } else if (
                content &&
                current &&
                !current.includes(content) &&
                !content.includes(current)
              ) {
                // 短状态提示（非全文重复）才追加
                if (content.length < 200) {
                  appendThought(`\n${content}`, sid);
                }
              }
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
              // 时间线嵌入：工具卡出现前先落盘当前流式文字段
              commitStream(sid);
              const toolName = (payload.tool_name as string) || "unknown";
              const args = (payload.arguments as Record<string, unknown>) || {};
              addToolCall(toolName, args, sid);
              // 保持 isStreaming，后续文字继续流式
              setStreaming(true, sid);
              break;
            }

            case "tool_result": {
              const toolName = (payload.tool_name as string) || "unknown";
              const success = !!payload.success;
              const output = (payload.output as string) || "";
              updateToolResult(toolName, success, output, sid);
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
              setApproval(
                {
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
                },
                sid,
              );
              break;
            }

            // D1-T05: 文件变更 → 写入后端 session_changes + 同步 changesStore
            // 多会话：用 payload.session_id 路由到事件所属会话
            case "file_changed": {
              const sessionId = sid ?? useSessionStore.getState().activeSessionId;
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
                addAssistantMessage(content, sid);
              }
              setStreaming(false, sid);
              break;
            }

            case "finished": {
              const answer = (payload.answer as string) || "";
              const committed = commitStream(sid);
              if (!committed && answer && answer !== "Done") {
                addAssistantMessage(answer, sid);
              }
              setStreaming(false, sid);
              addSystemMessage("Agent finished", sid);
              break;
            }

            case "error": {
              const message = (payload.message as string) || "Unknown error";
              commitStream(sid);
              addErrorMessage(message, sid);
              setStreaming(false, sid);
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
                addUsage(inputTokens, outputTokens, sid);
              }
              break;
            }

            case "cancelled": {
              const partial = (payload.partial_answer as string) || "";
              const committed = commitStream(sid);
              if (!committed && partial) {
                addAssistantMessage(partial, sid);
              }
              setStreaming(false, sid);
              addSystemMessage("Agent cancelled", sid);
              break;
            }

            case "context_compacted": {
              const before = (payload.messages_before as number) || 0;
              const after = (payload.messages_after as number) || 0;
              // P1: 改用更醒目的压缩通知条（MessageList 顶部展示）
              addCompactionNotice(before, after, sid);
              break;
            }

            case "tool_failed": {
              const toolName = (payload.tool_name as string) || "unknown";
              const error = (payload.error as string) || "Tool failed";
              updateToolResult(toolName, false, error, sid);
              break;
            }

            case "message_delta": {
              const delta = (payload.delta as string) || "";
              if (delta) appendThought(delta, sid);
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
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    };
  }, [
    addSystemMessage,
    addErrorMessage,
    addAssistantMessage,
    appendThought,
    addToolCall,
    updateToolResult,
    commitStream,
    setStreaming,
    setApproval,
    addUsage,
    addCompactionNotice,
  ]);

  return { isStreaming };
}
