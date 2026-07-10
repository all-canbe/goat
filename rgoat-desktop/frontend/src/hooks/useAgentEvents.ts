import { useEffect } from "react";
import { tauriListen, AgentEvent } from "../lib/tauri-bridge";
import { useChatStore } from "../stores/chatStore";

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

            case "approval": {
              const toolName = (payload.tool_name as string) || "unknown";
              const decision = (payload.decision as string) || "Ask";
              const message = (payload.message as string) || "";
              const args =
                (payload.arguments as Record<string, unknown>) || {};
              setApproval({ tool_name: toolName, decision, message, arguments: args });
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
              break;
            }

            case "step_completed": {
              // Optional: could update a progress indicator
              break;
            }
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
