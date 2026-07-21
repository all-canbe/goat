import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ModelManagerDialog from "./ModelManagerDialog";

const mockConfigStore = vi.hoisted(() => ({
  providers: [] as {
    name: string;
    model: string;
    provider_type: string;
    is_current: boolean;
    enabled: boolean;
    source: "settings" | "env" | "fallback";
  }[],
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

vi.mock("../../lib/tauri-bridge", () => ({
  tauriInvoke: vi.fn(),
}));

function setProviders(list: typeof mockConfigStore.providers) {
  mockConfigStore.providers = list;
}

describe("ModelManagerDialog", () => {
  beforeEach(() => {
    mockConfigStore.setProviderEnabled.mockReset();
    mockConfigStore.deleteProvider.mockReset();
    mockToastStore.addToast.mockReset();
    setProviders([]);
  });

  it("lists only settings-source providers (env/fallback hidden)", () => {
    setProviders([
      { name: "Alpha", model: "a", provider_type: "openai_compatible", is_current: true, enabled: true, source: "settings" },
      { name: "openai", model: "gpt", provider_type: "openai_compatible", is_current: false, enabled: true, source: "env" },
      { name: "deepseek", model: "d", provider_type: "openai_compatible", is_current: false, enabled: true, source: "fallback" },
    ]);

    render(<ModelManagerDialog isOpen onClose={vi.fn()} onAddProvider={vi.fn()} />);

    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.queryByText("openai")).not.toBeInTheDocument();
    expect(screen.queryByText("deepseek")).not.toBeInTheDocument();
  });

  it("disables switch and delete for current provider", () => {
    setProviders([
      { name: "Alpha", model: "a", provider_type: "openai_compatible", is_current: true, enabled: true, source: "settings" },
    ]);

    render(<ModelManagerDialog isOpen onClose={vi.fn()} onAddProvider={vi.fn()} />);

    const switchBtn = screen.getByRole("switch", { name: /切换 Alpha/i });
    expect(switchBtn).toBeDisabled();

    const deleteBtn = screen.getByRole("button", { name: /删除 Alpha/i });
    expect(deleteBtn).toBeDisabled();
  });

  it("toggles enabled state for non-current provider", async () => {
    mockConfigStore.setProviderEnabled.mockResolvedValueOnce(undefined);
    setProviders([
      { name: "Alpha", model: "a", provider_type: "openai_compatible", is_current: true, enabled: true, source: "settings" },
      { name: "Beta", model: "b", provider_type: "openai_compatible", is_current: false, enabled: true, source: "settings" },
    ]);

    render(<ModelManagerDialog isOpen onClose={vi.fn()} onAddProvider={vi.fn()} />);

    fireEvent.click(screen.getByRole("switch", { name: /切换 Beta/i }));
    await waitFor(() =>
      expect(mockConfigStore.setProviderEnabled).toHaveBeenCalledWith("Beta", false)
    );
    expect(mockToastStore.addToast).toHaveBeenCalledWith(
      expect.stringContaining("Beta"),
      "success"
    );
  });

  it("confirms before deleting a non-current provider", async () => {
    mockConfigStore.deleteProvider.mockResolvedValueOnce(undefined);
    setProviders([
      { name: "Alpha", model: "a", provider_type: "openai_compatible", is_current: true, enabled: true, source: "settings" },
      { name: "Beta", model: "b", provider_type: "openai_compatible", is_current: false, enabled: true, source: "settings" },
    ]);

    render(<ModelManagerDialog isOpen onClose={vi.fn()} onAddProvider={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /删除 Beta/i }));
    expect(screen.getByText(/确认删除 Provider/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    await waitFor(() =>
      expect(mockConfigStore.deleteProvider).toHaveBeenCalledWith("Beta")
    );
  });
});
