import { describe, it, expect, vi } from "vitest";
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
}));

describe("MessageList", () => {
  it("renders dividers between consecutive Agent messages", () => {
    mockStore.messages = [
      { id: "1", type: "assistant", content: "Hello" },
      { id: "2", type: "tool_call", toolName: "read_file", arguments: {} },
      { id: "3", type: "tool_result", toolName: "read_file", success: true, output: "..." },
      { id: "4", type: "assistant", content: "Done" },
    ];
    render(<MessageList />);

    const dividers = document.querySelectorAll(".border-t.border-divider");
    expect(dividers).toHaveLength(3);
  });

  it("does not render divider before the first message or after user messages", () => {
    mockStore.messages = [
      { id: "1", type: "assistant", content: "Hello" },
      { id: "2", type: "user", content: "Question" },
      { id: "3", type: "assistant", content: "Answer" },
    ];
    render(<MessageList />);

    const dividers = document.querySelectorAll(".border-t.border-divider");
    expect(dividers).toHaveLength(0);
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