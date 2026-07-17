import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const tauriInvoke = invoke;
export const tauriListen = listen;

// D1-T05: 完整事件类型清单 — 与 rgoat-core/src/agent/types.rs AgentEvent 对齐
// (serde rename_all = "snake_case")
export type AgentEvent = {
  type:
    | "started"
    | "thought"
    | "tool_call"
    | "tool_result"
    | "approval" // 旧（保留兼容，后端不再 emit）
    | "approval_required" // D1-T05: 审批请求
    | "file_changed" // D1-T05: 文件变更
    | "message"
    | "message_delta"
    | "finished"
    | "error"
    | "step_completed"
    | "usage"
    | "cancelled"
    | "context_compacted"
    | "tool_failed";
  source: string;
  [key: string]: unknown;
};
