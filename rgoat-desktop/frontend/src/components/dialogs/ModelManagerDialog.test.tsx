import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import ModelManagerDialog from "./ModelManagerDialog";

const mockConfigStore = vi.hoisted(() => ({
  providers: [],
  setProviderEnabled: vi.fn(),
  deleteProvider: vi.fn(),
}));

const mockToastStore = vi.hoisted(() => ({
  addToast: vi.fn(),
}));

vi.mock("../../stores/configStore", () => ({
  useConfigStore: () => mockConfigStore,
}));

vi.mock("../../stores/toastStore", () => ({
  useToastStore: () => mockToastStore,
}));

describe("ModelManagerDialog", () => {
  it("does not show the global focus-visible outline on the provider search input", () => {
    render(
      <ModelManagerDialog
        isOpen
        onClose={vi.fn()}
        onAddProvider={vi.fn()}
      />
    );

    expect(screen.getByPlaceholderText("搜索 Provider...")).toHaveClass(
      "focus-visible:outline-none"
    );
  });
});
