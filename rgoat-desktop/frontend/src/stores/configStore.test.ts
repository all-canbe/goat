import { describe, it, expect, vi, beforeEach } from "vitest";

const tauriInvokeMock = vi.hoisted(() => vi.fn());

vi.mock("../lib/tauri-bridge", () => ({
  tauriInvoke: tauriInvokeMock,
}));

import { useConfigStore } from "./configStore";

describe("configStore", () => {
  beforeEach(() => {
    tauriInvokeMock.mockReset();
    useConfigStore.setState({
      providers: [],
      currentProvider: "",
      hasConfiguredProvider: false,
      loading: false,
    });
  });

  it("configureProvider 使用 Tauri 所需的 camelCase 参数", async () => {
    tauriInvokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce([]);

    await useConfigStore
      .getState()
      .configureProvider("https://example.com/v1", "secret", "model-a", "demo");

    expect(tauriInvokeMock).toHaveBeenNthCalledWith(1, "configure_provider", {
      baseUrl: "https://example.com/v1",
      apiKey: "secret",
      model: "model-a",
      name: "demo",
    });
  });
});
