import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import MessageList from "./MessageList";
import type { ChatMessage } from "../../stores/chatStore";

const mockStore = vi.hoisted(() => ({
  messages: [] as ChatMessage[],
  streamingContent: "",
  isStreaming: false,
  setDraft: vi.fn(),
  compactionNotices: [],
  dismissCompactionNotice: vi.fn(),
}));

vi.mock("../../stores/chatStore", () => ({
  useChatStore: () => mockStore,
  useActiveSessionState: () => ({
    messages: mockStore.messages,
    streamingContent: mockStore.streamingContent,
    isStreaming: mockStore.isStreaming,
    compactionNotices: mockStore.compactionNotices,
  }),
}));

describe("MessageList", () => {
  beforeEach(() => {
    mockStore.messages = [];
    mockStore.streamingContent = "";
    mockStore.isStreaming = false;
  });

  it("renders assistant and tool calls in timeline order without dividers", () => {
    mockStore.messages = [
      { id: "1", type: "assistant", content: "Hello" },
      { id: "2", type: "tool_call", toolName: "read_file", arguments: {} },
      { id: "3", type: "tool_result", toolName: "read_file", success: true, output: "..." },
      { id: "4", type: "assistant", content: "Done" },
    ];
    render(<MessageList />);

    // 生产级时间线：不再用 divider 把工具区割裂
    const dividers = document.querySelectorAll(".border-t.border-divider");
    expect(dividers).toHaveLength(0);
    expect(screen.getByText("Hello")).toBeInTheDocument();
    expect(screen.getByText("Done")).toBeInTheDocument();
  });

  it("keeps user/assistant turns without artificial dividers", () => {
    mockStore.messages = [
      { id: "1", type: "assistant", content: "Hello" },
      { id: "2", type: "user", content: "Question" },
      { id: "3", type: "assistant", content: "Answer" },
    ];
    render(<MessageList />);

    const dividers = document.querySelectorAll(".border-t.border-divider");
    expect(dividers).toHaveLength(0);
  });

  it("keeps messages in a centered content column within the scroll area", () => {
    mockStore.messages = [{ id: "1", type: "assistant", content: "Hello" }];
    render(<MessageList />);

    expect(screen.getByTestId("message-list-scroll")).toHaveClass(
      "overflow-y-auto",
      "pb-56"
    );
    expect(screen.getByTestId("chat-content-column")).toHaveClass(
      "w-full",
      "max-w-[840px]",
      "mx-auto"
    );
  });

  it("shows a waiting indicator before the first streamed token", () => {
    mockStore.messages = [{ id: "1", type: "user", content: "Question" }];
    mockStore.streamingContent = "";
    mockStore.isStreaming = true;
    render(<MessageList />);

    expect(screen.getByText("正在生成回复…")).toBeInTheDocument();
  });

  it("renders empty-state example cards with focus-ring classes", () => {
    mockStore.messages = [];
    render(<MessageList />);

    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBeGreaterThan(0);

    const firstExample = buttons[0];
    expect(firstExample).toHaveClass("hover:ring-2");
    expect(firstExample).toHaveClass("hover:ring-ring");
    expect(firstExample).toHaveClass("focus-visible:ring-2");
    expect(firstExample).toHaveClass("focus-visible:ring-ring");
    expect(firstExample).toHaveClass("transition");
  });
});