import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import StatusBar from "./StatusBar";

const defaultProps = {
  isStreaming: false,
  currentMode: "Agent",
  pendingApprovals: 0,
  totalChanges: 0,
  toolCallCount: 0,
  tokenUsage: { inputTokens: 0, outputTokens: 0, totalCost: 0 },
};

describe("StatusBar", () => {
  it("hides Tools statistics when toolCallCount is zero", () => {
    render(<StatusBar {...defaultProps} toolCallCount={0} />);
    expect(screen.queryByTitle("Tool calls in current session")).not.toBeInTheDocument();
  });

  it("shows Tools statistics only when toolCallCount is non-zero", () => {
    const { rerender } = render(<StatusBar {...defaultProps} toolCallCount={0} />);
    expect(screen.queryByTitle("Tool calls in current session")).not.toBeInTheDocument();

    rerender(<StatusBar {...defaultProps} toolCallCount={3} />);
    expect(screen.getByTitle("Tool calls in current session")).toBeInTheDocument();
    expect(screen.getByText("Tools 3")).toBeInTheDocument();
  });
});
