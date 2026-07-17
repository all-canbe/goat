import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import SettingsDialog from "./SettingsDialog";

const mockConfigStore = vi.hoisted(() => ({
  providers: [] as { name: string; model: string; provider_type: string; is_current: boolean }[],
  switchProvider: vi.fn(),
  loadProviders: vi.fn(),
}));

const mockToastStore = vi.hoisted(() => ({
  addToast: vi.fn(),
}));

const mockThemeStore = vi.hoisted(() => ({
  theme: "dark" as const,
  setTheme: vi.fn(),
}));

vi.mock("../../stores/configStore", () => ({
  useConfigStore: () => mockConfigStore,
}));

vi.mock("../../stores/toastStore", () => ({
  useToastStore: () => mockToastStore,
}));

vi.mock("../../stores/themeStore", () => ({
  useThemeStore: () => mockThemeStore,
}));

describe("SettingsDialog", () => {
  it("renders header with surface background", () => {
    render(<SettingsDialog isOpen={true} onClose={vi.fn()} />);

    const header = screen.getByText("设置").parentElement;
    expect(header).toHaveClass("bg-surface");
  });

  it("marks default Mode active with subtle brand tokens", () => {
    render(<SettingsDialog isOpen={true} onClose={vi.fn()} defaultMode="Agent" />);

    const activeMode = screen.getByRole("button", { name: "Agent" });
    expect(activeMode).toHaveClass("bg-primary-subtle");
    expect(activeMode).toHaveClass("text-brand");
    expect(activeMode).not.toHaveClass("bg-primary");
    expect(activeMode).not.toHaveClass("text-white");
  });
  it("marks inactive Mode buttons with muted text", () => {
    render(<SettingsDialog isOpen={true} onClose={vi.fn()} defaultMode="Agent" />);

    const inactiveMode = screen.getByRole("button", { name: "Plan" });
    expect(inactiveMode).toHaveClass("text-text-secondary");
    expect(inactiveMode).not.toHaveClass("bg-primary-subtle");
  });

  it("marks active theme with subtle brand tokens", () => {
    mockThemeStore.theme = "dark";
    render(<SettingsDialog isOpen={true} onClose={vi.fn()} />);

    const activeTheme = screen.getByRole("button", { name: "暗色" });
    expect(activeTheme).toHaveClass("bg-primary-subtle");
    expect(activeTheme).toHaveClass("text-brand");
    expect(activeTheme).not.toHaveClass("bg-primary");
    expect(activeTheme).not.toHaveClass("text-white");
  });

  it("keeps primary style on the done button", () => {
    render(<SettingsDialog isOpen={true} onClose={vi.fn()} />);

    const doneButton = screen.getByRole("button", { name: "完成" });
    expect(doneButton).toHaveClass("bg-primary");
    expect(doneButton).toHaveClass("text-white");
  });
});