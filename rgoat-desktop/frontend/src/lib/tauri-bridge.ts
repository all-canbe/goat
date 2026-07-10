import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const tauriInvoke = invoke;
export const tauriListen = listen;

export type AgentEvent = {
  type:
    | "started"
    | "thought"
    | "tool_call"
    | "tool_result"
    | "approval"
    | "message"
    | "finished"
    | "error"
    | "step_completed";
  source: string;
  [key: string]: unknown;
};
