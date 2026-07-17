import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import CommandPalette from "./CommandPalette";

describe("CommandPalette", () => {
  it("renders the panel with RGoat style tokens", () => {
    render(<CommandPalette isOpen={true} onClose={vi.fn()} />);

    const panel = screen.getByRole("textbox").parentElement?.parentElement;
    expect(panel).toHaveClass("bg-surface");
    expect(panel).toHaveClass("rounded-xl");
    expect(panel).toHaveClass("shadow-lg");
    expect(panel).not.toHaveClass("shadow-2xl");
    expect(panel).toHaveClass("border-border");
  });

  it("marks the first item as active with brand tokens", () => {
    render(<CommandPalette isOpen={true} onClose={vi.fn()} />);

    const firstItem = screen.getAllByRole("button")[0];
    expect(firstItem).toHaveClass("bg-primary-subtle");
    expect(firstItem).toHaveClass("text-brand");
  });

  it("renders footer shortcut hints with unified text-2xs size", () => {
    render(<CommandPalette isOpen={true} onClose={vi.fn()} />);

    const footer = screen.getByText(/Navigate/i).parentElement;
    expect(footer).toHaveClass("text-2xs");

    const shortcuts = footer?.querySelectorAll("kbd");
    expect(shortcuts?.length).toBe(3);
    shortcuts?.forEach((kbd) => {
      expect(kbd).toHaveClass("text-2xs");
    });
  });
});