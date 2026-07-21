import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import AddProviderDialog from "./AddProviderDialog";

vi.mock("../provider/ProviderForm", () => ({
  default: () => <div data-testid="mock-provider-form" />,
}));

vi.mock("../../lib/tauri-bridge", () => ({
  tauriInvoke: vi.fn(),
}));

describe("AddProviderDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders nothing when closed", () => {
    render(<AddProviderDialog isOpen={false} onClose={vi.fn()} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("has dialog role, aria-modal=true, and accessible name when open", () => {
    render(<AddProviderDialog isOpen onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAccessibleName("连接提供商");
  });

  it("calls onClose when Escape pressed", () => {
    const onClose = vi.fn();
    render(<AddProviderDialog isOpen onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when overlay clicked", () => {
    const onClose = vi.fn();
    render(<AddProviderDialog isOpen onClose={onClose} />);
    // 点击遮罩层（外层 fixed 容器）
    const overlay = screen.getByRole("dialog").parentElement!;
    fireEvent.click(overlay);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
