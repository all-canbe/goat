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

vi.mock("../../stores/chatStore", () => ({
  useChatStore: () => mockChatStore,
}));

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
    mockChatStore.setStreaming.mockReset();
    mockChatStore.addUserMessage.mockReset();
    mockChatStore.addSystemMessage.mockReset();
    mockChatStore.clearMessages.mockReset();
    mockChatStore.cancelAgent.mockReset();
    mockChatStore.draft = "";
    mockChatStore.setDraft.mockReset();
    mockSkillStore.skills = [];
    mockSkillStore.loadSkills.mockReset();
    mockSkillStore.readSkill.mockReset();
    mockToastStore.addToast.mockReset();
    tauriInvokeMock.mockResolvedValue({ session_id: "s-1", message: "ok" });
  });

  it("does not send thinking_level field when level is Default", async () => {
    setCurrentProvider("Alpha", "a-model");
    render(<InputPanel />);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "hello" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => expect(tauriInvokeMock).toHaveBeenCalled());
    const payload = tauriInvokeMock.mock.calls[0][1] as Record<string, unknown>;
    expect(payload).not.toHaveProperty("thinking_level");
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
    const payload = tauriInvokeMock.mock.calls[0][1] as Record<string, unknown>;
    expect(payload.thinking_level).toBe("high");
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
    const payload = tauriInvokeMock.mock.calls[0][1] as Record<string, unknown>;
    expect(payload.thinking_level).toBe("medium");
    expect(typeof payload.prompt).toBe("string");
    expect(payload.prompt).toContain("SKILL BODY");
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
});
