import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useAgentEvents } from "./useAgentEvents";

const listener = vi.hoisted(() => ({ callback: undefined as ((event: { payload: Record<string, unknown> }) => void) | undefined }));
const chat = vi.hoisted(() => ({
  messages: [{ id: "user-1", type: "user", content: "你可以使用哪些 skill" }],
  streamingContent: "",
  isStreaming: false,
  sessions: {} as Record<string, { streamingContent: string }>,
  addSystemMessage: vi.fn(),
  addErrorMessage: vi.fn(),
  addAssistantMessage: vi.fn(),
  appendThought: vi.fn((text: string, sid?: string) => {
    chat.streamingContent += text;
    if (sid) {
      const prev = chat.sessions[sid]?.streamingContent || "";
      chat.sessions[sid] = { streamingContent: prev + text };
    }
  }),
  addToolCall: vi.fn(),
  updateToolResult: vi.fn(),
  commitStream: vi.fn(),
  setStreaming: vi.fn((value: boolean) => { chat.isStreaming = value; }),
  clearMessages: vi.fn(),
  setApproval: vi.fn(),
  addUsage: vi.fn(),
  addCompactionNotice: vi.fn(),
}));

vi.mock("../lib/tauri-bridge", () => ({
  tauriListen: vi.fn(async (_name: string, callback: typeof listener.callback) => {
    listener.callback = callback;
    return vi.fn();
  }),
  tauriInvoke: vi.fn(),
}));
vi.mock("../stores/chatStore", () => ({
  useChatStore: Object.assign(() => chat, { getState: () => chat }),
  useActiveSessionState: () => ({ isStreaming: chat.isStreaming }),
}));
vi.mock("../stores/sessionStore", () => ({ useSessionStore: { getState: () => ({ activeSessionId: "" }) } }));
vi.mock("../stores/changesStore", () => ({ useChangesStore: { getState: () => ({ addChange: vi.fn() }) } }));
vi.mock("../stores/toastStore", () => ({ useToastStore: { getState: () => ({ addToast: vi.fn() }) } }));

describe("useAgentEvents", () => {
  beforeEach(() => {
    listener.callback = undefined;
    chat.messages = [{ id: "user-1", type: "user", content: "你可以使用哪些 skill" }];
    chat.streamingContent = "";
    chat.isStreaming = false;
    Object.values(chat).forEach((value) => { if (typeof value === "function" && "mockReset" in value) value.mockReset(); });
  });

  it("keeps existing messages when an agent starts", async () => {
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());
    chat.clearMessages.mockClear();

    listener.callback?.({ payload: { type: "started", mode: "agent", session_id: "s1" } });

    expect(chat.messages).toHaveLength(1);
    expect(chat.messages[0].content).toBe("你可以使用哪些 skill");
    expect(chat.clearMessages).not.toHaveBeenCalled();
    expect(chat.setStreaming).toHaveBeenCalledWith(true, "s1");
    expect(chat.addSystemMessage).toHaveBeenCalledWith("Agent started in agent mode", "s1");
  });

  it("appends message_delta content to the streaming response", async () => {
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());

    listener.callback?.({ payload: { type: "message_delta", delta: "你好", session_id: "s1" } });

    expect(chat.appendThought).toHaveBeenCalledWith("你好", "s1");
  });

  it("does not re-append full thought when stream already has content", async () => {
    chat.streamingContent = "## 计划\n1. 初始化";
    chat.sessions = { s1: { streamingContent: "## 计划\n1. 初始化" } };
    // sessions may not exist on mock — set via appendThought path using getState
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());
    chat.appendThought.mockClear();

    listener.callback?.({
      payload: {
        type: "thought",
        content: "## 计划\n1. 初始化",
        session_id: "s1",
      },
    });

    // 全文重复时不应再 append
    expect(chat.appendThought).not.toHaveBeenCalled();
  });

  it("commits stream before tool_call for timeline interleaving", async () => {
    chat.streamingContent = "先说明计划";
    chat.commitStream.mockReturnValue(true);
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());

    listener.callback?.({
      payload: {
        type: "tool_call",
        tool_name: "shell",
        arguments: { command: "ls" },
        session_id: "s1",
      },
    });

    expect(chat.commitStream).toHaveBeenCalledWith("s1");
    expect(chat.addToolCall).toHaveBeenCalledWith("shell", { command: "ls" }, "s1");
    expect(chat.setStreaming).toHaveBeenCalledWith(true, "s1");
  });

  it("commits streamed content without adding the finished answer again", async () => {
    chat.streamingContent = "完整回答";
    chat.commitStream.mockReturnValue(true);
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());

    listener.callback?.({ payload: { type: "finished", answer: "完整回答\n", session_id: "s1" } });

    expect(chat.commitStream).toHaveBeenCalledWith("s1");
    expect(chat.addAssistantMessage).not.toHaveBeenCalled();
    expect(chat.setStreaming).toHaveBeenCalledWith(false, "s1");
  });

  it("adds a finished answer when no streamed content exists", async () => {
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());

    listener.callback?.({ payload: { type: "finished", answer: "非流式回答", session_id: "s1" } });

    expect(chat.commitStream).toHaveBeenCalledWith("s1");
    expect(chat.addAssistantMessage).toHaveBeenCalledWith("非流式回答", "s1");
  });

  it("commits streamed content without adding the cancelled partial answer again", async () => {
    chat.streamingContent = "已生成部分";
    chat.commitStream.mockReturnValue(true);
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());

    listener.callback?.({ payload: { type: "cancelled", partial_answer: "已生成部分", session_id: "s1" } });

    expect(chat.commitStream).toHaveBeenCalledWith("s1");
    expect(chat.addAssistantMessage).not.toHaveBeenCalled();
    expect(chat.addSystemMessage).toHaveBeenCalledWith("Agent cancelled", "s1");
  });
});

describe("performance baseline", () => {
  beforeEach(() => {
    // 恢复 mock 实现：前面 describe 的 mockReset() 可能清除了 appendThought 的实现
    chat.appendThought.mockImplementation((text: string, sid?: string) => {
      chat.streamingContent += text;
      if (sid) {
        const prev = chat.sessions[sid]?.streamingContent || "";
        chat.sessions[sid] = { streamingContent: prev + text };
      }
    });
    chat.setStreaming.mockImplementation((value: boolean) => { chat.isStreaming = value; });
    chat.commitStream.mockReturnValue(undefined);
    // 重置状态
    chat.streamingContent = "";
    chat.isStreaming = false;
    chat.sessions = {};
  });

  it("event storm baseline: 1000 message_deltas produce 1000 appendThought calls", async () => {
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());

    // 构造 1000 条 delta，混合中文、换行和 Markdown 符号
    const deltas: string[] = [];
    for (let i = 0; i < 1000; i++) {
      if (i % 10 === 0) {
        deltas.push(`**第${i + 1}段**\n`);
      } else if (i % 7 === 0) {
        deltas.push("`代码` ");
      } else {
        deltas.push(`这是第${i + 1}条中文内容，包含一些测试符号。`);
      }
    }

    // 清理已注册的 mock 调用计数
    chat.appendThought.mockClear();

    // 注入所有 delta
    for (const delta of deltas) {
      // 使用 s1 以保证所有内容被拼接到同个 session
      listener.callback!({ payload: { type: "message_delta", delta, session_id: "s1" } });
    }

    // 断言：当前基线中 appendThought 被调用 1000 次
    expect(chat.appendThought).toHaveBeenCalledTimes(1000);

    // 断言：最终内容等于所有 delta 拼接
    const expected = deltas.join("");
    expect(chat.streamingContent).toBe(expected);
  });

  it("long content integrity: 10000+ chars with markdown table and code blocks", async () => {
    renderHook(() => useAgentEvents());
    await waitFor(() => expect(listener.callback).toBeDefined());

    // 重置
    chat.streamingContent = "";
    chat.appendThought.mockClear();

    // 构造 10,000+ 字符内容
    // 1. 长中文文本
    let longText = "";
    for (let i = 0; i < 100; i++) {
      longText += "乌龙茶是一种半发酵茶，产于中国福建、广东和台湾等地。其制作工艺包括萎凋、做青、杀青、揉捻和干燥等步骤。";
    }
    // 2. GFM 表格（200 行 × 8 列）
    let table = "| 品种 | 产地 | 发酵程度 | 汤色 | 香气 | 滋味 | 叶底 | 备注 |\n|------|------|---------|-----|------|-----|------|------|\n";
    for (let i = 0; i < 200; i++) {
      table += `| 品种${i + 1} | 福建 | 30% | 金黄 | 花香 | 甘醇 | 红边 | 名茶 |\n`;
    }
    // 3. 代码块
    const codeBlock = "```python\ndef brew_tea(temperature, time):\n    \"\"\"冲泡乌龙茶\"\"\"\n    if 95 <= temperature <= 100:\n        return \"冲泡完成\"\n    return \"温度不合适\"\n```\n";
    const allContent = longText + "\n\n" + table + "\n\n" + codeBlock;

    // 切成 1000+ 个 delta
    const deltas: string[] = [];
    const chunkSize = Math.ceil(allContent.length / 1000);
    for (let i = 0; i < allContent.length; i += chunkSize) {
      deltas.push(allContent.slice(i, i + chunkSize));
    }

    // 注入所有 delta
    for (const delta of deltas) {
      listener.callback!({ payload: { type: "message_delta", delta, session_id: "s1" } });
    }

    // 注入 finished 事件
    listener.callback!({ payload: { type: "finished", answer: allContent, session_id: "s1" } });

    // 断言：appendThought 调用次数 = delta 数量
    expect(chat.appendThought).toHaveBeenCalledTimes(deltas.length);

    // 断言：最终内容完整
    expect(chat.streamingContent).toBe(allContent);

    // 断言：finished 后 setStreaming(false) 被调用
    expect(chat.setStreaming).toHaveBeenCalledWith(false, "s1");

    // 断言：appendThought 调用次数（基线值，用于后续对比 Task 4 后）
    // 此值在 Task 4 实现后应明显降低
    console.log(`[performance baseline] long content deltas: ${deltas.length}, appendThought calls: ${chat.appendThought.mock.calls.length}`);
  });
});
