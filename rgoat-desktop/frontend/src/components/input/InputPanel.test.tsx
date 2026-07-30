import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import InputPanel from "./InputPanel";

const mockConfigStore = vi.hoisted(() => ({
  providers: [] as {
    name: string;
    model: string;
    provider_type: string;
    is_current: boolean;
    enabled: boolean;
    source: "settings" | "env" | "fallback";
  }[],
  currentProvider: "",
  switchProvider: vi.fn(),
  loadProviders: vi.fn(),
}));

const mockSessionStore = vi.hoisted(() => ({
  activeSessionId: "",
  setActiveSessionId: vi.fn(),
  renameSession: vi.fn().mockResolvedValue(undefined),
  sessions: [] as { id: string; title: string }[],
}));

const mockChatStore = vi.hoisted(() => ({
  isStreaming: false,
  setStreaming: vi.fn(),
  addUserMessage: vi.fn(),
  addSystemMessage: vi.fn(),
  clearMessages: vi.fn(),
  cancelAgent: vi.fn(),
  draft: "",
  setDraft: vi.fn(),
  focusInputTrigger: 0,
  fileRefs: [] as { id: string; name: string; path: string }[],
  addFileRef: vi.fn(),
  removeFileRef: vi.fn(),
  clearFileRefs: vi.fn(),
  ensureSession: vi.fn(),
}));

const mockSkillStore = vi.hoisted(() => ({
  skills: [] as {
    name: string;
    description: string;
    source: string;
  }[],
  loadSkills: vi.fn(),
  readSkill: vi.fn(),
}));

const mockToastStore = vi.hoisted(() => ({
  addToast: vi.fn(),
}));

const tauriInvokeMock = vi.hoisted(() => vi.fn());

vi.mock("../../stores/configStore", () => ({
  useConfigStore: () => mockConfigStore,
}));

vi.mock("../../stores/sessionStore", () => ({
  useSessionStore: () => mockSessionStore,
}));

vi.mock("../../stores/chatStore", () => {
  const useChatStore = Object.assign(() => mockChatStore, {
    getState: () => mockChatStore,
  });
  return {
    useChatStore,
    useActiveSessionState: () => ({
      isStreaming: mockChatStore.isStreaming,
      fileRefs: mockChatStore.fileRefs,
    }),
  };
});

vi.mock("../../stores/skillStore", () => ({
  useSkillStore: () => mockSkillStore,
}));

vi.mock("../../stores/toastStore", () => ({
  useToastStore: () => mockToastStore,
}));

vi.mock("../../lib/tauri-bridge", () => ({
  tauriInvoke: tauriInvokeMock,
}));

function setCurrentProvider(name: string, model: string) {
  mockConfigStore.currentProvider = name;
  mockConfigStore.providers = [
    {
      name,
      model,
      provider_type: "openai_compatible",
      is_current: true,
      enabled: true,
      source: "settings",
    },
  ];
}

describe("InputPanel", () => {
  beforeEach(() => {
    tauriInvokeMock.mockReset();
    mockConfigStore.providers = [];
    mockConfigStore.currentProvider = "";
    mockConfigStore.switchProvider.mockReset();
    mockConfigStore.loadProviders.mockReset();
    mockSessionStore.activeSessionId = "";
    mockSessionStore.setActiveSessionId.mockReset();
    mockChatStore.isStreaming = false;
    mockChatStore.setStreaming.mockReset();
    mockChatStore.addUserMessage.mockReset();
    mockChatStore.addSystemMessage.mockReset();
    mockChatStore.clearMessages.mockReset();
    mockChatStore.cancelAgent.mockReset();
    mockChatStore.draft = "";
    mockChatStore.setDraft.mockReset();
    mockChatStore.fileRefs = [];
    mockChatStore.addFileRef.mockReset();
    mockChatStore.removeFileRef.mockReset();
    mockChatStore.clearFileRefs.mockReset();
    mockChatStore.ensureSession.mockReset();
    mockSkillStore.skills = [];
    mockSkillStore.loadSkills.mockReset();
    mockSkillStore.readSkill.mockReset();
    mockToastStore.addToast.mockReset();
    tauriInvokeMock.mockResolvedValue({ session_id: "s-1", message: "ok" });
  });

  it("does not show the global focus-visible outline on the chat textarea", () => {
    render(<InputPanel />);

    expect(screen.getByRole("textbox")).toHaveClass("focus-visible:outline-none");
  });

  it("sends the default request shape without thinking_level", async () => {
    setCurrentProvider("Alpha", "a-model");
    render(<InputPanel />);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "hello" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => expect(tauriInvokeMock).toHaveBeenCalled());
    expect(tauriInvokeMock).toHaveBeenCalledWith("send_prompt", {
      request: expect.objectContaining({ prompt: "hello", mode: "agent" }),
    });
    const request = tauriInvokeMock.mock.calls[0][1].request as Record<string, unknown>;
    expect(request).not.toHaveProperty("thinking_level");
  });

  it("sends thinking_level when a non-default level is selected", async () => {
    setCurrentProvider("Alpha", "a-model");
    render(<InputPanel />);

    // 打开思考强度下拉，选择 High（Max 的 hint 也是 reasoning_effort: high，
    // 用 /^High\s/i 匹配以 High 开头后跟空格，避免匹配到 Max 项）
    fireEvent.click(screen.getByRole("button", { name: /思考强度/i }));
    fireEvent.click(
      screen.getByRole("option", { name: /^High\s/i })
    );

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "hello" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => expect(tauriInvokeMock).toHaveBeenCalled());
    const request = tauriInvokeMock.mock.calls[0][1].request as Record<string, unknown>;
    expect(request.thinking_level).toBe("high");
  });

  it("passes thinking_level through slash-skill send_prompt path", async () => {
    setCurrentProvider("Alpha", "a-model");
    mockSkillStore.skills = [
      { name: "summarize", description: "summarize", source: "user" },
    ];
    mockSkillStore.readSkill.mockResolvedValue("SKILL BODY");

    render(<InputPanel />);

    // 选择非默认思考强度
    fireEvent.click(screen.getByRole("button", { name: /思考强度/i }));
    fireEvent.click(screen.getByRole("option", { name: /Medium/i }));

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "/summarize" } });
    // 第一次 Enter：在 slash 菜单打开时选中 slash 项，input 变成 "/summarize "
    fireEvent.keyDown(textarea, { key: "Enter" });
    // 第二次 Enter：slash 菜单已关闭，触发 handleSend → handleSlashCommand
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => expect(tauriInvokeMock).toHaveBeenCalled());
    const request = tauriInvokeMock.mock.calls[0][1].request as Record<string, unknown>;
    expect(request.thinking_level).toBe("medium");
    expect(typeof request.prompt).toBe("string");
    expect(request.prompt).toContain("SKILL BODY");
  });

  it("sends the active session ID inside request", async () => {
    setCurrentProvider("Alpha", "a-model");
    mockSessionStore.activeSessionId = "session-1";
    render(<InputPanel />);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "hello" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => expect(tauriInvokeMock).toHaveBeenCalledWith("send_prompt", {
      request: expect.objectContaining({ session_id: "session-1" }),
    }));
  });

  it("renames a default session from the first user message", async () => {
    setCurrentProvider("Alpha", "a-model");
    mockSessionStore.activeSessionId = "session-1";
    mockSessionStore.sessions = [{ id: "session-1", title: "New Session" }];
    render(<InputPanel />);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "你好，帮我分析这个项目" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => expect(mockSessionStore.renameSession).toHaveBeenCalledWith(
      "session-1",
      "你好，帮我分析这..."
    ));
  });

  it("renders top textarea row with Plus and send button, bottom toolbar with mode, model, thinking", () => {
    setCurrentProvider("Alpha", "a-model");
    render(<InputPanel />);

    // Plus 占位
    expect(screen.getByRole("button", { name: /添加|附件|Plus/i })).toBeInTheDocument();
    // 模式切换
    expect(screen.getByRole("button", { name: /模式|Agent/i })).toBeInTheDocument();
    // 模型触发器
    expect(screen.getByRole("button", { name: /Alpha|切换模型|模型/i })).toBeInTheDocument();
    // 思考强度
    expect(screen.getByRole("button", { name: /思考强度/i })).toBeInTheDocument();
  });

  it("forwards onAddProvider callback when palette add button clicked", () => {
    setCurrentProvider("Alpha", "a-model");
    const onAddProvider = vi.fn();
    render(<InputPanel onAddProvider={onAddProvider} />);

    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("button", { name: "连接提供商" }));
    expect(onAddProvider).toHaveBeenCalledTimes(1);
  });

  it("forwards onManageProviders callback when palette manage button clicked", () => {
    setCurrentProvider("Alpha", "a-model");
    const onManageProviders = vi.fn();
    render(<InputPanel onManageProviders={onManageProviders} />);

    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("button", { name: "管理模型" }));
    expect(onManageProviders).toHaveBeenCalledTimes(1);
  });

  it("does not render AddProviderDialog or ModelManagerDialog internally", () => {
    setCurrentProvider("Alpha", "a-model");
    render(<InputPanel onAddProvider={vi.fn()} onManageProviders={vi.fn()} />);

    // 打开 palette 并点击 add — 对话框不应在 InputPanel 内部打开
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("button", { name: "连接提供商" }));
    // AddProviderDialog 打开时会渲染 "连接提供商" 标题 span；
    // 但 palette 已关闭（按钮 aria-label 不是 text content），此处应无此文本
    expect(screen.queryByText("连接提供商")).not.toBeInTheDocument();

    // 同理验证 manage
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("button", { name: "管理模型" }));
    expect(screen.queryByText("管理模型")).not.toBeInTheDocument();
  });

  it("renders file ref chips when fileRefs exist", () => {
    mockChatStore.fileRefs = [
      { id: "f1", name: "test.ts", path: "src/test.ts" },
    ];
    render(<InputPanel />);
    expect(screen.getByText("test.ts")).toBeInTheDocument();
  });

  it("includes file refs in prompt on send", async () => {
    mockChatStore.fileRefs = [
      { id: "f1", name: "test.ts", path: "src/test.ts" },
    ];
    render(<InputPanel />);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "帮我看看这个文件" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => {
      expect(mockChatStore.addUserMessage).toHaveBeenCalledWith(
        expect.stringContaining("帮我看看这个文件")
      );
    });
    expect(mockChatStore.addUserMessage).toHaveBeenCalledWith(
      expect.stringContaining("src/test.ts")
    );
    await waitFor(() => {
      expect(tauriInvokeMock).toHaveBeenCalledWith("send_prompt", {
        request: expect.objectContaining({ prompt: expect.stringContaining("src/test.ts") }),
      });
    });
    expect(mockChatStore.clearFileRefs).toHaveBeenCalled();
  });
});
