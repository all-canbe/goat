import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ProviderForm from "./ProviderForm";

const mockConfigStore = vi.hoisted(() => ({
  configureProvider: vi.fn(),
}));

vi.mock("../../stores/configStore", () => ({
  useConfigStore: () => mockConfigStore,
}));

vi.mock("../../lib/tauri-bridge", () => ({
  tauriInvoke: vi.fn(),
}));

function fillFields() {
  fireEvent.change(screen.getByPlaceholderText("https://api.openai.com/v1"), {
    target: { value: "https://api.example.com/v1" },
  });
  fireEvent.change(screen.getByPlaceholderText("sk-..."), {
    target: { value: "key-123" },
  });
  fireEvent.change(screen.getByPlaceholderText("gpt-4o"), {
    target: { value: "gpt-test" },
  });
  fireEvent.change(screen.getByPlaceholderText("OpenAI"), {
    target: { value: "MyProvider" },
  });
}

describe("ProviderForm", () => {
  beforeEach(() => {
    mockConfigStore.configureProvider.mockReset();
  });

  it("calls configureProvider and onSuccess with trimmed values", async () => {
    mockConfigStore.configureProvider.mockResolvedValueOnce(undefined);
    const onSuccess = vi.fn();
    render(<ProviderForm onSuccess={onSuccess} />);

    fillFields();
    fireEvent.click(screen.getByRole("button", { name: /Save & Continue/i }));

    await waitFor(() => expect(onSuccess).toHaveBeenCalled());
    expect(mockConfigStore.configureProvider).toHaveBeenCalledWith(
      "https://api.example.com/v1",
      "key-123",
      "gpt-test",
      "MyProvider"
    );
  });

  it("rejects reserved provider names", async () => {
    const onSuccess = vi.fn();
    render(<ProviderForm onSuccess={onSuccess} />);

    fillFields();
    fireEvent.change(screen.getByPlaceholderText("OpenAI"), {
      target: { value: "deepseek" },
    });
    fireEvent.click(screen.getByRole("button", { name: /Save & Continue/i }));

    await waitFor(() =>
      expect(screen.getByText(/reserved for environment/i)).toBeInTheDocument()
    );
    expect(mockConfigStore.configureProvider).not.toHaveBeenCalled();
    expect(onSuccess).not.toHaveBeenCalled();
  });

  it("shows error message when configureProvider throws", async () => {
    mockConfigStore.configureProvider.mockRejectedValueOnce(
      new Error("boom")
    );
    render(<ProviderForm />);

    fillFields();
    fireEvent.click(screen.getByRole("button", { name: /Save & Continue/i }));

    await waitFor(() => expect(screen.getByText("boom")).toBeInTheDocument());
  });
});
