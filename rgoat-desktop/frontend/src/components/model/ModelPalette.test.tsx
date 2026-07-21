import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ModelPalette from "./ModelPalette";

const mockConfigStore = vi.hoisted(() => ({
  providers: [] as {
    name: string;
    model: string;
    provider_type: string;
    is_current: boolean;
    enabled: boolean;
    source: "settings" | "env" | "fallback";
  }[],
  switchProvider: vi.fn(),
}));

vi.mock("../../stores/configStore", () => ({
  useConfigStore: () => mockConfigStore,
}));

vi.mock("../../lib/tauri-bridge", () => ({
  tauriInvoke: vi.fn(),
}));

function setProviders(list: typeof mockConfigStore.providers) {
  mockConfigStore.providers = list;
}

describe("ModelPalette", () => {
  beforeEach(() => {
    mockConfigStore.switchProvider.mockReset();
    setProviders([]);
  });

  it("dropdown variant shows only enabled settings providers", () => {
    setProviders([
      { name: "Alpha", model: "a-model", provider_type: "openai_compatible", is_current: true, enabled: true, source: "settings" },
      { name: "Beta", model: "b-model", provider_type: "openai_compatible", is_current: false, enabled: false, source: "settings" },
      { name: "Gamma", model: "g-model", provider_type: "openai_compatible", is_current: false, enabled: true, source: "env" },
    ]);

    render(
      <ModelPalette
        isOpen
        onClose={vi.fn()}
        variant="dropdown"
        onAddProvider={vi.fn()}
        onManageProviders={vi.fn()}
      />
    );

    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Beta")).not.toBeInTheDocument();
    expect(screen.queryByText("Gamma")).not.toBeInTheDocument();
  });

  it("dropdown exposes add and manage entry buttons", () => {
    setProviders([
      { name: "Alpha", model: "a-model", provider_type: "openai_compatible", is_current: true, enabled: true, source: "settings" },
    ]);
    const onAdd = vi.fn();
    const onManage = vi.fn();
    render(
      <ModelPalette
        isOpen
        onClose={vi.fn()}
        variant="dropdown"
        onAddProvider={onAdd}
        onManageProviders={onManage}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "连接提供商" }));
    expect(onAdd).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "管理模型" }));
    expect(onManage).toHaveBeenCalled();
  });

  it("switches provider and closes on item click", () => {
    const onClose = vi.fn();
    setProviders([
      { name: "Alpha", model: "a-model", provider_type: "openai_compatible", is_current: false, enabled: true, source: "settings" },
    ]);
    render(
      <ModelPalette
        isOpen
        onClose={onClose}
        variant="dropdown"
        onAddProvider={vi.fn()}
        onManageProviders={vi.fn()}
      />
    );

    fireEvent.click(screen.getByText("Alpha"));
    expect(mockConfigStore.switchProvider).toHaveBeenCalledWith("Alpha");
    expect(onClose).toHaveBeenCalled();
  });

  it("shows add-provider empty state when no settings providers", () => {
    const onAdd = vi.fn();
    render(
      <ModelPalette
        isOpen
        onClose={vi.fn()}
        variant="dropdown"
        onAddProvider={onAdd}
        onManageProviders={vi.fn()}
      />
    );

    expect(screen.getByText(/No providers configured/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Add provider/i }));
    expect(onAdd).toHaveBeenCalled();
  });

  it("closes dropdown on an overlay click or Escape outside search input", () => {
    const onClose = vi.fn();
    render(
      <ModelPalette
        isOpen
        onClose={onClose}
        variant="dropdown"
        onAddProvider={vi.fn()}
        onManageProviders={vi.fn()}
      />
    );

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByTestId("model-palette-backdrop"));
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
