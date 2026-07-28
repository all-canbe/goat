import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriInvokeMock = vi.hoisted(() => vi.fn());

vi.mock("../lib/tauri-bridge", () => ({
  tauriInvoke: tauriInvokeMock,
}));

import {
  useSessionStore,
  mapSessionMessagesToChat,
  isInternalIntervention,
  type SessionMessage,
} from "./sessionStore";
import { useWorkspaceStore } from "./workspaceStore";

const baseState = {
  sessions: [
    {
      id: "session-1",
      title: "已有会话",
      message_count: 1,
      created_at: "2026-07-22T00:00:00Z",
      workspace: "E:/workspace",
    },
  ],
  activeSessionId: null,
  loading: false,
  selectingId: null,
  deletingId: null,
};

describe("sessionStore", () => {
  beforeEach(() => {
    tauriInvokeMock.mockReset();
    useSessionStore.setState(baseState);
    // 重置 workspaceStore 避免跨用例污染
    useWorkspaceStore.setState({ workspace: null, pendingWorkspacePath: null });
    useWorkspaceStore.getState().cancelPendingSwitch();
  });

  it("loads persisted messages before activating a selected session", async () => {
    tauriInvokeMock.mockResolvedValueOnce([]);

    await useSessionStore.getState().selectSession("session-1");

    expect(tauriInvokeMock).toHaveBeenCalledWith("get_session_messages", {
      sessionId: "session-1",
    });
    expect(useSessionStore.getState().activeSessionId).toBe("session-1");
    expect(useSessionStore.getState().selectingId).toBeNull();
  });

  it("selectSession 联动 scheduleWorkspaceSwitch 传入 session.workspace", async () => {
    tauriInvokeMock.mockResolvedValueOnce([]);

    await useSessionStore.getState().selectSession("session-1");

    // scheduleWorkspaceSwitch 应设置 pendingWorkspacePath 为会话 workspace
    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBe("E:/workspace");
  });

  it("throws when get_session_messages fails", async () => {
    tauriInvokeMock.mockRejectedValueOnce(new Error("db locked"));

    await expect(
      useSessionStore.getState().selectSession("session-1")
    ).rejects.toThrow("db locked");

    expect(useSessionStore.getState().activeSessionId).toBeNull();
    expect(useSessionStore.getState().selectingId).toBeNull();
  });

  it("removes session after successful delete", async () => {
    // cancel_agent → delete_session → clear_session_changes
    tauriInvokeMock.mockResolvedValue(undefined);

    await useSessionStore.getState().deleteSession("session-1");

    expect(tauriInvokeMock).toHaveBeenCalledWith("cancel_agent", {
      sessionId: "session-1",
    });
    expect(tauriInvokeMock).toHaveBeenCalledWith("delete_session", {
      sessionId: "session-1",
    });
    expect(useSessionStore.getState().sessions).toHaveLength(0);
    expect(useSessionStore.getState().deletingId).toBeNull();
  });

  it("throws when delete_session fails", async () => {
    tauriInvokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "delete_session") throw new Error("not found");
      return undefined;
    });

    await expect(
      useSessionStore.getState().deleteSession("session-1")
    ).rejects.toThrow("not found");

    expect(useSessionStore.getState().sessions).toHaveLength(1);
    expect(useSessionStore.getState().deletingId).toBeNull();
  });

  it("skips reloading history for a streaming session", async () => {
    const { useChatStore } = await import("./chatStore");
    useChatStore.setState({
      sessions: {
        "session-1": {
          messages: [{ id: "live-1", type: "tool_call", toolName: "Write" }],
          isStreaming: true,
          streamingContent: "partial",
          pendingApproval: null,
          toolCallCount: 1,
          pendingApprovals: 0,
          tokenUsage: { inputTokens: 0, outputTokens: 0, totalCost: 0 },
          fileRefs: [],
          compactionNotices: [],
          planContent: "",
        },
      },
    });

    await useSessionStore.getState().selectSession("session-1");

    expect(tauriInvokeMock).not.toHaveBeenCalledWith("get_session_messages", {
      sessionId: "session-1",
    });
    expect(useSessionStore.getState().activeSessionId).toBe("session-1");
    expect(useChatStore.getState().sessions["session-1"].messages).toEqual([
      { id: "live-1", type: "tool_call", toolName: "Write" },
    ]);
    expect(useSessionStore.getState().selectingId).toBeNull();
  });

  it("maps history into tool cards and filters internal interventions", async () => {
    const { useChatStore } = await import("./chatStore");
    useChatStore.setState({ sessions: {} });

    const toolCalls = JSON.stringify([
      {
        id: "call_1",
        type: "function",
        function: {
          name: "write_file",
          arguments: JSON.stringify({ file_path: "a.ts" }),
        },
      },
    ]);

    tauriInvokeMock.mockResolvedValueOnce([
      { id: 1, role: "user", content: "帮我创建项目", tool_calls: null, tool_call_id: null },
      {
        id: 2,
        role: "user",
        content: "前两步工具调用连续失败。请重新评估当前计划，如果当前路径不可行，请使用 ## 计划 重新规划。",
        tool_calls: null,
        tool_call_id: null,
      },
      {
        id: 3,
        role: "assistant",
        content: "开始写文件",
        tool_calls: toolCalls,
        tool_call_id: null,
      },
      {
        id: 4,
        role: "tool",
        content: "Successfully wrote a.ts",
        tool_calls: null,
        tool_call_id: "call_1",
      },
      {
        id: 5,
        role: "tool",
        content: "检测到重复调用 write_file，请更换策略",
        tool_calls: null,
        tool_call_id: "missing",
      },
    ] satisfies SessionMessage[]);

    await useSessionStore.getState().selectSession("session-1");

    const mapped = useChatStore.getState().sessions["session-1"].messages;
    expect(mapped.map((m) => m.type)).toEqual(["user", "assistant", "tool_call", "tool_call"]);
    expect(mapped[0].content).toBe("帮我创建项目");
    expect(mapped[2]).toMatchObject({
      type: "tool_call",
      toolName: "write_file",
      success: true,
      output: "Successfully wrote a.ts",
      toolCallId: "call_1",
    });
    expect(mapped[3]).toMatchObject({
      type: "tool_call",
      success: true,
      output: "检测到重复调用 write_file，请更换策略",
    });
    expect(mapped.some((m) => m.content?.includes("前两步工具调用连续失败"))).toBe(false);
  });

  it("会话激活时序：workspace 切换完成前不激活会话", async () => {
    vi.useFakeTimers();
    try {
      const infoB = { path: "/ws/b", is_temporary: false };
      tauriInvokeMock.mockImplementation(
        async (cmd: string, _args?: { sessionId?: string; path?: string }) => {
          if (cmd === "get_session_messages") return [];
          if (cmd === "set_workspace") return infoB;
          return undefined;
        }
      );

      useSessionStore.setState({
        sessions: [
          {
            id: "session-a",
            title: "会话 A",
            message_count: 1,
            created_at: "2026-07-22T00:00:00Z",
            workspace: "/ws/a",
          },
          {
            id: "session-b",
            title: "会话 B",
            message_count: 1,
            created_at: "2026-07-22T01:00:00Z",
            workspace: "/ws/b",
          },
        ],
        activeSessionId: "session-a",
      });

      // 选择 B —— 期望行为：在 scheduleWorkspaceSwitch 完成前不应激活 B
      await useSessionStore.getState().selectSession("session-b");

      // 期望：activeSessionId 仍为 "session-a"（因为 workspace 切换尚未完成）
      // 当前代码会在这里失败：selectSession 先设置 activeSessionId 再调用 scheduleWorkspaceSwitch
      expect(useSessionStore.getState().activeSessionId).toBe("session-a");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("mapSessionMessagesToChat", () => {
  it("detects internal intervention prompts", () => {
    expect(isInternalIntervention("检测到重复调用 write_file，请更换策略")).toBe(true);
    expect(isInternalIntervention("正常用户问题")).toBe(false);
  });

  it("rebuilds tool timeline from assistant tool_calls + tool rows", () => {
    const messages: SessionMessage[] = [
      {
        id: 1,
        role: "assistant",
        content: "",
        tool_calls: JSON.stringify([
          {
            id: "tc1",
            function: { name: "shell", arguments: '{"command":"ls"}' },
          },
        ]),
      },
      {
        id: 2,
        role: "tool",
        content: "[error] boom",
        tool_call_id: "tc1",
      },
    ];
    const chat = mapSessionMessagesToChat(messages);
    expect(chat).toHaveLength(1);
    expect(chat[0]).toMatchObject({
      type: "tool_call",
      toolName: "shell",
      success: false,
      output: "boom",
      toolCallId: "tc1",
    });
  });
});
