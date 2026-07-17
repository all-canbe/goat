import { describe, it, expect, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import { useGlobalShortcuts } from "./useGlobalShortcuts";

function fireKeyDown(options: {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  target?: HTMLElement;
}) {
  const event = new KeyboardEvent("keydown", {
    key: options.key,
    ctrlKey: options.ctrlKey ?? false,
    metaKey: options.metaKey ?? false,
    shiftKey: options.shiftKey ?? false,
    bubbles: true,
    cancelable: true,
  });
  (options.target ?? window).dispatchEvent(event);
  return event;
}

describe("useGlobalShortcuts", () => {
  it("Cmd/Ctrl+Shift+N 调用 onNewSession", () => {
    const onNewSession = vi.fn();
    renderHook(() =>
      useGlobalShortcuts({
        onToggleCommandPalette: vi.fn(),
        onNewSession,
        onToggleSidebar: vi.fn(),
        onToggleRightPanel: vi.fn(),
        onFocusInput: vi.fn(),
        onCloseDialog: vi.fn(),
      })
    );

    fireKeyDown({ key: "N", ctrlKey: true, shiftKey: true });
    expect(onNewSession).toHaveBeenCalledTimes(1);

    fireKeyDown({ key: "N", metaKey: true, shiftKey: true });
    expect(onNewSession).toHaveBeenCalledTimes(2);
  });

  it("Cmd/Ctrl+Shift+N 在输入框聚焦时仍然触发", () => {
    const onNewSession = vi.fn();
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();

    renderHook(() =>
      useGlobalShortcuts({
        onToggleCommandPalette: vi.fn(),
        onNewSession,
        onToggleSidebar: vi.fn(),
        onToggleRightPanel: vi.fn(),
        onFocusInput: vi.fn(),
        onCloseDialog: vi.fn(),
      })
    );

    fireKeyDown({ key: "N", ctrlKey: true, shiftKey: true, target: input });
    expect(onNewSession).toHaveBeenCalledTimes(1);

    document.body.removeChild(input);
  });
});